use futures_util::StreamExt;
use futures_util::stream::Map;
use std::convert::Infallible;
use std::sync::Arc;

use tokio_stream::wrappers::WatchStream;

use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::{
    Json, Router,
    extract::{Extension, Path},
    routing::{get, post},
};

use autonomic_core::controller::OperationController;
use autonomic_core::errors::{ActivationError, ControllerError};
use autonomic_core::operation::{OpState, OperationInfo};
use autonomic_core::serde::AnySerializable;
use autonomic_core::trace_trace;
use autonomic_core::traits::Identity;

use autonomic_api::controller::ControllerService;

/// Creates a router instance with endpoints related to the controller service.
///
/// # Parameters
/// - `controller`: The operation controller instance.
///
/// # Endpoints:`
/// - `GET` `/{controller_id}/op/{id}`: Retrieves a specific operation by its ID.
/// - `GET` `/{controller_id}/ops`: Retrieves all operations.
/// - `GET` `/{controller_id}/active_ops`: Retrieves the currently active operations.
/// - `POST` `/{controller_id}/activate/{id}`: Activates a specific operation without streaming its state updates.
/// - `POST` `/{controller_id}/activate_stream/{id}`: Activates a specific operation and sends its states as `SSE` stream.
/// - `POST` `/{controller_id}/abort/{id}`: Aborts a specific operation.
/// - `POST` `/{controller_id}/lock/{id}`: Locks a specific operation.
/// - `POST` `/{controller_id}/unlock/{id}`: Unlocks a specific operation.
/// - `POST` `/{controller_id}/activate_sensor/{id}`: Activates the sensor for a specific operation.
/// - `POST` `/{controller_id}/deactivate_sensor/{id}`: Deactivates the sensor for a specific operation.
///
/// > **Important Note**:
/// > The URL path is based on the `controller ID`, which must be unique for each controller.
/// > If the controller ID is not unique, the endpoints will clash.
///
/// # Fallback Response
/// - Status: `501`.
/// - Body: `ControllerError::NotImplemented`.
///
/// # Returns
/// A `Router` contains endpoints related to the controller service.
pub fn controller_router(controller: Arc<OperationController<'static>>) -> Router {
    let base_path = { format!("/{}", controller.id()) };
    Router::new()
        .route(
            &format!("{}/op/:id", base_path),
            get(OpenAPIEndpoints::operation),
        )
        .route(
            &format!("{}/ops", base_path),
            get(OpenAPIEndpoints::operations),
        )
        .route(
            &format!("{}/active_ops", base_path),
            get(OpenAPIEndpoints::active_operations),
        )
        .route(
            &format!("{}/activate/:id", base_path),
            post(OpenAPIEndpoints::activate),
        )
        .route(
            &format!("{}/activate_stream/:id", base_path),
            post(OpenAPIEndpoints::activate_stream),
        )
        .route(
            &format!("{}/abort/:id", base_path),
            post(OpenAPIEndpoints::abort),
        )
        .route(
            &format!("{}/lock/:id", base_path),
            post(OpenAPIEndpoints::lock),
        )
        .route(
            &format!("{}/unlock/:id", base_path),
            post(OpenAPIEndpoints::unlock),
        )
        .route(
            &format!("{}/activate_sensor/:id", base_path),
            post(OpenAPIEndpoints::activate_sensor),
        )
        .route(
            &format!("{}/deactivate_sensor/:id", base_path),
            post(OpenAPIEndpoints::deactivate_sensor),
        )
        .layer(Extension(controller))
        .fallback(not_implemented)
}

// But why with wrapper? Because of the 'exceptionally annoying' Rust's orphan rule.
struct ServiceError(ControllerError);

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status_code = match self.0 {
            ControllerError::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            ControllerError::Empty => StatusCode::NO_CONTENT,
            ControllerError::OpNotFound => StatusCode::NOT_FOUND,
            ControllerError::NoActiveOps => StatusCode::NOT_FOUND,
            ControllerError::ActivationErr(ActivationError::Active) => StatusCode::CONFLICT,
            ControllerError::ActivationErr(ActivationError::NotSet) => StatusCode::NOT_FOUND,
            ControllerError::ActivationErr(ActivationError::Locked) => StatusCode::LOCKED,
        };

        let body = Json(self.0);
        (status_code, body).into_response()
    }
}

#[inline(always)]
const fn into_service_error(err: ControllerError) -> ServiceError {
    ServiceError(err)
}

/// Fallback handler
async fn not_implemented() -> ServiceError {
    ServiceError(ControllerError::NotImplemented)
}

type StateStream = Sse<Map<WatchStream<OpState>, fn(OpState) -> Result<Event, Infallible>>>;

struct OpenAPIEndpoints;

impl ControllerService for OpenAPIEndpoints {
    // **Note**: We use `Arc` because `Extension` clones the value on each request.
    // Making the controller static can be a better option.
    type ControllerParameter = Extension<Arc<OperationController<'static>>>;
    type ServiceError = ServiceError;
    type OperationIDParameter = Path<String>;
    type OperationReturn = Json<OperationInfo>;
    type OperationsReturn = Json<Vec<OperationInfo>>;
    type ActiveOperationsReturn = Json<Vec<&'static str>>;
    type ActivationParamsOption = Json<Option<AnySerializable>>;
    type ActivateReturn = StatusCode;
    type ActivateStreamReturn = StateStream;
    type AbortReturn = StatusCode;
    type LockReturn = StatusCode;
    type UnlockReturn = StatusCode;
    type ActivateSensorReturn = StatusCode;
    type DeactivateSensorReturn = StatusCode;

    /// Retrieves a specific operation by its ID.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to retrieve.
    ///
    /// # Returns
    /// - Status code `200` and `Json<OperationInfo>` as body: If the request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the teh controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    async fn operation(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::OperationReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to get operation={}", id)
        );
        // further data is done in the controller
        match controller.operation(id.as_str()) {
            Ok(op_info) => Ok(Json(op_info)),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Retrieves all operations.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    ///
    /// # Returns
    /// - Status code `200` and `Json<Vec<OperationInfo>>` as body: If the request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the teh controller is empty.
    async fn operations(
        Extension(controller): Self::ControllerParameter,
    ) -> Result<Self::OperationsReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = "Received HTTP request to get all operations"
        );
        // further data is done in the controller
        match controller.operations() {
            Ok(op_infos) => Ok(Json(op_infos)),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Retrieves the currently active operations.
    ///
    /// This function locks the `ActivationController`, retrieves the list of currently active operations
    /// using the `active_ops` method, and returns them as a JSON response.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    ///
    /// # Returns
    /// - Status code `200` and `Json<Vec<String>>` as body: If the request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::NoActiveOps>` as body: If the controller is empty.
    async fn active_operations(
        Extension(controller): Self::ControllerParameter,
    ) -> Result<Self::ActiveOperationsReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = "Received HTTP request to get all active operations"
        );
        match controller.active_operations() {
            Ok(ops) => Ok(Json(ops)),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Activates a specific operation by its ID.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to activate.
    /// - `params`: Optional parameters for the operation.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    /// - Status code `409` and `Json<ActivationError::Active>` as body: If the operation is already active.
    /// - Status code `423` and `Json<ActivationError::Locked>` as body: If the operation is locked.
    async fn activate(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
        Json(params): Self::ActivationParamsOption,
    ) -> Result<Self::ActivateReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to activate operation={}", id)
        );
        match controller.activate(id.as_str(), params) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Activates a specific operation and sends its states as `Server-Sent Events (SSE)` stream.
    ///
    /// This method is useful for activation requests where live state updates are needed.
    /// However, it is recommended to set up an event channel using events' publisher to track all events.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to activate.
    /// - `params`: Optional parameters for the operation.
    ///
    /// # Returns
    /// - Status code `200` and `Sse` stream as body: If request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    /// - Status code `409` and `Json<ActivationError::Active>` as body: If the operation is already active.
    /// - Status code `423` and `Json<ActivationError::Locked>` as body: If the operation is locked.
    async fn activate_stream(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
        Json(params): Self::ActivationParamsOption,
    ) -> Result<Self::ActivateStreamReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!(
                "Received HTTP request to activate operation={} with streaming",
                id
            )
        );
        match controller.activate_stream(id.as_str(), params) {
            Ok(watch_stream) => {
                // Transform the stream to emit SSE events
                // Full annotation is required here to avoid type inference issues
                let stream_map: Map<
                    WatchStream<OpState>,
                    fn(OpState) -> Result<Event, Infallible>,
                > = watch_stream.map(|state| {
                    // This shouldn't fail
                    Ok::<Event, Infallible>(
                        Event::default()
                            .json_data(state)
                            .expect("Unable to serialize state to create an event"),
                    )
                });
                trace_trace!(
                    source = "OpenAPIService",
                    message = format!("Starting SSE stream for operation={}", id)
                );
                Ok(Sse::new(stream_map))
            }
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Aborts a specific operation.
    /// Aborted state are expected to be received by the client with event stream,
    /// or when retrieving operation's data from data API.
    ///
    /// > **Note**: It does not guarantee that operation will be aborted.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to abort.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    async fn abort(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::AbortReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to abort operation={}", id)
        );
        match controller.abort(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Locks a specific operation.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to abort.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    async fn lock(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::LockReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to lock operation={}", id)
        );
        match controller.lock(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }
    /// Unlocks a specific operation.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to abort.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    async fn unlock(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::UnlockReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to unlock operation={}", id)
        );
        match controller.unlock(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Activates the sensor associated with an operation.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to abort.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    /// - Status code `404` and `Json<ActivationError::NotSet>` as body: If the sensor has not been set.
    /// - Status code `409` and `Json<ActivationError::Active>` as body: If the sensor is already active.
    /// - Status code `423` and `Json<ActivationError::Locked>` as body: If the operation is locked.
    async fn activate_sensor(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::ActivateSensorReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to activate sensor={}", id)
        );
        match controller.activate_sensor(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }

    /// Activates the sensor associated with an operation.
    ///
    /// # Parameters
    /// - `controller`: the controller instance.
    /// - `id`: The ID of the operation to abort.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `204` and `Json<ControllerError::Empty>` as body: If the controller is empty.
    /// - Status code `404` and `Json<ControllerError::OpNotFound>` as body: If the operation is not found.
    /// - Status code `404` and `Json<ActivationError::NotSet>` as body: If the sensor has not been set.
    async fn deactivate_sensor(
        Extension(controller): Self::ControllerParameter,
        Path(id): Self::OperationIDParameter,
    ) -> Result<Self::DeactivateSensorReturn, Self::ServiceError> {
        trace_trace!(
            source = "OpenAPIService",
            message = format!("Received HTTP request to deactivate sensor={}", id)
        );
        match controller.deactivate_sensor(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(into_service_error(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;
    use std::string::String;
    use std::time::Duration;

    use tower::util::ServiceExt;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};

    use autonomic_core::controller::OperationController;
    use autonomic_core::operation::OpState;
    use autonomic_core::traits::{Describe, Identity, IntoArc, IntoSensor};

    use autonomic_core::testkit::conditions::TestIntervalCondition;
    use autonomic_core::testkit::operation::TestOperation;
    use autonomic_core::testkit::params::TestRetry;
    use autonomic_core::testkit::tracing::init_tracing;

    #[tokio::test]
    async fn test_fallback() {
        init_tracing();
        let controller = OperationController::new("test_controller");

        let router = controller_router(controller.into_arc());

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/test_controller/not_implemented/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        let err = serde_json::from_slice::<ControllerError>(&body)
            .expect("Failed to deserialize OperationInfo");

        assert_eq!(err, ControllerError::NotImplemented)
    }

    #[tokio::test]
    async fn test_get_op() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();
        let operation_desc = operation.describe();

        controller.submit(operation, None);

        let router = controller_router(controller.into_arc());

        let request = Request::builder()
            .uri(format!("/test_controller/op/{}", operation_id))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        let received_info: OperationInfo =
            serde_json::from_slice(&body).expect("Failed to deserialize OperationInfo");

        assert_eq!(received_info.id(), operation_id);
        assert_eq!(received_info.description(), operation_desc);
        assert_eq!(received_info.active(), false);
    }

    #[tokio::test]
    async fn test_get_ops() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation_1 = TestOperation::ok("test_operation_1", "this is test operation 1", None);
        let operation_2 = TestOperation::ok("test_operation_2", "this is test operation 2", None);
        let operation_3 = TestOperation::ok("test_operation_3", "this is test operation 3", None);

        // Operation will move to controller, clone id and description
        let operation_1_id = operation_1.id();
        let operation_2_id = operation_2.id();
        let operation_3_id = operation_3.id();

        let operation_1_des = operation_1.describe();
        let operation_2_des = operation_2.describe();
        let operation_3_des = operation_3.describe();

        controller.submit(operation_1, None);
        controller.submit(operation_2, None);
        controller.submit(operation_3, None);

        let router = controller_router(controller.into_arc());

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/test_controller/ops")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        let mut operations_info: Vec<OperationInfo> =
            serde_json::from_slice(&body).expect("Failed to deserialize slice as vector");

        let mut expected_ops_info = vec![
            OperationInfo::from_str(operation_1_id, operation_1_des, false, false, false),
            OperationInfo::from_str(operation_2_id, operation_2_des, false, false, false),
            OperationInfo::from_str(operation_3_id, operation_3_des, false, false, false),
        ];

        // Sort both vectors
        operations_info.sort_by(|a, b| a.id().cmp(b.id()));
        expected_ops_info.sort_by(|a, b| a.id().cmp(b.id()));

        // Compare the sorted vectors
        assert_eq!(operations_info, expected_ops_info);
    }

    #[tokio::test]
    async fn test_get_active_ops() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let active_operation = TestOperation::ok(
            "active_operation",
            "this is test operation",
            Some(Duration::from_millis(10)),
        );

        let inactive_operation =
            TestOperation::ok("inactive_operation", "this is test operation", None);

        let active_op_id = active_operation.id();

        controller.submit(active_operation, None);
        controller.submit(inactive_operation, None);

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        // Test when no operations are active
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/test_controller/active_ops")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let error: ControllerError =
            serde_json::from_slice(&body).expect("Failed to deserialize controllerError");

        assert_eq!(error, ControllerError::NoActiveOps);

        // Activate the operation
        controller_ref.activate::<()>(&active_op_id, None).unwrap();

        // Operation execution is delayed for 10 millis, so it should be still active when we check
        tokio::time::sleep(Duration::from_millis(1)).await;
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/test_controller/active_ops")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        // Result must be Ok
        let result = serde_json::from_slice::<Vec<String>>(&body)
            .expect("Failed to deserialize Vec<String>");

        // The first element must be the active operation
        assert_eq!(result[0], active_op_id.to_string());
    }

    #[tokio::test]
    async fn test_activate_op() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        let router = controller_router(controller.into_arc());

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/activate/{}", operation_id))
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"null"#)) // No parameters
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_activate_op_params() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::err("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        let params = AnySerializable::new_register(TestRetry::new(3, 0));

        let params_json = serde_json::to_string(&Some(params)).unwrap();

        let router = controller_router(controller.into_arc());

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/activate/{}", operation_id))
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(params_json)) // No parameters
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_activate_op_stream() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        let router = controller_router(controller.into_arc());

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/activate_stream/{}", operation_id))
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"null"#)) // No parameters with `None`
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        // Convert bytes to string
        let data = std::str::from_utf8(&body).expect("Failed to convert bytes to string");

        // Get the last event
        let last_event = data
            .lines()
            .filter(|line| !line.is_empty())
            .last()
            .expect("No events found");

        //  "data: " prefix from event must be removed to deserialize the JSON string
        let json_str = last_event.trim_start_matches("data: ");

        let received_state: OpState =
            serde_json::from_str(json_str).expect("Failed to deserialize state");

        assert_eq!(
            received_state,
            OpState::Ok(Some(Cow::Borrowed("Result"))),
            "Unexpected event: {:?}",
            received_state
        );
    }

    #[tokio::test]
    async fn test_activate_op_stream_params() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        // Failing operation to test parameters that rerun operation on failure
        let operation = TestOperation::err("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        let params = AnySerializable::new_register(TestRetry::new(3, 0));

        let params_json = serde_json::to_string(&Some(params)).unwrap();

        let request = Request::builder()
            .uri(format!("/test_controller/activate_stream/{}", operation_id))
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(params_json))
            .unwrap();

        let router = controller_router(controller.into_arc());

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        // Convert bytes to string
        let data = std::str::from_utf8(&body).expect("Failed to convert bytes to string");

        // Get the last event
        let last_event = data
            .lines()
            .filter(|line| !line.is_empty())
            .last()
            .expect("No events found");

        // "data: " prefix from event must be removed to deserialize the JSON string
        let json_str = last_event.trim_start_matches("data: ");

        let state_message: OpState =
            serde_json::from_str(json_str).expect("Failed to deserialize OperationState");

        let expected_event_done = OpState::Failed(Some(Cow::Borrowed("Expected Error")));

        assert_eq!(
            state_message, expected_event_done,
            "Unexpected event: {}",
            state_message
        );
    }

    #[tokio::test]
    async fn test_abort_operation() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        // The sum of the waiting time must be less than 5 millis,
        // in order to guarantee realistic activation/abort cycle
        let operation = TestOperation::ok(
            "test_operation",
            "this is test operation",
            Some(Duration::from_millis(10)),
        );

        let operation_id = operation.id();

        controller.submit(operation, None);

        // Activate the operation
        controller.activate::<()>(&operation_id, None).unwrap();

        // Wait some time for activation to take place
        tokio::time::sleep(Duration::from_millis(2)).await;

        // Operation must be active by now
        assert!(controller.is_active(&operation_id).unwrap());

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        // Abort request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/abort/{}", operation_id))
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Wait some time for deactivation to take place
        tokio::time::sleep(Duration::from_millis(2)).await;

        // Operation must have been deactivated
        assert!(!controller_ref.is_active(&operation_id).unwrap());
    }

    #[tokio::test]
    async fn test_lock_operation() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        // Lock request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/lock/{}", operation_id))
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Operation must have been locked
        assert!(controller_ref.is_locked(&operation_id).unwrap());
    }

    #[tokio::test]
    async fn test_unlock_operation() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        controller.submit(operation, None);

        // Lock the operation
        controller.lock(operation_id).unwrap();

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        // Unlock request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/unlock/{}", operation_id))
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Operation must have been unlocked
        assert!(!controller_ref.is_locked(&operation_id).unwrap());
    }

    #[tokio::test]
    async fn test_activate_sensor() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        // one second interval for activation
        let sensor = TestIntervalCondition::new(1, None).into_sensor();

        controller.submit(operation, Some(sensor));

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        let response = &router
            .oneshot(
                Request::builder()
                    .uri(format!("/test_controller/activate_sensor/{}", operation_id))
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"null"#)) // No parameters
                    .unwrap(),
            )
            .await
            .unwrap();

        // Wait some time for activation to take place
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Sensor must be active by now
        assert!(controller_ref.is_sensor_active(&operation_id).unwrap());
    }

    #[tokio::test]
    async fn test_deactivate_sensor() {
        init_tracing();
        let mut controller = OperationController::new("test_controller");

        let operation = TestOperation::ok("test_operation", "this is test operation", None);

        let operation_id = operation.id();

        // one second interval for activation
        let sensor = TestIntervalCondition::new(1, None).into_sensor();

        controller.submit(operation, Some(sensor));

        // Active sensor in controller
        controller.activate_sensor(&operation_id).unwrap();

        // Wait some time for activation to take place
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Sensor must be active by now
        assert!(controller.is_sensor_active(&operation_id).unwrap());

        let controller_ref = controller.into_arc();
        let router = controller_router(controller_ref.clone());

        let response = &router
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/test_controller/deactivate_sensor/{}",
                        operation_id
                    ))
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"null"#)) // No parameters
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Wait some time for deactivation to take place
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Sensor must have been deactivated
        assert!(!controller_ref.is_sensor_active(&operation_id).unwrap());
    }
}
