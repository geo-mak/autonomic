use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::Router;

use tokio::sync::Notify;

use autonomic_operation::{trace_error, trace_trace};

#[cfg(feature = "openapi-server-tls")]
use axum_server::tls_rustls::RustlsConfig;

#[cfg(feature = "openapi-server-tls")]
use axum_server::tls_rustls::RustlsAcceptor;

#[cfg(feature = "openapi-server-tls")]
use axum_server::Handle;

#[cfg(feature = "openapi-server-tls")]
use axum_server::Server;

/// OpenAPI server implementation.
///
/// > **Notes**:
/// > - Configurations are currently not supported, but they might be added in the future.
/// > - Support for TLS is optional and can be enabled with the `openapi-server-tls` feature.
pub struct OpenAPIServer {
    shutdown_tx: Arc<Notify>,
    #[cfg(feature = "openapi-server-tls")]
    shutdown_tx_tls: Handle,
}

impl OpenAPIServer {
    /// Creates a new instance of the OpenAPI server.
    pub fn new() -> Self {
        OpenAPIServer {
            shutdown_tx: Arc::new(Notify::new()),
            #[cfg(feature = "openapi-server-tls")]
            shutdown_tx_tls: Handle::new(),
        }
    }

    /// Starts the OpenAPI service on the provided address.
    ///
    /// > **Notes**:
    /// > - When called it will suspend the current task until termination.
    /// > - If suspending is not desired, consider spawning in a separate task.
    /// > - If it is running in a separate task, the `stop` method can be used to `gracefully` stop the service.
    /// > - Failure to bind to the address will result in an error message being logged but will **not** terminate the application.
    ///
    /// # Parameters
    /// - `address`: The address to bind the service to.
    /// - `router`: The router that contains the OpenAPI endpoints.
    pub async fn serve(&self, address: SocketAddr, router: Router) {
        trace_trace!(
            source = "OpenAPIServer",
            message = "Starting OpenAPI service..."
        );
        let shutdown_signal = self.shutdown_tx.clone();
        if let Ok(listener) = tokio::net::TcpListener::bind(address).await {
            trace_trace!(
                source = "OpenAPIServer",
                message = format!("OpenAPI service serving at: {}", address)
            );
            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    shutdown_signal.notified().await;
                })
                .await
            {
                trace_error!(
                    source = "OpenAPIServer",
                    message = format!("Failed to start service: {}", e)
                );
            }
        } else {
            trace_error!(
                source = "OpenAPIServer",
                message = format!("Failed to bind on {}", address)
            );
        }
    }

    /// Stops the OpenAPI service.
    pub fn stop(&self) {
        trace_trace!(
            source = "OpenAPIServer",
            message = "Initiating OpenAPI service shutdown..."
        );
        self.shutdown_tx.notify_one();
    }

    /// Starts the OpenAPI service with TLS on the provided address.
    /// Server does not manage cryptographic configurations.
    /// The process-level `CryptoProvider` must be set before starting the TLS server.
    ///
    /// # Parameters
    /// - `address`: The address to bind the service to.
    /// - `router`: The router that contains the OpenAPI endpoints.
    /// - `cert_path`: The path to the certificate file.
    /// - `key_path`: The path to the private key file.
    ///
    /// > **Notes**:
    /// > - When called it will suspend the current task until termination.
    /// > - If suspending is not desired, consider spawning in a separate task.
    /// > - If it is running in a separate task, the `stop_tls` method can be used to `gracefully` stop the service.
    /// > - Failure to bind to the address will result in an error message being logged but will **not** terminate the application.
    #[cfg(feature = "openapi-server-tls")]
    pub async fn serve_tls(
        &self,
        address: SocketAddr,
        router: Router,
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) {
        trace_trace!(
            source = "OpenAPIServer",
            message = "Starting OpenAPI TLS service..."
        );
        let io_result = RustlsConfig::from_pem_file(cert_path, key_path).await;

        match io_result {
            Ok(config) => {
                trace_trace!(
                    source = "OpenAPIServer",
                    message = "TLS configurations loaded successfully"
                );
                let server: Server<RustlsAcceptor> =
                    axum_server::bind_rustls(address, config).handle(self.shutdown_tx_tls.clone());
                trace_trace!(
                    source = "OpenAPIServer",
                    message = format!("OpenAPI TLS service serving at: {}", address)
                );
                if let Err(e) = server.serve(router.into_make_service()).await {
                    trace_error!(
                        source = "OpenAPIServer",
                        message = format!("Failed to start OpenAPI TLS service: {}", e)
                    );
                }
            }
            Err(e) => {
                trace_error!(
                    source = "OpenAPIServer",
                    message = format!("Failed to load TLS config: {}", e)
                );
            }
        }
    }

    /// Stops the OpenAPI service with TLS.
    #[cfg(feature = "openapi-server-tls")]
    pub fn stop_tls(&self) {
        trace_trace!(
            source = "OpenAPIServer",
            message = "Initiating OpenAPI TLS service shutdown..."
        );
        self.shutdown_tx_tls.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::{Router, routing::get};
    use reqwest::{Client, StatusCode};

    use autonomic_operation::testkit::tracing::init_tracing;

    async fn test_handler() -> StatusCode {
        StatusCode::OK
    }

    #[tokio::test]
    async fn test_server_insecure() {
        init_tracing();
        let router = Router::new().route("/", get(test_handler));
        let server = Arc::new(OpenAPIServer::new());

        let addr = "127.0.0.1:8000".parse().unwrap();

        let server_clone = server.clone();

        // Start the TLS server in a separate task
        tokio::spawn(async move {
            server_clone.serve(addr, router).await;
        });

        // Server needs some time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let client = Client::builder().build().unwrap();

        // Send HTTP request
        let response = client.get("http://127.0.0.1:8000").send().await;

        match response {
            Ok(resp) => {
                assert!(resp.status().is_success());
            }
            Err(e) => panic!("Failed to connect to TLS server: {:?}", e),
        }

        // Stop the server
        server.stop();
    }

    #[cfg(feature = "openapi-server-tls")]
    #[tokio::test]
    async fn test_server_tls() {
        init_tracing();
        let router = Router::new().route("/", get(test_handler));
        let server = Arc::new(OpenAPIServer::new());

        let addr = "127.0.0.1:8443".parse().unwrap();

        let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let cert_path = format!("{}/tls_test_keys/cert.pem", cargo_manifest_dir);
        let key_path = format!("{}/tls_test_keys/key.pem", cargo_manifest_dir);

        // Install the default crypto provider at the process level
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install crypto provider");

        let cloned_service = server.clone();

        // Start the TLS server in a separate task
        tokio::spawn(async move {
            cloned_service
                .serve_tls(addr, router, cert_path, key_path)
                .await;
        });

        // Server needs some time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Create a client and send a request to the server
        let client = Client::builder()
            .use_rustls_tls()
            .danger_accept_invalid_certs(true) // Accept self-signed certificates for testing
            .build()
            .unwrap();

        // Send HTTPS request
        let response = client.get("https://127.0.0.1:8443").send().await;

        match response {
            Ok(resp) => {
                assert!(resp.status().is_success());
            }
            Err(e) => panic!("Failed to connect to TLS server: {:?}", e),
        }

        // Stop the server
        server.stop_tls();
    }
}
