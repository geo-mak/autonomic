use std::any::Any;

use serde::{Deserialize, Serialize};

use autonomic_core::operation::OperationParameters;

/// Common parameters shared between operations

/// Parameters for retrying action according to a specified condition.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Retry {
    pub retries: u8,
    pub delay_ms: u32,
}

impl Retry {
    /// Creates new `Retry` instance.
    /// # Parameters
    /// - `retries`: Number of retries.
    /// - `delay_ms`: The delay between retries in `milliseconds`.
    pub fn new(retries: u8, delay_ms: u32) -> Self {
        Self { retries, delay_ms }
    }
}

impl OperationParameters for Retry {
    fn as_parameters(&self) -> &dyn Any {
        self
    }
}
