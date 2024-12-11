use reqwest::Client;

use futures_util::StreamExt;

use tokio_stream::Stream;

use crate::core::errors::ControllerError;
use crate::core::operation::{OpState, OperationInfo};
use crate::core::serde::AnySerializable;
use crate::core::service::ControllerClient;
use crate::trace_trace;

/// Client for interacting with the OpenAPI service.
/// It implements methods corresponding to the service endpoints and returns typed results.
#[derive(Clone)]
pub struct OpenAPIClient<'a> {
    client: Client, // Cloning just clones the inner Arc
    host: &'a str,
}

impl<'a> OpenAPIClient<'a> {
    /// Creates a new OpenAPIClient with the specified base URL.
    pub fn new(client: Client, host: &'a str) -> Self {
        OpenAPIClient { client, host }
    }
}

/// Error type for the OpenAPIClient.
#[derive(Debug)]
pub enum OpenAPIClientError {
    ResponseError(ControllerError),
    RequestError(reqwest::Error),
}

impl<'a> ControllerClient for OpenAPIClient<'a> {
    type ClientError = OpenAPIClientError;

    /// Retrieves a specific operation by its ID.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    /// - `operation_id`: A string slice that holds the operation ID.
    ///
    /// # Returns
    /// - `Ok(OperationInfo)`: If the operation is found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn operation(
        &self,
        controller_id: &str,
        operation_id: &str,
    ) -> Result<OperationInfo, Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to get operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/op/{}", self.host, controller_id, operation_id);
        let result = self.client.get(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    let op_info = response
                        .json()
                        .await
                        .expect("Failed to deserialize response result");
                    Ok(op_info)
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
    }

    /// Retrieves all operations for a specific controller.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    ///
    /// # Returns
    /// - `Ok(Vec<OperationInfo>)`: If the operations are found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn operations(
        &self,
        controller_id: &str,
    ) -> Result<Vec<OperationInfo>, Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to get all operations for controller={}",
                controller_id
            )
        );
        let url = format!("{}/{}/ops", self.host, controller_id);
        let result = self.client.get(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    let ops = response
                        .json::<Vec<OperationInfo>>()
                        .await
                        .expect("Failed to deserialize response result");
                    Ok(ops)
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
    }

    /// Retrieves the currently active operations for a specific controller.
    ///
    /// # Parameters
    /// - `controller_id`: A string slice that holds the controller ID.
    ///
    /// # Returns
    /// - `Ok(Vec<String>)`: If active operations are found.
    /// - `Err(OpenAPIClientError)`: If the response is Err or when the request fails.
    async fn active_operations(
        &self,
        controller_id: &str,
    ) -> Result<Vec<String>, Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to get active operations for controller={}",
                controller_id
            )
        );
        let url = format!("{}/{}/active_ops", self.host, controller_id);
        let result = self.client.get(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    let active_op = response
                        .json::<Vec<String>>()
                        .await
                        .expect("Failed to deserialize response result");
                    Ok(active_op)
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
        params: Option<&AnySerializable>,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to activate operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/activate/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).json(&params).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
        params: Option<&AnySerializable>,
    ) -> Result<impl Stream<Item = OpState>, Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
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
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    let stream = response.bytes_stream().map(|result| {
                        let event_bytes = result.expect("Failed to read event's bytes");
                        // Convert to string literal
                        let event_str = std::str::from_utf8(&event_bytes)
                            .expect("Failed to convert event's bytes to string");
                        // Remove `data` field from `MessageEvent`
                        let state_str = event_str.trim_start_matches("data: ");
                        // Parse as `OpState`
                        serde_json::from_str::<OpState>(&state_str)
                            .expect("Failed to deserialize event")
                    });
                    Ok(stream)
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to abort operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/abort/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
    async fn lock(&self, controller_id: &str, operation_id: &str) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to lock operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/lock/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
            message = format!(
                "Sending HTTP request to unlock operation={} for controller={}",
                operation_id, controller_id
            )
        );
        let url = format!("{}/{}/unlock/{}", self.host, controller_id, operation_id);
        let result = self.client.post(&url).send().await;
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
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
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
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
        controller_id: &str,
        operation_id: &str,
    ) -> Result<(), Self::ClientError> {
        trace_trace!(
            source = "OpenAPIClient",
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
        match result {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let err: ControllerError = response
                        .json()
                        .await
                        .expect("Failed to deserialize response error");
                    Err(OpenAPIClientError::ResponseError(err))
                }
            }
            Err(err) => Err(OpenAPIClientError::RequestError(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerOpts};
    use reqwest::ClientBuilder;

    use crate::core::errors::{ActivationError, ControllerError};
    use crate::core::operation::OpState;
    use crate::testkit::params::TestRetry;

    // --------------------------- Ok Tests ---------------------------
    #[tokio::test]
    async fn test_get_op() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operation_id = "123";
        let operation_info = OperationInfo::from_str("Test Operation", "123", false, false, false);

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

        let result = api_client
            .operation(&controller_id, &operation_id)
            .await
            .unwrap();

        assert_eq!(result, operation_info);
    }

    #[tokio::test]
    async fn test_get_ops() {
        let config = ServerOpts::default();
        let mut server = Server::new_with_opts_async(config).await;
        let host = server.url();

        let controller_id = "controller1";
        let operations_info = vec![
            OperationInfo::from_str("Operation1", "1", false, false, false),
            OperationInfo::from_str("Operation2", "2", false, false, false),
        ];

        // Response body as JSON
        let body = serde_json::to_string(&operations_info).unwrap();

        let _m = server
            .mock("GET", format!("/{}/ops", controller_id).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.operations(controller_id).await.unwrap();

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
            .mock("GET", format!("/{}/active_ops", controller_id).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.active_operations(controller_id).await.unwrap();

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

        let mut stream = api_client
            .activate_stream(controller_id, operation_id, None)
            .await
            .unwrap();

        while let Some(result) = stream.next().await {
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
            .with_body(body) // TODO: Not accurate because server sends SSE with MessageEvent with payload behind "data"
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);

        let params = AnySerializable::new_register(TestRetry::new(3, 1000));

        let mut stream = api_client
            .activate_stream(controller_id, operation_id, Some(&params))
            .await
            .unwrap();

        while let Some(result) = stream.next().await {
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
        let result = api_client.operation(controller_id, op_id).await;

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
            .mock("GET", format!("/{}/ops", controller_id).as_str())
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.operations(controller_id).await;

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
            .mock("GET", format!("/{}/active_ops", controller_id).as_str())
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let client = ClientBuilder::new().build().unwrap();
        let api_client = OpenAPIClient::new(client, &host);
        let result = api_client.active_operations(controller_id).await;

        assert!(matches!(
            result,
            Err(OpenAPIClientError::ResponseError(
                ControllerError::NoActiveOps
            ))
        ));
    }
}
