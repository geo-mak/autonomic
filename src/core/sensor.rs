use std::sync::Arc;

use tokio::sync::Notify;

use async_trait::async_trait;

use crate::core::effector::Effector;
use crate::core::operation::OperationParameters;
use crate::core::sync::AtomicGuard;
use crate::core::traits::IntoSensor;
use crate::{trace_error, trace_info};

/// Trait for activation conditions.
/// Condition is a suspension event that prevents `activate` method from returning until the condition is met.
/// The condition is considered met when the `activate` method returns.
#[async_trait]
pub trait ActivationCondition: Send + Sync {
    async fn activate(&self) -> Option<Arc<dyn OperationParameters>>;
}

impl<T> IntoSensor for T
where
    T: ActivationCondition + 'static,
{
    /// Transforms the activation condition into a sensor.
    #[inline]
    fn into_sensor(self) -> Sensor {
        Sensor::new(self)
    }
}

struct SensorData {
    condition: Box<dyn ActivationCondition>,
    deactivate: Notify,
    guard: AtomicGuard,
}

/// Sensor is an observer object that observes a condition, when met, it calls activation on effector.
/// Sensor accepts any type that implements `ActivationCondition` trait as its activation condition.
///
/// When activated, the sensor starts observing its condition in an infinite loop,
/// activating the effector every time the condition is met.
///
/// Sensor can be deactivated anytime using its deactivation handle.
///
/// > **Notes**:
/// > - Sensor itself is not associated with any effector. Effector is any instance provided as parameter by the container.
/// > - Both activation and deactivation are managed internally by the container.
pub struct Sensor {
    data: Arc<SensorData>,
}

impl Sensor {
    /// Creates a new `Sensor`.
    ///
    /// # Parameters
    /// - `condition`: Any type that implements `ActivationCondition` trait.
    ///
    /// # Returns
    /// New `Sensor` type.
    pub fn new(condition: impl ActivationCondition + 'static) -> Self {
        Sensor {
            data: Arc::new(SensorData {
                condition: Box::new(condition),
                deactivate: Notify::new(),
                guard: AtomicGuard::default(),
            }),
        }
    }

    /// Activates the sensor.
    ///
    /// # Safety
    ///  Guarding for activation must be ensured by the calling context.
    ///  Guarding for activation is not done by the sensor to minimize the overhead.
    ///
    /// # Parameters
    /// - `effector`: a counted reference to the effector that controls the operation.
    pub(super) fn activate(&self, effector: Arc<Effector>) {
        // Activate guard to prevent new activation
        self.data.guard.activate();
        let op_id: &str = effector.id();
        trace_info!(source = op_id, message = "Sensor Activated");
        // Cloned reference for the execution block
        let data_ref = self.data.clone();
        // Execution domain
        tokio::spawn(async move {
            tokio::select! {
                // Deactivation requested
                _ = data_ref.deactivate.notified() => {
                    // Deactivate guard
                    data_ref.guard.deactivate();
                    trace_info!(
                        source = op_id,
                        message = "Sensor Deactivated"
                    );
                },
                _ = async {
                    loop {
                        let params= data_ref.condition.activate().await;
                        // **Safety**: effector must not be locked
                        if effector.is_locked(){
                            // Deactivating sensor with notify might not be fast enough to avoid new activation
                            // So, we deactivate the guard directly and break the loop immediately
                            // No new activations should be allowed after this point
                            data_ref.guard.deactivate();
                            trace_error!(source = op_id, message = "Sensor Deactivated");
                            break;
                        }
                        // **Safety**: effector must not be active
                        if !effector.is_active(){
                           effector.activate(params);
                        }
                    }
                } => {}
            }
        });
    }

    /// Deactivates the sensor.
    #[inline]
    pub(super) fn deactivate(&self) {
        self.data.deactivate.notify_one();
    }

    /// Checks if the sensor is currently active.
    #[inline]
    pub(super) fn is_active(&self) -> bool {
        self.data.guard.is_active()
    }
}