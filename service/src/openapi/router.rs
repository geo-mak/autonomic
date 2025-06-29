use std::convert::Infallible;

use futures_util::StreamExt;
use futures_util::stream::Map;

use tokio_stream::wrappers::WatchStream;

use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::{
    Json, Router,
    extract::{Extension, Path},
    routing::{get, post},
};

use autonomic_events::trace_trace;

use autonomic_api::controller::ControllerService;

use autonomic_controllers::controller::{ControllerInfo, OpState};
use autonomic_controllers::errors::ControllerError;
use autonomic_controllers::manager::ControllerManager;

/// Creates a router instance with endpoints related to the controller service.
///
/// # Endpoints:`
/// - `GET` `/{manager_id}/ctrl/{id}`: Retrieves a specific controller by its ID.
/// - `GET` `/{manager_id}/list`: Retrieves all controllers.
/// - `GET` `/{manager_id}/list_performing`: Retrieves the controllers with currently active operations.
/// - `POST` `/{manager_id}/perform/{id}`: Activates the operation of a specific controller and sends its states as `SSE` stream.
/// - `POST` `/{manager_id}/abort/{id}`: Aborts an operation of a controller.
/// - `POST` `/{manager_id}/lock/{id}`: Locks a specific controller.
/// - `POST` `/{manager_id}/unlock/{id}`: Unlocks a specific controller.
/// - `POST` `/{manager_id}/start_sensor/{id}`: Activates the sense of a specific controller.
/// - `POST` `/{manager_id}/stop_sensor/{id}`: Deactivates the sense of a specific controller.
///
/// # Fallback Response
/// - Status: `501`.
/// - Body: `ControllerError::NotImplemented`.
pub fn controller_router(manager: &'static ControllerManager) -> Router {
    Router::new()
        .route("/ctrl_mgr/ctrl/:id", get(OpenAPIEndpoints::ctrl))
        .route("/ctrl_mgr/list", get(OpenAPIEndpoints::list))
        .route(
            "/ctrl_mgr/list_performing",
            get(OpenAPIEndpoints::list_performing),
        )
        .route("/ctrl_mgr/perform/:id", post(OpenAPIEndpoints::perform))
        .route("/ctrl_mgr/abort/:id", post(OpenAPIEndpoints::abort))
        .route("/ctrl_mgr/lock/:id", post(OpenAPIEndpoints::lock))
        .route("/ctrl_mgr/unlock/:id", post(OpenAPIEndpoints::unlock))
        .route(
            "/ctrl_mgr/start_sensor/:id",
            post(OpenAPIEndpoints::start_sensor),
        )
        .route(
            "/ctrl_mgr/stop_sensor/:id",
            post(OpenAPIEndpoints::stop_sensor),
        )
        .layer(Extension(manager))
        .fallback(not_implemented)
}

struct ServiceError(ControllerError);

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status_code = match self.0 {
            ControllerError::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            ControllerError::NoResults => StatusCode::NO_CONTENT,
            ControllerError::NotFound => StatusCode::NOT_FOUND,
            ControllerError::Active => StatusCode::CONFLICT,
            ControllerError::Locked => StatusCode::LOCKED,
        };

        let body = Json(self.0);
        (status_code, body).into_response()
    }
}

/// Fallback handler
async fn not_implemented() -> ServiceError {
    ServiceError(ControllerError::NotImplemented)
}

type StreamMapper = Map<WatchStream<OpState>, fn(OpState) -> Result<Event, Infallible>>;
type EventsStream = Sse<StreamMapper>;

const SERVICE_LABEL: &str = "OpenAPIService";

struct OpenAPIEndpoints;

impl ControllerService for OpenAPIEndpoints {
    type ServiceManager = Extension<&'static ControllerManager>;
    type ServiceError = ServiceError;
    type ControllerID = Path<String>;
    type ControllerReturn = Json<ControllerInfo>;
    type ControllersReturn = Json<Vec<ControllerInfo>>;
    type PerformingReturn = Json<Vec<&'static str>>;
    type PerformReturn = EventsStream;
    type AbortReturn = StatusCode;
    type LockReturn = StatusCode;
    type UnlockReturn = StatusCode;
    type StartSensorReturn = StatusCode;
    type StopSensorReturn = StatusCode;

    /// Retrieves a specific operation by its ID.
    ///
    /// # Returns
    /// - Status code `200` and `Json<ControllerInfo>` as body: If the request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    async fn ctrl(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::ControllerReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!("Received HTTP request to get controller {}", id)
        );
        match manager.controller(id.as_str()) {
            Ok(op_info) => Ok(Json(op_info)),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Retrieves all operations.
    ///
    /// # Returns
    /// - Status code `200` and `Json<Vec<ControllerInfo>>` as body: If the request was successful.
    /// - Status code `204` and `Json<ControllerError::NoResults>` as body: If there are no controllers.
    async fn list(
        Extension(manager): Self::ServiceManager,
    ) -> Result<Self::ControllersReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = "Received HTTP request to list controllers"
        );
        match manager.list() {
            Ok(op_infos) => Ok(Json(op_infos)),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Retrieves the currently active operations.
    ///
    /// # Returns
    /// - Status code `200` and `Json<Vec<String>>` as body: If the request was successful.
    /// - Status code `404` and `Json<ControllerError::NoActiveOps>` as body: If the controller is empty.
    async fn list_performing(
        Extension(manager): Self::ServiceManager,
    ) -> Result<Self::PerformingReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = "Received HTTP request to list controllers with performing operations"
        );
        match manager.list_performing() {
            Ok(ops) => Ok(Json(ops)),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Activates a specific operation and sends its states as `Server-Sent Events (SSE)` stream.
    ///
    /// # Returns
    /// - Status code `200` and `Sse` stream as body: If request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the operation is not found.
    /// - Status code `409` and `Json<ControllerError::Active>` as body: If the operation is already active.
    /// - Status code `423` and `Json<ControllerError::Locked>` as body: If the operation is locked.
    async fn perform(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::PerformReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!(
                "Received HTTP request start the operation of the controller {} with streaming",
                id
            )
        );
        match manager.perform(id.as_str()) {
            Ok(watch_stream) => {
                // Transform the stream to emit SSE events
                // Full annotation is required here to avoid type inference issues
                let stream_map: StreamMapper = watch_stream.map(|state| {
                    let event = Event::default()
                        .json_data(state)
                        .expect("Unable to serialize state to create an event");
                    Ok(event)
                });
                trace_trace!(
                    source = SERVICE_LABEL,
                    message = format!(
                        "Starting SSE stream for the operation of the controller {}",
                        id
                    )
                );
                Ok(Sse::new(stream_map))
            }
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Aborts the operation of a controller.
    ///
    /// > **Note**: It does not guarantee that operation will be aborted.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    async fn abort(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::AbortReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!(
                "Received HTTP request to abort the operation of controller {}",
                id
            )
        );
        match manager.abort(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Locks a specific controller.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    async fn lock(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::LockReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!("Received HTTP request to lock the controller {}", id)
        );
        match manager.lock(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(ServiceError(err)),
        }
    }
    /// Unlocks a specific controller.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    async fn unlock(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::UnlockReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!("Received HTTP request to unlock the controller {}", id)
        );
        match manager.unlock(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Activates the sensor associated with a controller.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    /// - Status code `409` and `Json<ControllerError::Active>` as body: If the sensor is already active.
    /// - Status code `423` and `Json<ControllerError::Locked>` as body: If the controller is locked.
    async fn start_sensor(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::StartSensorReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!(
                "Received HTTP request to activate the sense of the controller {}",
                id
            )
        );
        match manager.start_sensor(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(ServiceError(err)),
        }
    }

    /// Activates the sensor of the controller.
    ///
    /// # Returns
    /// - Status code `200`: if request was successful.
    /// - Status code `404` and `Json<ControllerError::NotFound>` as body: If the controller is not found.
    async fn stop_sensor(
        Extension(manager): Self::ServiceManager,
        Path(id): Self::ControllerID,
    ) -> Result<Self::StopSensorReturn, Self::ServiceError> {
        trace_trace!(
            source = SERVICE_LABEL,
            message = format!(
                "Received HTTP request to deactivate the sense of the controller {}",
                id
            )
        );
        match manager.stop_sensor(id.as_str()) {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(ServiceError(err)),
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

    use autonomic_controllers::controller::OpState;
    use autonomic_controllers::manager::ControllerManager;

    use autonomic_controllers::testkit::controller::TestController;
    use autonomic_events::testkit::global::init_tracing;

    #[tokio::test]
    async fn test_fallback() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let router = controller_router(manager);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/ctrl_mgr/not_implemented/")
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
            .expect("Failed to deserialize ControllerInfo");

        assert_eq!(err, ControllerError::NotImplemented)
    }

    #[tokio::test]
    async fn test_get_ctrl() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, None, 0);

        manager.submit(controller);

        let router = controller_router(manager);

        let request = Request::builder()
            .uri(format!("/ctrl_mgr/ctrl/{}", controller_id))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // body must not be empty
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");

        let received_info: ControllerInfo =
            serde_json::from_slice(&body).expect("Failed to deserialize ControllerInfo");

        assert_eq!(received_info.id(), controller_id);
        assert_eq!(received_info.description(), controller_id);
        assert_eq!(received_info.performing(), false);
    }

    #[tokio::test]
    async fn test_get_ctrls() {
        let manager = ControllerManager::new().into_static();

        let controller_1_id = "test_controller_1";
        let controller_2_id = "test_controller_2";
        let controller_3_id = "test_controller_3";

        let controller_1 = TestController::ok(controller_1_id, None, 0);
        let controller_2 = TestController::ok(controller_2_id, None, 0);
        let controller_3 = TestController::ok(controller_3_id, None, 0);

        manager.submit(controller_1);
        manager.submit(controller_2);
        manager.submit(controller_3);

        let router = controller_router(manager);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/ctrl_mgr/list")
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

        let mut controllers_info: Vec<ControllerInfo> =
            serde_json::from_slice(&body).expect("Failed to deserialize slice as vector");

        let mut expected = vec![
            ControllerInfo::from_str(controller_1_id, controller_1_id, 0, false),
            ControllerInfo::from_str(controller_2_id, controller_2_id, 0, false),
            ControllerInfo::from_str(controller_3_id, controller_3_id, 0, false),
        ];

        controllers_info.sort_by(|a, b| a.id().cmp(b.id()));
        expected.sort_by(|a, b| a.id().cmp(b.id()));

        assert_eq!(controllers_info, expected);
    }

    #[tokio::test]
    async fn test_get_perf_ctrl() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let active_controller_id = "active_ctrl";
        let active_ctrl =
            TestController::ok(active_controller_id, Some(Duration::from_millis(10)), 0);

        let inactive_controller_id = "inactive_ctrl";
        let inactive_ctrl = TestController::ok(inactive_controller_id, None, 0);

        manager.submit(active_ctrl);

        manager.submit(inactive_ctrl);

        let router = controller_router(manager);

        // Test when no operations are active
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ctrl_mgr/list_performing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let error: ControllerError =
            serde_json::from_slice(&body).expect("Failed to deserialize controllerError");

        assert_eq!(error, ControllerError::NoResults);

        // Activate the operation
        manager.perform(&active_controller_id).unwrap();

        // Operation execution is delayed for 10 millis, so it should be still active when we check
        tokio::time::sleep(Duration::from_millis(1)).await;
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/ctrl_mgr/list_performing")
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
        assert_eq!(result[0], active_controller_id.to_string());
    }

    #[tokio::test]
    async fn test_perform() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, None, 0);

        manager.submit(controller);

        let router = controller_router(manager);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/perform/{}", controller_id))
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
    async fn test_abort_ctrl() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";

        // Duration is high to make sure abort is working as espected.
        let controller = TestController::ok(controller_id, Some(Duration::from_secs(10)), 0);

        manager.submit(controller);

        // Activate the operation
        manager.perform(&controller_id).unwrap();

        // Wait some time for activation to take place
        tokio::time::sleep(Duration::from_millis(2)).await;

        // Operation must be active by now
        assert!(manager.is_performing(&controller_id).unwrap());

        let router = controller_router(manager);

        // Abort request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/abort/{}", controller_id))
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
        assert!(!manager.is_performing(&controller_id).unwrap());
    }

    #[tokio::test]
    async fn test_lock_ctrl() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, None, 0);

        manager.submit(controller);

        let router = controller_router(manager);

        // Lock request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/lock/{}", controller_id))
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Operation must have been locked
        assert!(manager.is_locked(&controller_id).unwrap());
    }

    #[tokio::test]
    async fn test_unlock_ctrl() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, None, 0);

        manager.submit(controller);

        // Lock the operation
        manager.lock(controller_id).unwrap();

        let router = controller_router(manager);

        // Unlock request
        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/unlock/{}", controller_id))
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Response should be ok
        assert_eq!(response.status(), StatusCode::OK);

        // Controller must have been unlocked
        assert!(!manager.is_locked(&controller_id).unwrap());
    }

    #[tokio::test]
    async fn test_start_sensor() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, Some(Duration::from_millis(10)), 0);

        manager.submit(controller);

        let router = controller_router(manager);

        let response = &router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/start_sensor/{}", controller_id))
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
        assert!(manager.sensing(&controller_id).unwrap());
    }

    #[tokio::test]
    async fn test_stop_sensor() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller_id = "test_controller";
        let controller = TestController::ok(controller_id, Some(Duration::from_millis(10)), 1);

        manager.submit(controller);

        // Active sensor in controller
        manager.start_sensor(&controller_id).unwrap();

        // Wait some time for activation to take place
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Sensor must be active by now
        assert!(manager.sensing(&controller_id).unwrap());

        let router = controller_router(manager);

        let response = &router
            .oneshot(
                Request::builder()
                    .uri(format!("/ctrl_mgr/stop_sensor/{}", controller_id))
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
        assert!(!manager.sensing(&controller_id).unwrap());
    }
}
