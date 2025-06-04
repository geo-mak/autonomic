use reqwest::{Client, Error, Response};
use std::marker::PhantomData;

use futures_util::StreamExt;
use tokio_stream::Stream;

use autonomic_events::trace_trace;

use autonomic_api::operation::OperationClient;

use autonomic_operation::errors::ControllerError;
use autonomic_operation::operation::{OpInfo, OpState};
use autonomic_operation::serde::AnySerializable;

/// Error type for the OpenAPIClient.
#[derive(Debug)]
pub enum OpenAPIClientError {
    ResponseError(ControllerError),
    RequestError(reqwest::Error),
}

/// Client for interacting with the OpenAPI service.
/// It implements methods corresponding to the service endpoints and returns typed results.
#[derive(Clone)]
pub struct OpenAPIClient<'a> {
    client: Client, // Cloning just clones the inner Arc
    host: &'a str,
}

/// Helper type to transform the raw stream to typed stream.
pub struct StreamMapper<T>
where
    T: for<'de> serde::de::Deserialize<'de>,
{
    response: Response,
    _t: PhantomData<T>,
}

impl<T> StreamMapper<T>
where
    T: for<'de> serde::de::Deserialize<'de>,
{
    /// Transforms the stream from a stream of bytes to stream of type `T`.
    pub fn map(self) -> impl Stream<Item = T> {
        self.response.bytes_stream().map(|result| {
            let event_bytes = result.expect("Failed to read event's bytes");
            let event_str = std::str::from_utf8(&event_bytes)
                .expect("Failed to convert event's bytes to string");
            let state_str = event_str.trim_start_matches("data: ");
            serde_json::from_str::<T>(&state_str).expect("Failed to deserialize event as `T`")
        })
    }
}

impl<'a> OpenAPIClient<'a> {
    const CLIENT_LABEL: &'static str = "OpenAPIClient";

    /// Creates a new OpenAPIClient with the specified base URL.
    pub fn new(client: Client, host: &'a str) -> Self {
        OpenAPIClient { client, host }
    }

    #[inline]
    async fn parse_response_as<T>(response: Response) -> T
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        response
            .json::<T>()
            .await
            .expect("Failed to deserialize response")
    }

    async fn match_request_result<C, F, T>(
        result: Result<Response, Error>,
        on_success: C,
    ) -> Result<T, OpenAPIClientError>
    where
        C: FnOnce(Response) -> F,
        F: std::future::Future<Output = T>,
        T: Sized,
    {
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(on_success(response).await)
                } else {
                    let err = Self::parse_response_as::<ControllerError>(response).await;
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
    }

    async fn match_and_parse_as<T>(result: Result<Response, Error>) -> Result<T, OpenAPIClientError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        Self::match_request_result(result, async |response: Response| {
            Self::parse_response_as::<T>(response).await
        })
        .await
    }

    #[inline]
    async fn match_ok(result: Result<Response, Error>) -> Result<(), OpenAPIClientError> {
        Self::match_request_result(result, async |_| ()).await
    }
}

impl<'a> OperationClient for OpenAPIClient<'a> {
    type ClientError = OpenAPIClientError;
    type ControllerID = &'a str;
    type OperationID = &'a str;
    type OperationInfo = OpInfo;
    type OperationsInfo = Vec<OpInfo>;
    type ActiveOperations = Vec<String>;
    type ActivationParams = Option<&'a AnySerializable>;

    // Stream is always returned as opaque type (impl Stream), which is not allowed as associated type
    // or as type alias currently.
    type StateStream = StreamMapper<OpState>;

    /// Retrieves a specific operation by its ID.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(OpInfo)`: If the operation is found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn op(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<Self::OperationInfo, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to get operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/op/{}", self.host, controller_id, operation_id);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::OperationInfo>(result).await
    }

    /// Retrieves all operations for a specific controller.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    ///
    /// # Returns
    /// - `Ok(Vec<OpInfo>)`: If the operations are found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn list(
        &self,
        controller_id: Self::ControllerID,
    ) -> Result<Self::OperationsInfo, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to get all operations for controller={}",
                controller_id
            )
        );
        let url = format!("{}/{}/list", self.host, controller_id);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::OperationsInfo>(result).await
    }

    /// Retrieves the currently active operations for a specific controller.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    ///
    /// # Returns
    /// - `Ok(Vec<String>)`: If active operations are found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn list_active(
        &self,
        controller_id: Self::ControllerID,
    ) -> Result<Self::ActiveOperations, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to get active operations for controller={}",
                controller_id
            )
        );
        let url = format!("{}/{}/list_active", self.host, controller_id);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::ActiveOperations>(result).await
    }

    /// Activates a specific operation without streaming its state updates.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    /// - `params`: An optional reference to the operation parameters.
    ///
    /// # Returns
    /// - `Ok(())`: If the operation is successfully activated.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn activate(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
        params: Self::ActivationParams,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to activate operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/activate/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).json(&params).send().await;
        Self::match_ok(result).await
    }

    /// Activates a specific operation and returns a stream of its state updates.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    /// - `params`: An optional reference to the operation parameters.
    ///
    /// # Returns
    /// - `Ok(impl Stream<Item = OperationState>)`: If the operation is successfully started.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn activate_stream(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
        params: Self::ActivationParams,
    ) -> Result<Self::StateStream, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to activate operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!(
            "{}/{}/activate_stream/{}",
            self.host, controller_id, operation_id
        );
        let result = self.client.post(&url).json(&params).send().await;
        Self::match_request_result(result, async |response: Response| StreamMapper {
            response,
            _t: PhantomData,
        })
        .await
    }

    /// Aborts a specific operation.
    /// If the operation is not active, it does nothing.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn abort(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to abort operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/abort/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Locks a specific operation.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn lock(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to lock operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/lock/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Unlocks a specific operation.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn unlock(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to unlock operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/unlock/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Activates the sensor of the operation if it has been set, and it is currently inactive.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(())`: If the activation was successful.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn activate_sensor(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to activate sensor for operation={} for controller={}",
                operation_id, controller_id
            )
        );

        let url = format!(
            "{}/{}/activate_sensor/{}",
            self.host, controller_id, operation_id
        );

        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Deactivates the sensor of the operation if it has been set, and it is currently active.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(())`: If the deactivation was successful.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn deactivate_sensor(
        &self,
        controller_id: Self::ControllerID,
        operation_id: Self::OperationID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to deactivate sensor for operation={} for controller={}",
                operation_id, controller_id
            )
        );

        let url = format!(
            "{}/{}/deactivate_sensor/{}",
            self.host, controller_id, operation_id
        );

        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerOpts};
    use reqwest::ClientBuilder;

    use autonomic_operation::errors::{ActivationError, ControllerError};
    use autonomic_operation::operation::OpState;
    use autonomic_operation::testkit::params::TestRetry;

    // --------------------------- Ok Tests ---------------------------
    #[tokio::test]
    async fn test_get_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let operation_info = OpInfo::from_str("Test Operation", "123", false, false, false);

        // Response body as JSON
        let body = serde_json::to_string(&operation_info).unwrap();

        let _m = server
            .mock(
                "GET",
                format!("/{}/op/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let result = api_client.op(&controller_id, &operation_id).await.unwrap();

        assert_eq!(result, operation_info);
    }

    #[tokio::test]
    async fn test_get_ops() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operations_info = vec![
            OpInfo::from_str("Operation1", "1", false, false, false),
            OpInfo::from_str("Operation2", "2", false, false, false),
        ];

        // Response body as JSON
        let body = serde_json::to_string(&operations_info).unwrap();

        let _m = server
            .mock("GET", format!("/{}/list", controller_id).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list(controller_id).await.unwrap();

        assert_eq!(result, operations_info);
    }

    #[tokio::test]
    async fn test_get_active_ops() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";

        // Response body as JSON array
        let body = r#"["ActiveOperation1", "ActiveOperation2"]"#;

        let _m = server
            .mock("GET", format!("/{}/list_active", controller_id).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list_active(controller_id).await.unwrap();

        assert_eq!(
            result,
            vec![
                "ActiveOperation1".to_string(),
                "ActiveOperation2".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_activate_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let body = serde_json::to_string(&()).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client
            .activate(controller_id, operation_id, None)
            .await
            .unwrap();

        assert_eq!(result, ());
    }

    #[tokio::test]
    async fn test_activate_op_params() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let body = serde_json::to_string(&()).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let params = AnySerializable::new_register(TestRetry::new(3, 1000));

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client
            .activate(controller_id, operation_id, Some(&params))
            .await
            .unwrap();

        assert_eq!(result, ());
    }

    #[tokio::test]
    async fn test_activate_op_stream() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let operation_state = OpState::Active;

        // Response body as JSON
        let body = serde_json::to_string(&operation_state).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate_stream/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let mut stream_map = api_client
            .activate_stream(controller_id, operation_id, None)
            .await
            .unwrap()
            .map();

        while let Some(result) = stream_map.next().await {
            assert_eq!(result, operation_state);
        }
    }

    #[tokio::test]
    async fn test_activate_op_stream_params() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let operation_state = OpState::Active;

        // Response body as JSON
        let body = serde_json::to_string(&operation_state).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate_stream/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            // TODO: Not accurate because server sends SSE as MessageEvent with payload behind "data"
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let params = AnySerializable::new_register(TestRetry::new(3, 1000));

        let mut stream_map = api_client
            .activate_stream(controller_id, operation_id, Some(&params))
            .await
            .unwrap()
            .map();

        while let Some(result) = stream_map.next().await {
            assert_eq!(result, operation_state);
        }
    }

    #[tokio::test]
    async fn test_abort_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/abort/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.abort(controller_id, operation_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_lock_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/lock/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.lock(controller_id, operation_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unlock_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/unlock/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.unlock(controller_id, operation_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_activate_sensor() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate_sensor/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client
            .activate_sensor(controller_id, operation_id)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deactivate_sensor() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/deactivate_sensor/{}", controller_id, operation_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();

        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client
            .deactivate_sensor(controller_id, operation_id)
            .await;
        assert!(result.is_ok());
    }

    // --------------------------- Err Tests ---------------------------
    #[tokio::test]
    async fn test_get_op_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let op_id = "123";
        let error_response = ControllerError::OpNotFound;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock("GET", format!("/{}/op/{}", controller_id, op_id).as_str())
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.op(controller_id, op_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::OpNotFound
            ))
        ));
    }

    #[tokio::test]
    async fn test_get_ops_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let error_response = ControllerError::OpNotFound;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock("GET", format!("/{}/list", controller_id).as_str())
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list(controller_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::OpNotFound
            ))
        ));
    }

    #[tokio::test]
    async fn test_activate_op_stream_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let op_id = "123";
        let error_response = ControllerError::ActivationErr(ActivationError::Active);

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/activate_stream/{}", controller_id, op_id).as_str(),
            )
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let result = api_client.activate_stream(controller_id, op_id, None).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::ActivationErr(ActivationError::Active)
            ))
        ));
    }

    #[tokio::test]
    async fn test_abort_op_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let op_id = "123";
        let error_response = ControllerError::NoActiveOps;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/abort/{}", controller_id, op_id).as_str(),
            )
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.abort(controller_id, op_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::NoActiveOps
            ))
        ));
    }

    #[tokio::test]
    async fn test_get_active_ops_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let error_response = ControllerError::NoActiveOps;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock("GET", format!("/{}/list_active", controller_id).as_str())
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list_active(controller_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::NoActiveOps
            ))
        ));
    }
}
