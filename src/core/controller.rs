use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_stream::wrappers::WatchStream;

use crate::core::container::OperationContainer;
use crate::core::errors::ControllerError;
use crate::core::operation::{OpState, Operation, OperationInfo, OperationParameters};
use crate::core::sensor::{ActivationCondition, Sensor};
use crate::core::serde::{DeserializeRegistry, GenericSerializable};
use crate::core::traits::{Identity, IntoArc};
use crate::trace_warn;

/// Operation controller is responsible for managing access to operations and activating them.
///
/// > **Notes**:
/// > - Controller **must** have a unique ID.
/// > - Each operation **must** have a unique ID.
/// > - Controller's ID acts as a **namespace** for its operations.
/// > - If the controller's ID is not unique, it will cause **namespace collision** and its service will not work as expected.
pub struct OperationController<'a> {
    id: &'a str,
    containers: HashMap<&'a str, OperationContainer>,
}

impl<'a> OperationController<'a> {
    /// Creates a new `OperationController`.
    pub fn new(id: &'a str) -> Self {
        Self {
            id,
            containers: HashMap::new(),
        }
    }

    /// Checks if there are no operations in the controller.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.containers.is_empty()
    }

    /// Returns the number of operations submitted to the controller.
    #[inline]
    pub fn count(&self) -> usize {
        self.containers.len()
    }

    /// Submits an operation to the controller.
    /// The operation will be owned by the controller and will be dropped when the controller is dropped.
    ///
    /// > **Note**: If operation accepts a parameters-type that is carried by `AnySerializable`,
    /// > you need to register it, otherwise deserialization will fail.
    /// > Therefore, consider using `submit_parameters` method instead.
    /// > You can manually register the type using `DeserializeRegistry`, but this **must** be done before the start of the remote service.
    ///
    /// # Parameters
    /// - `operation`: The operation to submit.
    /// - `sensor`: An optional sensor for the operation.
    ///
    /// # Panics
    /// If an operation with the same ID already submitted in the controller.
    pub fn submit(&mut self, operation: impl Operation + 'static, sensor: Option<Sensor>) {
        if self.containers.contains_key(operation.id()) {
            panic!("Operation with ID={} already submitted", operation.id());
        } else {
            self.containers
                .insert(operation.id(), OperationContainer::new(operation, sensor));
        }
    }

    /// Submits an operation to the controller and registers its parameters' type to be carried by `AnySerializable`.
    /// The operation will be owned by the controller and will be dropped when the controller is dropped.
    ///
    /// # Generic Parameters
    /// - `T`: The type of parameters the operation expects.
    ///
    /// # Parameters
    /// - `operation`: The operation to submit.
    /// - `sensor`: An optional sensor for the operation.
    ///
    /// # Panics
    /// If an operation with the same ID already submitted in the controller.
    pub fn submit_parameters<T>(
        &mut self,
        operation: impl Operation + 'static,
        sensor: Option<Sensor>,
    ) where
        T: OperationParameters + GenericSerializable + for<'de> Deserialize<'de> + 'static,
    {
        self.submit(operation, sensor);
        DeserializeRegistry::register::<T>();
    }

    /// Retrieves an immutable reference to an operation by its ID.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation.
    ///
    /// # Returns
    /// - `Ok(&OperationContainer)`: If the operation is found.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    fn get(&self, id: &'a str) -> Result<&OperationContainer, ControllerError> {
        if self.containers.is_empty() {
            trace_warn!(source = self.id, message = "Empty");
            return Err(ControllerError::Empty);
        }
        if let Some(container) = self.containers.get(id) {
            return Ok(container);
        }
        trace_warn!(
            source = self.id,
            message = format!("Operation={} not found", id)
        );
        Err(ControllerError::OpNotFound)
    }

    /// Retrieves a mutable reference to an operation by its ID.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation.
    ///
    /// # Returns
    /// - `Ok(&mut OperationContainer)`: If the operation is found.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    fn get_mut(&mut self, id: &'a str) -> Result<&mut OperationContainer, ControllerError> {
        if self.containers.is_empty() {
            trace_warn!(source = self.id, message = "Empty");
            return Err(ControllerError::Empty);
        }
        if let Some(container) = self.containers.get_mut(id) {
            return Ok(container);
        }
        trace_warn!(
            source = self.id,
            message = format!("Operation={} not found", id)
        );
        Err(ControllerError::OpNotFound)
    }

    /// Retrieves info of single operation.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation.
    ///
    /// # Returns
    /// - `Ok(OperationInfo)`: If the operation is found.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn operation(&self, id: &'a str) -> Result<OperationInfo, ControllerError> {
        match self.get(id) {
            Ok(container) => Ok(container.info()),
            Err(e) => Err(e),
        }
    }

    /// Retrieves info of all operations.
    ///
    /// # Returns
    /// - `Ok(Vec<OperationInfo>)`: If the controller in not empty.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    pub fn operations(&self) -> Result<Vec<OperationInfo>, ControllerError> {
        if self.containers.is_empty() {
            trace_warn!(source = self.id, message = "Empty");
            return Err(ControllerError::Empty);
        }
        Ok(self
            .containers
            .values()
            .map(|container| container.info())
            .collect())
    }

    /// Retrieves a list of active operations.
    ///
    /// # Returns
    /// - `Ok(Vec<&str>)`: If controller is not empty, where `&str` represents the ID of the active operation.
    /// - `Err(ControllerError::Empty)`: If there are no operations in the controller.
    /// - `Err(ControllerError::NoActiveOps)`: If there are no **active** operations in the controller.
    pub fn active_operations(&self) -> Result<Vec<&'a str>, ControllerError> {
        if self.containers.is_empty() {
            trace_warn!(source = self.id, message = "Empty");
            return Err(ControllerError::Empty);
        }
        let running_ops: Vec<&'a str> = self
            .containers
            .iter()
            .filter(|(_, container)| container.is_active())
            .map(|(id, _)| *id)
            .collect();

        if !running_ops.is_empty() {
            return Ok(running_ops);
        }
        Err(ControllerError::NoActiveOps)
    }

    /// Number of operations currently active.
    pub fn active_count(&self) -> usize {
        self.containers
            .iter()
            .filter(|(_, container)| container.is_active())
            .count()
    }

    /// Checks if operation is currently active.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to check.
    ///
    /// # Returns
    /// - `Ok(true)`: If the operation is active.
    /// - `Ok(false)`: If the operation is not active.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn is_active(&self, id: &'a str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(container) => Ok(container.is_active()),
            Err(e) => Err(e),
        }
    }

    /// Checks if operation is currently locked.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to check.
    ///
    /// # Returns
    /// - `Ok(true)`: If the operation is locked.
    /// - `Ok(false)`: If the operation is not locked.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn is_locked(&self, id: &'a str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(container) => Ok(container.is_locked()),
            Err(e) => Err(e),
        }
    }

    /// Locks the operation.
    ///
    /// > **Note**: It doesn't abort the operation if it is currently active.
    /// > Operation is allowed to complete, but no new activation will be allowed.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to abort.
    ///
    /// # Returns
    /// - `Ok(())`: If locking was successful.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn lock(&self, id: &'a str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(container) => {
                container.lock();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Unlocks the operation **only** if it is currently locked.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to abort.
    ///
    /// # Returns
    /// - `Ok(())`: If unlocking was successful.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn unlock(&self, id: &'a str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(container) => {
                container.unlock();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Activates the operation with optional parameters.
    ///
    /// > **Note**: The result of the operation can be retrieved from the storage API.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to run.
    /// - `params`: Optional parameters for the operation.
    ///
    /// # Returns
    /// - `Ok(())`: If activation was successful.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    /// - `Err(ControllerError::ActivationErr::Active)`: If the operation is already active.
    /// - `Err(ControllerError::ActivationErr::Locked)`: If the operation is locked.
    pub fn activate<T>(&self, id: &'a str, params: Option<T>) -> Result<(), ControllerError>
    where
        T: OperationParameters,
    {
        match self.get(id) {
            // Controller Ok
            Ok(container) => match container.activate(params) {
                Ok(_) => Ok(()),
                Err(e) => Err(ControllerError::ActivationErr(e)),
            },
            // Controller err
            Err(e) => Err(e),
        }
    }

    /// Activates operation with optional parameters and streams its states.
    ///
    /// > **Notes**:
    /// > - This method is useful for activation requests where live state updates are needed without fetching result.
    /// > - Operation's state is streamed as a stream of `OpState` using `WatchStream` using unique channel per call.
    /// > - The streaming channel is ephemeral, and will be closed when the operation is done.
    /// > - Active receiver can still get unreceived messages in the stream after channel closes.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to run.
    /// - `params`: Optional parameters for the operation.
    ///
    /// # Returns
    /// - `Ok(WatchStream<OpState>)`: If activation was successful.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    /// - `Err(ControllerError::ActivationErr::Active)`: If the operation is already active.
    /// - `Err(ControllerError::ActivationErr::Locked)`: If the operation is locked.
    pub fn activate_stream<T>(
        &self,
        id: &'a str,
        params: Option<T>,
    ) -> Result<WatchStream<OpState>, ControllerError>
    where
        T: OperationParameters,
    {
        match self.get(id) {
            // Controller Ok
            Ok(container) => match container.activate_stream(params) {
                Ok(stream) => Ok(WatchStream::new(stream)),
                Err(e) => Err(ControllerError::ActivationErr(e)),
            },
            // Controller err
            Err(e) => Err(e),
        }
    }

    /// Aborts the operation.
    /// > **Note**: It sends abort signal, but it does not guarantee the aborting.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation to abort.
    ///
    /// # Returns
    /// - `Ok(())`: If an abort signal is sent to the operation and no error is occurred.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn abort(&self, id: &'a str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(container) => {
                if container.is_active() {
                    container.abort();
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Sets or updates sensor associated with operation from condition.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation associated with sensor.
    /// - `condition`: The condition to create sensor from.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been set or updated successfully.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn sensor_from(
        &mut self,
        id: &'a str,
        condition: impl ActivationCondition + 'static,
    ) -> Result<(), ControllerError> {
        match self.get_mut(id) {
            Ok(container) => {
                let sensor = Sensor::new(condition);
                container.update_sensor(Some(sensor));
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Sets, updates or removes sensor associated with operation.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation associated with sensor.
    /// - `sensor`: The optional value of type `Sensor`. If it is `None`, sensor will be set to `None` also.
    ///
    /// > **Note**:
    /// > If the passed sensor is:
    /// > - `None`: The current sensor will be deactivated and removed.
    /// > - `Some`: The current active sensor will be deactivated and replaced without automatic activation of the new sensor.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been set or updated successfully.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    pub fn update_sensor(
        &mut self,
        id: &'a str,
        sensor: Option<Sensor>,
    ) -> Result<(), ControllerError> {
        match self.get_mut(id) {
            Ok(container) => {
                container.update_sensor(sensor);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Activates the sensor associated with the operation.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation associated with sensor.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been activated.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    /// - `Err(ControllerError::ActivationError::NotSet)`: if the sensor is not set.
    /// - `Err(ControllerError::ActivationError::Active)`: if the sensor is already active.
    /// - `Err(ControllerError::ActivationError::Locked)` if the operation is locked.
    pub fn activate_sensor(&self, id: &'a str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(container) => match container.activate_sensor() {
                Ok(_) => Ok(()),
                Err(e) => Err(ControllerError::ActivationErr(e)),
            },
            Err(e) => Err(e),
        }
    }

    /// Deactivates sensor associated with the operation.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation associated with the sensor.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been deactivated.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    /// - `Err(ControllerError::ActivationError::NotSet)`: if the sensor is not set.
    pub fn deactivate_sensor(&self, id: &'a str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(container) => match container.deactivate_sensor() {
                Ok(_) => Ok(()),
                Err(e) => Err(ControllerError::ActivationErr(e)),
            },
            Err(e) => Err(e),
        }
    }

    /// Checks if sensor is currently active.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the operation associated with sensor.
    ///
    /// # Returns
    /// - `Ok(true)`: If sensor is active.
    /// - `Ok(false)`: If sensor is inactive.
    /// - `Err(ControllerError::Empty)`: If the controller is empty.
    /// - `Err(ControllerError::OpNotFound)`: If the operation is not found.
    /// - `Err(ControllerError::ActivationError::NotSet)`: If sensor is None.
    pub fn is_sensor_active(&self, id: &'a str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(container) => match container.is_sensor_active() {
                Ok(state) => Ok(state),
                Err(e) => Err(ControllerError::ActivationErr(e)),
            },
            Err(e) => Err(e),
        }
    }
}

impl<'a> Identity for OperationController<'a> {
    type ID = &'a str;
    #[inline]
    fn id(&self) -> Self::ID {
        self.id
    }
}

impl IntoArc for OperationController<'_> {
    /// Converts the controller into an `Arc`.
    #[inline]
    fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}
