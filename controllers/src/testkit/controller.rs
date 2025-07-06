use std::time::Duration;

use async_trait::async_trait;

use crate::controller::{ControlContext, Controller, ControllerResult};

#[derive(Clone)]
enum RunMode {
    Ok,
    Err,
    Panic,
}

#[derive(Clone)]
pub struct TestController {
    id: &'static str,
    run_delay: Option<Duration>,
    mode: RunMode,
    notify_interval: u32,
}

impl TestController {
    pub fn ok(id: &'static str, run_delay: Option<Duration>, notify_interval: u32) -> Self {
        Self {
            id,
            run_delay,
            mode: RunMode::Ok,
            notify_interval,
        }
    }

    pub fn err(id: &'static str, run_delay: Option<Duration>, notify_interval: u32) -> Self {
        Self {
            id,
            run_delay,
            mode: RunMode::Err,
            notify_interval,
        }
    }

    pub fn panic(id: &'static str, run_delay: Option<Duration>, notify_interval: u32) -> Self {
        Self {
            id,
            run_delay,
            mode: RunMode::Panic,
            notify_interval,
        }
    }

    async fn run_mode_action(&self) -> ControllerResult {
        if let Some(run_delay) = self.run_delay {
            tokio::time::sleep(run_delay).await;
        }
        match self.mode {
            RunMode::Ok => ControllerResult::Ok,
            RunMode::Err => ControllerResult::err("Expected Error"),
            RunMode::Panic => panic!("Unexpected Error"),
        }
    }
}

#[async_trait]
impl Controller for TestController {
    async fn perform(&self, ctx: &ControlContext) -> ControllerResult {
        tokio::select! {
            _ = ctx.abort() => ControllerResult::Abort,
            result = self.run_mode_action() => result,
        }
    }

    async fn notified(&self) {
        let notify_interval = self.notify_interval;
        tokio::time::sleep(Duration::from_secs(notify_interval as u64)).await;
    }

    fn id(&self) -> &'static str {
        self.id
    }

    fn description(&self) -> &'static str {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonomic_events::testkit::global::init_tracing;

    static CTRL_ID: &str = "test_ctrl";

    #[tokio::test]
    async fn test_activate_op_ok() {
        init_tracing();
        let controller = TestController::ok(CTRL_ID, None, 0);
        let ctx = ControlContext::new();
        let result = controller.perform(&ctx).await;
        assert_eq!(result, ControllerResult::Ok);
    }

    #[tokio::test]
    async fn test_ctrl_error() {
        init_tracing();
        let controller = TestController::err(CTRL_ID, None, 0);
        let ctx = ControlContext::new();
        let result = controller.perform(&ctx).await;
        assert_eq!(result, ControllerResult::err("Expected Error"));
    }

    #[tokio::test]
    #[should_panic(expected = "Unexpected Error")]
    async fn test_ctrl_panic() {
        init_tracing();
        let controller = TestController::panic(CTRL_ID, None, 0);
        let ctx = ControlContext::new();
        let _ = controller.perform(&ctx).await;
    }
}
