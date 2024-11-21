use crate::core::operation::OperationParameters;
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TestRetry {
    pub retries: u8,
    pub delay_ms: u32,
}

impl TestRetry {
    pub fn new(retries: u8, delay_ms: u32) -> Self {
        Self { retries, delay_ms }
    }
}

impl OperationParameters for TestRetry {
    fn as_parameters(&self) -> &dyn Any {
        self
    }
}
