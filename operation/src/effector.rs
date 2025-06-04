use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::SeqCst;
use tokio::sync::watch::Receiver;
use tokio::sync::{Notify, watch};
use tokio::task::JoinError;

use crate::operation::{OpState, Operation, OperationParameters, OperationResult};
use crate::{trace_error, trace_info, trace_warn};

struct EffectorData {
    operation: Box<dyn Operation>,
    abort_tx: Notify,
    // State's value has the following semantics:
    // 0: Inactive
    // 1: Active
    // 2: Locked
    state: AtomicU8,
}

/// Effector manages the activation and the lifecycle of an operation.
/// Effector is also responsible for dispatching and reporting the state and result of an operation to other subsystems.
pub(super) struct Effector {
    data: Arc<EffectorData>,
}

impl Effector {
    /// Creates new `Effector` instance.
    pub(super) fn new(operation: impl Operation + 'static) -> Self {
        Self {
            data: Arc::new(EffectorData {
                operation: Box::new(operation),
                abort_tx: Notify::new(),
                state: AtomicU8::new(0), // Default state is 0 (inactive)
            }),
        }
    }

    /// Returns the id of the operation.
    #[inline(always)]
    pub(super) fn id(&self) -> &'static str {
        self.data.operation.id()
    }

    /// Returns the description of the operation.
    #[inline]
    pub(super) fn describe(&self) -> &'static str {
        self.data.operation.describe()
    }

    /// Checks if operation is currently active.
    #[inline(always)]
    pub(super) fn is_active(&self) -> bool {
        self.data.state.load(SeqCst) == 1
    }

    /// Checks if the operation is currently locked.
    #[inline(always)]
    pub(super) fn is_locked(&self) -> bool {
        self.data.state.load(SeqCst) == 2
    }

    /// Locks the operation.
    /// > **Note**: It doesn't abort the operation if it is currently active.
    /// > Operation is allowed to complete, but no new activation will be allowed.
    #[inline]
    pub(super) fn lock(&self) {
        self.data.state.store(2, SeqCst);
        trace_warn!(
            source = self.data.operation.id(),
            message = "Operation locked"
        );
    }

    /// Unlocks the operation **only** if it is currently locked.
    #[inline]
    pub(super) fn unlock(&self) {
        // > **Safety**: Unlocking is only allowed if the operation is currently locked.
        if self.data.state.load(SeqCst) == 2 {
            self.data.state.store(0, SeqCst);
            trace_info!(
                source = self.data.operation.id(),
                message = "Operation unlocked"
            );
        }
    }

    /// Activates operation with the provided parameters.
    ///
    /// > **Safety**:
    /// > - Guarding for **activation** and **locking** must be ensured by the calling context.
    /// > - Effector doesn't guard for activation and locking to minimize the overhead.
    ///
    /// # Parameters
    /// - `parameters`: The operation's parameters for this activation cycle.
    ///
    /// > **Note**:
    /// > - `Arc` is used for easy interfacing with sensor.
    pub(super) fn activate(&self, parameters: Option<Arc<dyn OperationParameters>>) {
        // Update state to 1 (active) to prevent new activation
        self.data.state.store(1, SeqCst);
        let op_id: &str = self.data.operation.id();
        trace_info!(source = op_id, message = "Active");
        // Cloned reference for the execution block
        let data_ref = self.data.clone();
        // Execution domain
        tokio::spawn(async move {
            // New reference for performing detached task
            let op_data_ref = data_ref.clone();
            let result: Result<OperationResult, JoinError> = tokio::select! {
                // Abort requested
                _ = data_ref.abort_tx.notified() => {
                    // Update state to 0 (inactive)
                    data_ref.state.store(0, SeqCst);
                    trace_info!(source = op_id, message = "Aborted");
                    return;
                },
                // Run in detached task, otherwise the entire thread will panic, when operation panics
                result = tokio::spawn(async move {
                    // This might panic, nothing else should be put here
                    op_data_ref.operation.perform(parameters.as_deref()).await
                }) => {
                        result
            }
            };

            // TODO: Storing result when the data API is ready
            match result {
                // Detailed result is not included in events
                Ok(op_result) => match &op_result {
                    // Expected ok
                    OperationResult::Ok | OperationResult::OkMsg(_) => {
                        trace_info!(source = op_id, message = "Completed: Ok");
                    }
                    // Expected error
                    OperationResult::Err | OperationResult::ErrMsg(_) => {
                        trace_warn!(source = op_id, message = "Completed: Error");
                    }
                    // Locking result
                    OperationResult::Lock(_) => {
                        // Update state to 2 (locked)
                        data_ref.state.store(2, SeqCst);
                        trace_warn!(source = op_id, message = "Completed: Lock");
                        return; // Exit here to keep the lock
                    }
                },
                // Unexpected error
                Err(_) => {
                    // Update state to 2 (locked)
                    data_ref.state.store(2, SeqCst);
                    trace_error!(source = op_id, message = "Completed: Panicked");
                    return; // Exit here to keep the lock
                }
            };
            // Operation completed, deactivate guard
            data_ref.state.store(0, SeqCst);
        });
    }

    /// Activates operation with the provided parameters and stream its state updates.
    ///
    /// > **Safety**:
    /// > - Guarding for **activation** and **locking** must be ensured by the calling context.
    /// > - Effector doesn't guard for activation and locking to minimize the overhead.
    ///
    /// # Parameters
    /// - `parameters`: The operation's parameters for this activation cycle.
    ///
    /// # Returns
    /// - `Receiver<OpState>`: If activation was successful.
    ///
    /// > **Note**:
    /// > - For activation requests with high frequency that don't require streaming,
    /// >  consider using `activate` method instead.
    pub(super) fn activate_stream(
        &self,
        parameters: Option<Box<dyn OperationParameters>>,
    ) -> Receiver<OpState> {
        // Update state to 1 (active) to prevent new activation
        self.data.state.store(1, SeqCst);
        let op_id: &str = self.data.operation.id();
        trace_info!(source = op_id, message = "Active");
        // Send active state
        let (tx, rx) = watch::channel(OpState::Active);
        // Cloned reference for the execution block
        let data_ref = self.data.clone();
        // Execution domain
        tokio::spawn(async move {
            // New reference for performing detached task
            let data_ref_clone = data_ref.clone();
            let result: Result<OperationResult, JoinError> = tokio::select! {
                    // Abort requested
                _ = data_ref.abort_tx.notified() => {
                   // Update state to 0 (inactive)
                    data_ref.state.store(0, SeqCst);
                    trace_info!(source = op_id, message = "Aborted");
                    let _ = tx.send(OpState::Aborted);
                    return;
                },
                    // Run in detached task, otherwise the entire thread will panic, when operation panics
                    result = tokio::spawn(async move {
                    // This might panic, nothing else should be put here
                    data_ref_clone.operation.perform(parameters.as_deref()).await
                }) => {
                        result
            }
            };

            // TODO: Storing result when the data API is ready
            // Match the final state
            let state = match result {
                Ok(op_result) => {
                    // Detailed result is not included in events
                    match op_result {
                        // Expected ok
                        OperationResult::Ok => {
                            trace_info!(source = op_id, message = "Completed: Ok");
                            OpState::Ok(None)
                        }
                        OperationResult::OkMsg(msg) => {
                            trace_info!(source = op_id, message = "Completed: Ok");
                            OpState::Ok(Some(msg))
                        }
                        // Expected error
                        OperationResult::Err => {
                            trace_warn!(source = op_id, message = "Completed: Error");
                            OpState::Failed(None)
                        }
                        OperationResult::ErrMsg(msg) => {
                            trace_warn!(source = op_id, message = "Completed: Error");
                            OpState::Failed(Some(msg))
                        }
                        // Locking result
                        OperationResult::Lock(msg) => {
                            // Update state to 2 (locked)
                            data_ref.state.store(2, SeqCst);
                            trace_warn!(source = op_id, message = "Completed: Lock");
                            // Dispatch final state
                            let _ = tx.send(OpState::Locked(Some(msg)));
                            return; // Exit here to keep the lock
                        }
                    }
                }
                // Unexpected error
                Err(join_err) => {
                    // Update state to 2 (locked)
                    data_ref.state.store(2, SeqCst);
                    trace_error!(source = op_id, message = "Completed: Panicked");
                    // Dispatch final state
                    let _ = tx.send(OpState::Panicked(join_err.to_string()));
                    return; // Exit here to keep the lock
                }
            };
            // Dispatch final state
            let _ = tx.send(state);
            // Operation completed, deactivate guard
            data_ref.state.store(0, SeqCst);
        });
        rx
    }

    /// Aborts the operation.
    /// > **Note**: It sends abort signal, but it does not guarantee the aborting.
    #[inline]
    pub(super) fn abort(&self) {
        self.data.abort_tx.notify_one()
    }
}
