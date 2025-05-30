use std::future::Future;

use futures_util::Stream;

use crate::operation::{OpState, OperationInfo};
use crate::serde::AnySerializable;

/// Service trait for a controller-service.
/// The service provides methods for interacting with the controller.
pub trait ControllerService {
    /// The controller type or the required wrapper around it as a service dependency.
    type ControllerParameter;

    /// The type of the error returned by the service.
    type ServiceError;

    /// The type of the operation ID or the required wrapper around it.
    type OperationIDParameter;

    /// The type of the result returned when calling the `operation` method.
    type OperationReturn;

    /// The type of the result returned when calling the `operations` method.
    type OperationsReturn;

    /// The type of the result returned when calling the `active_operations` method.
    type ActiveOperationsReturn;

    /// The type of the parameters or wrappers around parameters for methods `activate` and `activate_stream`.
    type ActivationParamsOption;

    /// The type of the result returned when calling the `activate` method.
    type ActivateReturn;

    /// The type of the result returned when calling the `activate_stream` method.
    type ActivateStreamReturn;

    /// The type of the result returned when calling the `abort` method.
    type AbortReturn;

    /// The type of the result returned when calling the `lock` method.
    type LockReturn;

    /// The type of the result returned when calling the `unlock` method.
    type UnlockReturn;

    /// The type of the result returned when calling the `activate_sensor` method.
    type ActivateSensorReturn;

    /// The type of the result returned when calling the `deactivate_sensor` method.
    type DeactivateSensorReturn;

    /// Returns the information about the operation.
    fn operation(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::OperationReturn, Self::ServiceError>> + Send;

    /// Returns the information about all operations.
    fn operations(
        controller: Self::ControllerParameter,
    ) -> impl Future<Output = Result<Self::OperationsReturn, Self::ServiceError>> + Send;

    /// Returns the IDs of all active operations.
    fn active_operations(
        controller: Self::ControllerParameter,
    ) -> impl Future<Output = Result<Self::ActiveOperationsReturn, Self::ServiceError>> + Send;

    /// Activates an operation.
    fn activate(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
        params: Self::ActivationParamsOption,
    ) -> impl Future<Output = Result<Self::ActivateReturn, Self::ServiceError>> + Send;

    /// Activates an operation and returns a stream for monitoring the operation.
    fn activate_stream(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
        params: Self::ActivationParamsOption,
    ) -> impl Future<Output = Result<Self::ActivateStreamReturn, Self::ServiceError>> + Send;

    /// Aborts the active operation.
    fn abort(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    /// Locks the operation.
    fn lock(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    /// Unlocks the operation.
    fn unlock(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    /// Activates the sensor associated with the operation.
    fn activate_sensor(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::ActivateSensorReturn, Self::ServiceError>> + Send;

    /// Deactivates the sensor associated with the operation.
    fn deactivate_sensor(
        controller: Self::ControllerParameter,
        id: Self::OperationIDParameter,
    ) -> impl Future<Output = Result<Self::DeactivateSensorReturn, Self::ServiceError>> + Send;
}

/// Client trait for interacting with the controller-service.
pub trait ControllerClient {
    /// The type of the client error.
    /// It must take into account the service error type and any additional client-specific errors.
    type ClientError;

    /// Retrieves information about the operation.
    fn operation(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<OperationInfo, Self::ClientError>> + Send;

    /// Retrieves information about all operations.
    fn operations(
        &self,
        controller_id: &str,
    ) -> impl Future<Output = Result<Vec<OperationInfo>, Self::ClientError>> + Send;

    /// Retrieves the IDs of all active operations.
    fn active_operations(
        &self,
        controller_id: &str,
    ) -> impl Future<Output = Result<Vec<String>, Self::ClientError>> + Send;

    /// Sends activation request to activate the operation.
    fn activate(
        &self,
        controller_id: &str,
        operation_id: &str,
        params: Option<&AnySerializable>,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    /// Sends activation request to activate an operation and returns a stream of operation states.
    fn activate_stream(
        &self,
        controller_id: &str,
        operation_id: &str,
        params: Option<&AnySerializable>,
    ) -> impl Future<Output = Result<impl Stream<Item = OpState>, Self::ClientError>> + Send;

    /// Sends abort request to abort the active operation.
    fn abort(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    /// Sends lock request to lock the operation.
    fn lock(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    /// Sends unlock request to unlock the operation.
    fn unlock(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    /// Sends activation request to activate a sensor associated with an operation.
    fn activate_sensor(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    /// Sends deactivation request to deactivate a sensor associated with an operation.
    fn deactivate_sensor(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;
}
