use std::time::Duration;

use async_trait::async_trait;

use autonomic_events::trace_info;

use crate::operation::{Operation, OperationParameters, OperationResult};
use crate::testkit::params::TestRetry;
use crate::traits::{Describe, Identity};

enum RunMode {
    Ok,
    Err,
    Panic,
    Lock,
}

pub struct TestOperation {
    id: &'static str,
    description: &'static str,
    delay: Option<Duration>,
    mode: RunMode,
}

impl TestOperation {
    pub fn ok(id: &'static str, description: &'static str, delay: Option<Duration>) -> Self {
        Self {
            id,
            description,
            delay,
            mode: RunMode::Ok,
        }
    }

    pub fn err(id: &'static str, description: &'static str, delay: Option<Duration>) -> Self {
        Self {
            id,
            description,
            delay,
            mode: RunMode::Err,
        }
    }

    pub fn panic(id: &'static str, description: &'static str, delay: Option<Duration>) -> Self {
        Self {
            id,
            description,
            delay,
            mode: RunMode::Panic,
        }
    }

    pub fn lock(id: &'static str, description: &'static str, delay: Option<Duration>) -> Self {
        Self {
            id,
            description,
            delay,
            mode: RunMode::Lock,
        }
    }

    async fn run(delay: Option<Duration>, mode: &RunMode) -> OperationResult {
        if let Some(delay) = delay {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await
            };
        }

        match mode {
            RunMode::Ok => OperationResult::ok_msg("Result"),
            RunMode::Err => OperationResult::err_msg("Expected Error"),
            RunMode::Panic => {
                panic!("Unexpected Error")
            }
            RunMode::Lock => OperationResult::lock("Lock Operation"),
        }
    }
}

impl Identity for TestOperation {
    type ID = &'static str;

    #[inline]
    fn id(&self) -> Self::ID {
        self.id
    }
}

impl Describe for TestOperation {
    type Description = &'static str;

    #[inline]
    fn describe(&self) -> Self::Description {
        self.description
    }
}

#[async_trait]
impl Operation for TestOperation {
    async fn perform(&self, parameters: Option<&dyn OperationParameters>) -> OperationResult {
        // Check parameters
        let params_option = if let Some(params) = parameters {
            match params.as_parameters().downcast_ref::<TestRetry>() {
                None => return OperationResult::err_msg("Unexpected parameters"),
                Some(value) => Some(value),
            }
        } else {
            None
        };

        // Get result
        let mut result = Self::run(self.delay, &self.mode).await;

        // Retry is only relevant if result is err
        if result.is_err() {
            match params_option {
                None => {}
                Some(retry_params) => {
                    if retry_params.retries > 0 {
                        let id_str = self.id;
                        for i in 1..=retry_params.retries {
                            tokio::time::sleep(Duration::from_millis(retry_params.delay_ms as u64))
                                .await;
                            trace_info!(
                                source = id_str,
                                message = format!("Attempt={} to perform", i)
                            );
                            // Rerun inner operation
                            result = Self::run(self.delay, &self.mode).await;
                            if result.is_ok() {
                                break;
                            };
                        }
                    };
                }
            };
        };

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serde::AnySerializable;
    use crate::testkit::tracing::init_tracing;

    #[test]
    fn test_op_new() {
        let op = TestOperation::ok("test operation", "this is a new test operation", None);
        assert_eq!(op.id(), "test operation");
        assert_eq!(op.describe(), "this is a new test operation");
    }

    #[tokio::test]
    async fn test_activate_op_ok() {
        init_tracing();
        let op = TestOperation::ok(
            "test operation",
            "this is a test operation that succeeds",
            None,
        );
        let result = op.perform(None).await;
        assert_eq!(result, OperationResult::ok_msg("Result"));
    }

    #[tokio::test]
    async fn test_activate_op_err() {
        init_tracing();
        let op = TestOperation::err(
            "test operation",
            "this is a test operation that fails",
            None,
        );
        let result = op.perform(None).await;
        assert_eq!(result, OperationResult::err_msg("Expected Error"));
    }

    #[tokio::test]
    #[should_panic(expected = "Unexpected Error")]
    async fn test_activate_op_panics() {
        init_tracing();
        let op = TestOperation::panic(
            "test operation",
            "this is a test operation that panics",
            None,
        );
        let _ = op.perform(None).await;
    }

    #[tokio::test]
    async fn test_activate_with_params() {
        init_tracing();
        let operation = TestOperation::err("test operation", "this is test operation", None);

        let params = AnySerializable::new_register(TestRetry::new(3, 1000));

        let result = operation.perform(Some(&params)).await;

        assert_eq!(result, OperationResult::err_msg("Expected Error"));
    }
}
