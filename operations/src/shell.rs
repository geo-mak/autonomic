use std::any::Any;
use std::borrow::Cow;
use std::process::Output;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use autonomic_core::operation::{Operation, OperationParameters, OperationResult};
use autonomic_core::trace_info;
use autonomic_core::traits::{Describe, Identity};

use crate::parameters::Retry;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShellParameters {
    program: String,
    args: String,
    retry: Option<Retry>,
}

impl ShellParameters {
    pub fn new(program: &str, args: &str, retry: Option<Retry>) -> Self {
        ShellParameters {
            program: program.to_string(),
            args: args.to_string(),
            retry,
        }
    }
}

impl OperationParameters for ShellParameters {
    fn as_parameters(&self) -> &dyn Any {
        self
    }
}

pub struct ShellOperation {
    id: &'static str,
    description: &'static str,
}

impl ShellOperation {
    pub fn new(id: &'static str, description: &'static str) -> Self {
        ShellOperation { id, description }
    }

    #[inline]
    async fn execute_command(program: &str, args: &str) -> Result<Output, std::io::Error> {
        if args.is_empty() {
            Command::new(program).output().await
        } else {
            Command::new(program).arg("-c").arg(args).output().await
        }
    }

    async fn run(program: &str, args: &str) -> OperationResult {
        match Self::execute_command(program, args).await {
            Ok(output) => match output.status.success() {
                true if output.stdout.is_empty() => OperationResult::Ok,
                true => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    OperationResult::OkMsg(Cow::Owned(stdout))
                }
                false if output.stderr.is_empty() => OperationResult::Err,
                false => {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    OperationResult::ErrMsg(Cow::Owned(stderr))
                }
            },
            Err(err) => OperationResult::ErrMsg(Cow::Owned(err.to_string())),
        }
    }
}

impl Identity for ShellOperation {
    type ID = &'static str;

    #[inline]
    fn id(&self) -> Self::ID {
        self.id
    }
}

impl Describe for ShellOperation {
    type Description = &'static str;

    #[inline]
    fn describe(&self) -> Self::Description {
        self.description
    }
}

#[async_trait]
impl Operation for ShellOperation {
    async fn perform(&self, parameters: Option<&dyn OperationParameters>) -> OperationResult {
        // Extract parameters
        let params = match parameters {
            Some(params) => params,
            None => return OperationResult::err_msg("Parameters required"),
        };

        // Check if the parameters are of the expected type
        let cmd = match params.as_parameters().downcast_ref::<ShellParameters>() {
            Some(value) => value,
            None => return OperationResult::err_msg("Unexpected parameters"),
        };

        // Program must be specified
        if cmd.program.is_empty() {
            return OperationResult::err_msg("Program not specified");
        }

        // Execute
        let mut result = Self::run(&cmd.program, &cmd.args).await;

        // Try retry only if operation has failed
        if result.is_err() {
            if let Some(retry_params) = &cmd.retry {
                if retry_params.retries > 0 {
                    let id_str = self.id;
                    for i in 1..=retry_params.retries {
                        tokio::time::sleep(Duration::from_millis(retry_params.delay_ms as u64))
                            .await;
                        trace_info!(
                            source = id_str,
                            message = format!("Attempt={} to perform", i)
                        );
                        result = Self::run(&cmd.program, &cmd.args).await;
                        if result.is_ok() {
                            break;
                        }
                    }
                }
            }
        }

        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use autonomic_core::testkit::tracing::init_tracing;

    #[tokio::test]
    async fn test_basic_command_execution() {
        init_tracing();
        let operation = ShellOperation::new("bash_operation", "Test basic echo command");

        // Command
        let params = ShellParameters::new("bash", "echo Welcome to autonomic", None);

        let result = operation.perform(Some(&params)).await;
        assert!(result.is_ok());
        assert_eq!(result.to_string(), "Ok: Welcome to autonomic\n");
    }

    #[tokio::test]
    async fn test_failed_command_execution() {
        init_tracing();
        let operation = ShellOperation::new("bash_operation", "Test invalid command");

        // Parameters with invalid command
        let params = ShellParameters::new("bash", "invalid_command", None);

        let result = operation.perform(Some(&params)).await;

        // Check if the command returned an error
        assert!(result.is_err())
    }

    #[tokio::test]
    async fn test_retry_mechanism() {
        init_tracing();
        let operation = ShellOperation::new("bash_operation", "Test retry mechanism");

        // Parameters that fails initially
        let params = ShellParameters {
            program: "bash".to_string(),
            args: "false".to_string(), // `false` is a command that returns an error status
            retry: Some(Retry {
                retries: 3,
                delay_ms: 100,
            }),
        };

        let result = operation.perform(Some(&params)).await;

        // Check if the command returned an error
        assert!(result.is_err())
    }

    #[tokio::test]
    async fn test_perform_no_parameters() {
        init_tracing();
        let operation = ShellOperation::new("bash_operation", "Test invalid parameters");

        // Passing None to params should lead to an error
        let result = operation.perform(None).await;

        // Check if the command returned an error
        assert!(result.is_err())
    }

    #[tokio::test]
    async fn test_program_with_arguments() {
        init_tracing();

        // Create a sample file with known content
        let file_content = "hello autonomic\nfoo bar\n";
        let temp_file_path = "/tmp/test_grep_file.txt";
        tokio::fs::write(temp_file_path, file_content)
            .await
            .unwrap();

        // Define the operation
        let operation = ShellOperation::new("grep_operation", "Test grep command with arguments");

        // Command to search for "hello" in the temporary file
        let command = format!("grep 'hello' {}", temp_file_path);
        let params = ShellParameters::new("bash", &command, None);

        // Run the command
        let result = operation.perform(Some(&params)).await;

        assert_eq!(result.to_string(), "Ok: hello autonomic\n");

        // Clean up the temporary file
        tokio::fs::remove_file(temp_file_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_piped_command_execution() {
        init_tracing();
        let operation = ShellOperation::new("pipe_operation", "Test command with pipe");

        // Command to echo "hello autonomic" and pipe it to awk to extract "autonomic"
        let piped_command = "echo 'hello autonomic' | awk '{print $2}'";
        let params = ShellParameters::new("bash", piped_command, None);

        // Run the command
        let result = operation.perform(Some(&params)).await;

        assert_eq!(result.to_string(), "Ok: autonomic\n");
    }

    #[tokio::test]
    async fn test_command_with_flags_and_env() {
        init_tracing();
        let operation =
            ShellOperation::new("env_operation", "Test command with flags and env variables");

        // Command to echo the environment variable MY_VAR
        let command = "MY_VAR=hello_autonomic printenv MY_VAR";
        let params = ShellParameters::new("bash", command, None);

        // Run the command
        let result = operation.perform(Some(&params)).await;

        assert_eq!(result.to_string(), "Ok: hello_autonomic\n");
    }
}
