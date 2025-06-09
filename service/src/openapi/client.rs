use std::marker::PhantomData;

use reqwest::{Client, Error, Response};

use futures_util::StreamExt;
use tokio_stream::Stream;

use autonomic_events::trace_trace;

use autonomic_api::controller::ControllerClient;

use autonomic_controllers::controller::{ControllerInfo, OpState};
use autonomic_controllers::errors::ControllerError;

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
            serde_json::from_str::<T>(state_str).expect("Failed to deserialize event as `T`")
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

impl<'a> ControllerClient for OpenAPIClient<'a> {
    type ClientError = OpenAPIClientError;
    type ControllerID = &'a str;
    type ControllerInfo = ControllerInfo;
    type ControllersInfo = Vec<ControllerInfo>;
    type PerformingControllers = Vec<String>;

    // Stream is always returned as opaque type (impl Stream), which is not allowed as associated type
    // or as type alias currently.
    type PerformReturn = StreamMapper<OpState>;

    /// Retrieves a specific controller by its ID.
    ///
    /// # Returns
    /// - `Ok(ControllerInfo)`: If the controller is found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn ctrl(
        &self,
        controller_id: Self::ControllerID,
    ) -> Result<Self::ControllerInfo, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!("Sending HTTP request to get controller {}", controller_id)
        );
        let url = format!("{}/ctrl_mgr/ctrl/{}", self.host, controller_id);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::ControllerInfo>(result).await
    }

    /// Retrieves all controllers.
    ///
    /// # Returns
    /// - `Ok(Vec<ControllerInfo>)`: If controllers are available.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn list(&self) -> Result<Self::ControllersInfo, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!("Sending HTTP request to get all controllers")
        );
        let url = format!("{}/ctrl_mgr/list", self.host);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::ControllersInfo>(result).await
    }

    /// Retrieves the controllers with currently active operations.
    ///
    /// # Returns
    /// - `Ok(Vec<String>)`: If there were matches.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn list_performing(&self) -> Result<Self::PerformingControllers, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!("Sending HTTP request to get controllers with active operations")
        );
        let url = format!("{}/ctrl_mgr/list_performing", self.host);
        let result = self.client.get(&url).send().await;
        Self::match_and_parse_as::<Self::PerformingControllers>(result).await
    }

    /// Activates the operation of a controller. and returns a stream of its state updates.
    ///
    /// # Returns
    /// - `Ok(impl Stream<Item = OperationState>)`: If the operation is successfully started.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn perform(
        &self,
        controller_id: Self::ControllerID,
    ) -> Result<Self::PerformReturn, Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to start the control operation of the controller {}",
                controller_id
            )
        );
        let url = format!("{}/ctrl_mgr/perform/{}", self.host, controller_id);
        let result = self.client.post(&url).send().await;
        Self::match_request_result(result, async |response: Response| StreamMapper {
            response,
            _t: PhantomData,
        })
        .await
    }

    /// Aborts the operation of a controller
    /// If the operation is not active, it does nothing.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn abort(&self, controller_id: Self::ControllerID) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to abort operation of the controller {}",
                controller_id
            )
        );
        let url = format!("{}/ctrl_mgr/abort/{}", self.host, controller_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Locks a specific controller.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn lock(&self, controller_id: Self::ControllerID) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to lock the controller {}",
                controller_id
            )
        );
        let url = format!("{}/ctrl_mgr/lock/{}", self.host, controller_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Unlocks a specific controller.
    ///
    /// # Returns
    /// - `Ok(())`: If the abort is requested without issues.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn unlock(&self, controller_id: Self::ControllerID) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to unlock the controller {}",
                controller_id
            )
        );
        let url = format!("{}/ctrl_mgr/unlock/{}", self.host, controller_id);
        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Activates the sensor of a controller.
    ///
    /// # Returns
    /// - `Ok(())`: If the activation was successful.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn start_sensor(
        &self,

        controller_id: Self::ControllerID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to start the sensor of the controller {}",
                controller_id
            )
        );

        let url = format!("{}/ctrl_mgr/start_sensor/{}", self.host, controller_id);

        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }

    /// Deactivates the sensor of a controller.
    ///
    /// # Returns
    /// - `Ok(())`: If the deactivation was successful.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn stop_sensor(
        &self,
        controller_id: Self::ControllerID,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = Self::CLIENT_LABEL,
            message = format!(
                "Sending HTTP request to stop the sensor of the controller {}",
                controller_id
            )
        );

        let url = format!("{}/ctrl_mgr/stop_sensor/{}", self.host, controller_id);

        let result = self.client.post(&url).send().await;
        Self::match_ok(result).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerOpts};
    use reqwest::ClientBuilder;

    use autonomic_controllers::controller::OpState;
    use autonomic_controllers::errors::ControllerError;

    const TEST_CTRL_MGR: &'static str = "ctrl_mgr";

    // --------------------------- Ok Tests ---------------------------
    #[tokio::test]
    async fn test_get_ctrl() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";
        let operation_info = ControllerInfo::from_str("Test Operation", "123", 0, false);

        // Response body as JSON
        let body = serde_json::to_string(&operation_info).unwrap();

        let _m = server
            .mock(
                "GET",
                format!("/{}/ctrl/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let result = api_client.ctrl(&controller_id).await.unwrap();

        assert_eq!(result, operation_info);
    }

    #[tokio::test]
    async fn test_get_ctrls() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let operations_info = vec![
            ControllerInfo::from_str("Ctrl_1", "1", 0, false),
            ControllerInfo::from_str("Ctrl_2", "2", 0, false),
        ];

        // Response body as JSON
        let body = serde_json::to_string(&operations_info).unwrap();

        let _m = server
            .mock("GET", format!("/{}/list", TEST_CTRL_MGR).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list().await.unwrap();

        assert_eq!(result, operations_info);
    }

    #[tokio::test]
    async fn test_get_performing_ctrls() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        // Response body as JSON array
        let body = r#"["ActiveOperation1", "ActiveOperation2"]"#;

        let _m = server
            .mock(
                "GET",
                format!("/{}/list_performing", TEST_CTRL_MGR).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list_performing().await.unwrap();

        assert_eq!(
            result,
            vec![
                "ActiveOperation1".to_string(),
                "ActiveOperation2".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_perform() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";
        let operation_state = OpState::Started;

        // Response body as JSON
        let body = serde_json::to_string(&operation_state).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/perform/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let mut stream_map = api_client.perform(controller_id).await.unwrap().map();

        while let Some(result) = stream_map.next().await {
            assert_eq!(result, operation_state);
        }
    }

    #[tokio::test]
    async fn test_abort() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/abort/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.abort(controller_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_lock_ctrl() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/lock/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.lock(controller_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unlock_ctrl() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/unlock/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.unlock(controller_id).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_sensor() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/start_sensor/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.start_sensor(controller_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_sensor() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "123";

        let _m = server
            .mock(
                "POST",
                format!("/{}/stop_sensor/{}", TEST_CTRL_MGR, controller_id).as_str(),
            )
            .with_status(200)
            .create();

        let client = ClientBuilder::new().build().unwrap();

        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.stop_sensor(controller_id).await;
        assert!(result.is_ok());
    }

    // --------------------------- Err Tests ---------------------------
    #[tokio::test]
    async fn test_get_ctrl_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let op_id = "123";
        let error_response = ControllerError::NotFound;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock("GET", format!("/{}/ctrl/{}", TEST_CTRL_MGR, op_id).as_str())
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.ctrl(op_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(ControllerError::NotFound))
        ));
    }

    #[tokio::test]
    async fn test_get_ctrls_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let error_response = ControllerError::NotFound;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock("GET", format!("/{}/list", TEST_CTRL_MGR).as_str())
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list().await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(ControllerError::NotFound))
        ));
    }

    #[tokio::test]
    async fn test_perform_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let op_id = "123";
        let error_response = ControllerError::Active;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/perform/{}", TEST_CTRL_MGR, op_id).as_str(),
            )
            .with_status(409)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let result = api_client.perform(op_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(ControllerError::Active))
        ));
    }

    #[tokio::test]
    async fn test_abort_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let op_id = "123";
        let error_response = ControllerError::NoResults;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock(
                "POST",
                format!("/{}/abort/{}", TEST_CTRL_MGR, op_id).as_str(),
            )
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.abort(op_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::NoResults
            ))
        ));
    }

    #[tokio::test]
    async fn test_list_performing_fails() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let error_response = ControllerError::NoResults;

        let body = serde_json::to_string(&error_response).unwrap();

        let _m = server
            .mock(
                "GET",
                format!("/{}/list_performing", TEST_CTRL_MGR).as_str(),
            )
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.list_performing().await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::NoResults
            ))
        ));
    }
}
