use std::collections::HashMap;

use tokio_stream::wrappers::WatchStream;

use autonomic_events::trace_warn;

use crate::controller::{ControlUnit, Controller};
use crate::controller::{ControllerInfo, OpState};
use crate::errors::ControllerError;

/// Controller manager is responsible for managing access to controllers.
pub struct ControllerManager {
    controllers: HashMap<&'static str, ControlUnit>,
}

impl ControllerManager {
    pub const ID: &str = "Controller Manager";

    pub fn new() -> Self {
        Self {
            controllers: HashMap::new(),
        }
    }

    /// Returns the number of operations submitted to the controller.
    #[inline]
    pub fn count(&self) -> usize {
        self.controllers.len()
    }

    /// Submits a controller to the manager.
    ///
    /// # Panics
    /// If a controller with the same ID already submitted in the controller.
    pub fn submit(&mut self, controller: impl Controller + 'static) {
        let id = controller.id();
        if self.controllers.contains_key(id) {
            panic!("Controller with ID={id} already submitted");
        } else {
            self.controllers.insert(id, ControlUnit::new(controller));
        }
    }

    /// Retrieves an immutable reference to a controller by its id.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(&Controller)`: If the controller is found.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    fn get(&self, id: &str) -> Result<&ControlUnit, ControllerError> {
        if let Some(controller) = self.controllers.get(id) {
            return Ok(controller);
        }
        trace_warn!(message = format!("Controller={} not found", id));
        Err(ControllerError::NotFound)
    }

    /// Retrieves the info of a controller.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the ControllerInfo.
    ///
    /// # Returns
    /// - `Ok(ControllerInfo)`: If the operation is found.
    /// - `Err(ControllerError::NotFound)`: If the operation is not found.
    pub fn controller(&self, id: &str) -> Result<ControllerInfo, ControllerError> {
        match self.get(id) {
            Ok(controller) => Ok(controller.info()),
            Err(e) => Err(e),
        }
    }

    /// Retrieves info of all controllers.
    ///
    /// # Returns
    /// - `Ok(Vec<ControllerInfo>)`: If the manager in not empty.
    /// - `Err(ControllerError::NoResults)`: If the manager is empty.
    pub fn list(&self) -> Result<Vec<ControllerInfo>, ControllerError> {
        if self.controllers.is_empty() {
            trace_warn!(message = "Empty");
            return Err(ControllerError::NoResults);
        }
        Ok(self
            .controllers
            .values()
            .map(|controller| controller.info())
            .collect())
    }

    /// Retrieves a list of controllers with performing operations.
    ///
    /// # Returns
    /// - `Ok(Vec<&str>)`: If there were results, where `&str` represents the ID of controller.
    /// - `Err(ControllerError::NoResults)`: If there are no controllers with active operations.
    pub fn list_performing(&self) -> Result<Vec<&str>, ControllerError> {
        if self.controllers.is_empty() {
            trace_warn!(message = "Empty");
            return Err(ControllerError::NoResults);
        }
        let running_ops: Vec<&str> = self
            .controllers
            .iter()
            .filter(|(_, controller)| controller.performing())
            .map(|(id, _)| *id)
            .collect();

        if !running_ops.is_empty() {
            return Ok(running_ops);
        }
        Err(ControllerError::NoResults)
    }

    /// Number of controllers with performing operations.
    pub fn performing_count(&self) -> usize {
        self.controllers
            .iter()
            .filter(|(_, controller)| controller.performing())
            .count()
    }

    /// Checks if the control operation of controller is performing.
    ///
    /// # Parameters
    /// - `id`: The identifier of the controller
    ///
    /// # Returns
    /// - `Ok(true)`: If the controller is executing.
    /// - `Ok(false)`: If the controller is not executing.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn is_performing(&self, id: &str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(controller) => Ok(controller.performing()),
            Err(e) => Err(e),
        }
    }

    /// Checks if controller is currently locked.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(true)`: If the controller is locked.
    /// - `Ok(false)`: If the controller is not locked.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn is_locked(&self, id: &str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(controller) => Ok(controller.locked()),
            Err(e) => Err(e),
        }
    }

    /// Locks the controller.
    ///
    /// > **Note**: It doesn't abort the operation if it is currently active.
    /// > Operation is allowed to complete, but no new activation will be allowed.
    ///
    /// # Parameters
    /// - `id`: The identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(())`: If locking was successful.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn lock(&self, id: &str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                controller.lock();
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
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn unlock(&self, id: &str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                controller.unlock();
                Ok(())
            }

            Err(e) => Err(e),
        }
    }

    /// Starts the control operation of a controller and streams its states.
    ///
    /// # Returns
    /// - `Ok(WatchStream<OpState>)`: If starting was successful.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    /// - `Err(ControllerError::Active)`: If the operation is already performing.
    /// - `Err(ControllerError::Locked)`: If the controller is locked.
    pub fn perform(&'static self, id: &str) -> Result<WatchStream<OpState>, ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                let stream = controller.perform()?;
                Ok(WatchStream::new(stream))
            }
            Err(e) => Err(e),
        }
    }

    /// Aborts the control operation if it is currently performing.
    /// > **Note**: It sends abort signal, but it does not guarantee the aborting.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(())`: If an abort signal is sent to the controller and no error is occurred.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn abort(&self, id: &str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                controller.abort();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Activates the sensor of a controller.
    ///
    /// # Parameters
    /// - `id`: The unique identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been activated.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    /// - `Err(ControllerError::Active)`: if the sensor is already active.
    /// - `Err(ControllerError::Locked)` if the controller is locked.
    pub fn start_sensor(&'static self, id: &str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                controller.start_sensor()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Deactivates sensor of the a controller.
    ///
    /// # Parameters
    /// - `id`: The identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(())`: If sensor has been deactivated.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn stop_sensor(&self, id: &str) -> Result<(), ControllerError> {
        match self.get(id) {
            Ok(controller) => {
                controller.stop_sensor();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Checks if sensor of a controller is currently active.
    ///
    /// # Parameters
    /// - `id`: The identifier of the controller.
    ///
    /// # Returns
    /// - `Ok(true)`: If e is active.
    /// - `Ok(false)`: If e is inactive.
    /// - `Err(ControllerError::NotFound)`: If the controller is not found.
    pub fn sensing(&self, id: &str) -> Result<bool, ControllerError> {
        match self.get(id) {
            Ok(controller) => Ok(controller.sensing()),
            Err(e) => Err(e),
        }
    }

    /// Starts the sensors of all registered controllers.
    /// **Note**: Errors are ignored.
    pub fn start(&'static self) {
        for controller in self.controllers.values() {
            let _ = controller.start_sensor();
        }
    }

    /// Converts the instance into static reference (practically leaking it).
    pub fn into_static(self) -> &'static mut Self {
        Box::leak(Box::new(self))
    }
}
