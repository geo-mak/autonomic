use std::borrow::Cow;
use std::sync::Arc;

use tokio::sync::watch::Receiver;

use crate::core::effector::Effector;
use crate::core::errors::ActivationError;
use crate::core::operation::{OpState, Operation, OperationInfo, OperationParameters};
use crate::core::sensor::Sensor;
use crate::core::traits::{IntoArc, IntoBox};
use crate::{trace_error, trace_trace, trace_warn};

pub(super) struct OperationContainer {
    effector: Arc<Effector>,
    sensor: Option<Sensor>,
}

impl OperationContainer {
    pub(super) fn new(operation: impl Operation + 'static, sensor: Option<Sensor>) -> Self {
        Self {
            effector: Arc::new(Effector::new(operation)),
            sensor,
        }
    }

    pub(super) fn info(&self) -> OperationInfo {
        OperationInfo::new(
            Cow::Borrowed(self.effector.id()),
            Cow::Borrowed(self.effector.describe()),
            self.effector.is_active(),
            self.effector.is_locked(),
            if let Some(ref sensor) = self.sensor {
                sensor.is_active()
            } else {
                false
            },
        )
    }

    /// Checks if operation is currently active.
    #[inline(always)]
    pub(super) fn is_active(&self) -> bool {
        self.effector.is_active()
    }

    /// Checks if operation is currently locked.
    #[inline(always)]
    pub(super) fn is_locked(&self) -> bool {
        self.effector.is_locked()
    }

    /// Locks the operation.
    /// > **Note**: It doesn't abort the operation if it is currently active.
    /// > Operation is allowed to complete, but no new activation will be allowed.
    #[inline]
    pub(super) fn lock(&self) {
        trace_trace!(source = self.effector.id(), message = "Locking requested");
        self.effector.lock();
    }

    /// Unlocks the operation **only** if it is currently locked.
    #[inline]
    pub(super) fn unlock(&self) {
        trace_trace!(source = self.effector.id(), message = "Unlock requested");
        self.effector.unlock();
    }

    /// Activates operation with optional parameters.
    ///
    /// # Parameters
    ///
    /// - `Params`: Optional parameters the operation accepts.
    ///
    /// # Returns
    /// - `Ok(())`: If activation was successful.
    /// - `Err(ActivationErr::Active)`: If the operation is already active.
    /// - `Err(ActivationErr::Locked)`: If the operation is locked.
    pub(super) fn activate(
        &self,
        params: Option<impl OperationParameters>,
    ) -> Result<(), ActivationError> {
        let op_id = self.effector.id();
        trace_trace!(source = op_id, message = "Activation requested");
        // **Safety**: Operation must not be locked
        if self.effector.is_locked() {
            trace_error!(source = op_id, message = "Operation locked");
            return Err(ActivationError::Locked);
        }
        // **Safety**: Operation must not be currently active
        if self.effector.is_active() {
            trace_warn!(source = op_id, message = "Activation denied");
            return Err(ActivationError::Active);
        }
        // Activate the operation
        self.effector
            .activate(params.map(|p| p.into_arc() as Arc<dyn OperationParameters>));
        Ok(())
    }

    /// Activates operation with optional parameters and streams its states.
    ///
    /// # Parameters
    ///
    /// - `Params`: Optional parameters the operation accepts.
    ///
    /// # Returns
    /// - `Ok(Receiver<OperationState>)`: If activation is successful.
    /// - `Err(ActivationErr::Active)`: If the operation is already active.
    /// - `Err(ActivationErr::Locked)`: If the operation is locked.
    pub(super) fn activate_stream(
        &self,
        params: Option<impl OperationParameters>,
    ) -> Result<Receiver<OpState>, ActivationError> {
        let op_id = self.effector.id();
        trace_trace!(source = op_id, message = "Activation requested");
        // **Safety**: Operation must not be locked
        if self.effector.is_locked() {
            trace_error!(source = op_id, message = "Operation locked");
            return Err(ActivationError::Locked);
        }
        // **Safety**: Operation must not be currently active
        if self.effector.is_active() {
            trace_warn!(source = op_id, message = "Operation active");
            return Err(ActivationError::Active);
        }
        // Activate the operation
        let rx = self
            .effector
            .activate_stream(params.map(|p| p.into_box() as Box<dyn OperationParameters>));
        Ok(rx)
    }

    /// Aborts the operation.
    /// > **Note**: It sends abort signal, but it does not guarantee the aborting.
    #[inline]
    pub(super) fn abort(&self) {
        trace_trace!(source = self.effector.id(), message = "Abort requested");
        self.effector.abort();
    }

    /// Activates the sensor of an operation.
    ///
    /// # Parameters
    /// - `id` - The id of the operation to start its sensor.
    ///
    /// # Returns
    /// - `Ok(())` if the sensor was successfully activated.
    /// - `Err(ActivationError::NotSet)` if the sensor is not set.
    /// - `Err(ActivationError::Active)` if the sensor is already active.
    /// - `Err(ActivationError::Locked)` if the operation is locked.
    pub(super) fn activate_sensor(&self) -> Result<(), ActivationError> {
        let op_id = self.effector.id();
        trace_trace!(source = op_id, message = "Sensor activation requested");
        if let Some(ref sensor) = self.sensor {
            // **Safety**: Operation must not be locked
            if self.effector.is_locked() {
                trace_error!(source = op_id, message = "Operation locked");
                return Err(ActivationError::Locked);
            }
            // **Safety**: Sensor must not be currently active
            if sensor.is_active() {
                trace_warn!(source = op_id, message = "Sensor Active");
                return Err(ActivationError::Active);
            }
            // Activate the sensor
            sensor.activate(self.effector.clone());
            Ok(())
        } else {
            trace_warn!(source = op_id, message = "Sensor not set");
            Err(ActivationError::NotSet)
        }
    }

    /// Deactivates a sensor of an operation.
    ///
    /// # Parameters
    /// - `id` - The id of the operation to deactivate its sensor.
    ///
    /// # Returns
    /// - `Ok(())` if the sensor has been successfully deactivated.
    /// - `Err(ActivationError::NotSet)` if the sensor was not found.
    pub(super) fn deactivate_sensor(&self) -> Result<(), ActivationError> {
        let op_id = self.effector.id();
        trace_trace!(source = op_id, message = "Sensor deactivation requested");
        if let Some(ref sensor) = self.sensor {
            sensor.deactivate();
            Ok(())
        } else {
            trace_warn!(source = op_id, message = "Sensor not set");
            Err(ActivationError::NotSet)
        }
    }

    /// Checks if the sensor is currently active.
    ///
    /// # Returns
    /// - `Ok(true)` - If sensor is active.
    /// - `Ok(false)` - If the sensor is not active.
    /// - `Err(ActivationError::NotSet)` - If sensor is None.
    pub(super) fn is_sensor_active(&self) -> Result<bool, ActivationError> {
        if let Some(ref sensor) = self.sensor {
            Ok(sensor.is_active())
        } else {
            trace_warn!(source = self.effector.id(), message = "Sensor not set");
            Err(ActivationError::NotSet)
        }
    }
}
