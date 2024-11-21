use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;

use chrono::{Local, NaiveTime};

use tokio::time::{sleep, Duration};

use crate::core::operation::OperationParameters;
use crate::core::sensor::ActivationCondition;

#[derive(Clone, Debug)]
pub struct IntervalCondition {
    interval: u32, // interval in seconds
    parameters: Option<Arc<dyn OperationParameters>>,
}

impl IntervalCondition {
    /// Creates a new `IntervalCondition` instance.
    ///
    /// # Parameters
    /// - `interval`: The interval in `seconds`.
    /// - `parameters`: The operation parameters.
    ///
    /// > **Note**: `interval` must be greater than `0`. If `interval` is `0`, it will be set to `1`.
    pub fn new(interval: u32, parameters: Option<Arc<dyn OperationParameters>>) -> Self {
        IntervalCondition {
            interval: interval.max(1),
            parameters,
        }
    }
}

#[async_trait]
impl ActivationCondition for IntervalCondition {
    async fn activate(&self) -> Option<Arc<dyn OperationParameters>> {
        let interval = self.interval;
        sleep(Duration::from_secs(interval as u64)).await;
        self.parameters.clone()
    }
}

#[derive(Clone, Debug)]
pub struct TimeCondition {
    time: NaiveTime,
    parameters: Option<Arc<dyn OperationParameters>>,
}

impl TimeCondition {
    /// Creates a new `TimeCondition` instance.
    ///
    /// # Parameters
    /// - `time`: The time to active condition at.
    /// - `parameters`: The operation parameters.
    pub fn new(time: NaiveTime, parameters: Option<Arc<dyn OperationParameters>>) -> Self {
        TimeCondition { time, parameters }
    }
}

#[async_trait]
impl ActivationCondition for TimeCondition {
    async fn activate(&self) -> Option<Arc<dyn OperationParameters>> {
        let time = self.time;
        let now = Local::now().naive_local().time();
        let duration = if now < time {
            time - now
        } else {
            chrono::Duration::days(1) - (now - time)
        };
        sleep(duration.to_std().unwrap()).await;
        self.parameters.clone()
    }
}
