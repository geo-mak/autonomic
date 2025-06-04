use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::operation::OperationParameters;

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
