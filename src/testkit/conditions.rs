use std::fmt::Debug;
use std::sync::Arc;

use tokio::time::{sleep, Duration};

use async_trait::async_trait;

use crate::core::operation::OperationParameters;
use crate::core::sensor::ActivationCondition;

#[derive(Clone, Debug)]
pub struct TestIntervalCondition {
    interval: u32, // interval in seconds
    parameters: Option<Arc<dyn OperationParameters>>,
}

impl TestIntervalCondition {
    ///
    /// # Parameters
    /// - `interval`: The interval in `seconds`.
    /// - `parameters`: The operation parameters.
    ///
    /// > **Note**: `interval` must be greater than `0`. If `interval` is `0`, it will be set to `1`.
    pub fn new(interval: u32, parameters: Option<Arc<dyn OperationParameters>>) -> Self {
        TestIntervalCondition {
            interval: interval.max(1),
            parameters,
        }
    }
}

#[async_trait]
impl ActivationCondition for TestIntervalCondition {
    async fn activate(&self) -> Option<Arc<dyn OperationParameters>> {
        let interval = self.interval;
        sleep(Duration::from_secs(interval as u64)).await;
        self.parameters.clone()
    }
}
