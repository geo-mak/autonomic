pub trait ControllerService {
    type ServiceProvider;
    type ServiceError;
    type ControllerID;
    type ControllerReturn;
    type ControllersReturn;
    type PerformingReturn;
    type PerformReturn;
    type AbortReturn;
    type LockReturn;
    type UnlockReturn;
    type StartSensorReturn;
    type StopSensorReturn;

    fn ctrl(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::ControllerReturn, Self::ServiceError>> + Send;

    fn list(
        provider: Self::ServiceProvider,
    ) -> impl Future<Output = Result<Self::ControllersReturn, Self::ServiceError>> + Send;

    fn list_performing(
        provider: Self::ServiceProvider,
    ) -> impl Future<Output = Result<Self::PerformingReturn, Self::ServiceError>> + Send;

    fn perform(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::PerformReturn, Self::ServiceError>> + Send;

    fn abort(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    fn lock(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    fn unlock(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::AbortReturn, Self::ServiceError>> + Send;

    fn start_sensor(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::StartSensorReturn, Self::ServiceError>> + Send;

    fn stop_sensor(
        provider: Self::ServiceProvider,
        id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::StopSensorReturn, Self::ServiceError>> + Send;
}

pub trait ControllerClient {
    type ClientError;
    type ControllerID;
    type ControllerInfo;
    type ControllersInfo;
    type PerformingControllers;
    type PerformReturn;

    fn ctrl(
        &self,
        controller_id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::ControllerInfo, Self::ClientError>> + Send;

    fn list(&self)
    -> impl Future<Output = Result<Self::ControllersInfo, Self::ClientError>> + Send;

    fn list_performing(
        &self,
    ) -> impl Future<Output = Result<Self::PerformingControllers, Self::ClientError>> + Send;

    fn perform(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<Self::PerformReturn, Self::ClientError>> + Send;

    fn abort(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    fn lock(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    fn unlock(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    fn start_sensor(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;

    fn stop_sensor(
        &self,
        operation_id: Self::ControllerID,
    ) -> impl Future<Output = Result<(), Self::ClientError>> + Send;
}
