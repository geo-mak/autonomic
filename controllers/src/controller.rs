use std::borrow::Cow;
use std::fmt;
use std::fmt::{Debug, Display};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicBool, AtomicU8};

use futures_util::FutureExt;
use tokio::sync::watch::{self, Receiver};

use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use autonomic_core::sync::{Notification, Signal};
use autonomic_events::{trace_error, trace_info, trace_trace, trace_warn};

/// Exposes services and signals provided by the manager.
pub struct ControlContext {
    sig_abort: Signal,
}

impl ControlContext {
    #[inline]
    pub const fn new() -> Self {
        Self {
            sig_abort: Signal::new(),
        }
    }

    /// Returns a future that resolves when an abort is signaled.
    /// Note: Only the last waiter that acquires the future is notified.
    /// Previous pollers will not be notified.
    #[inline(always)]
    pub fn abort(&self) -> Notification<'_> {
        self.sig_abort.notified()
    }
}

/// Trait for implementing custom controllers.
#[async_trait]
pub trait Controller: Send + Sync {
    /// Returns the ID of the controller.
    fn id(&self) -> &'static str;

    /// Returns information about the role of the controller.
    fn description(&self) -> &'static str;

    /// Notifies the manager that the control operation needs to be started.
    /// This method will be awaited unit it returns, which indicates that its condition has been met.
    async fn notified(&self);

    /// Starts the control operation.
    ///
    /// The control context exposes services and signals provided by the manager.
    async fn perform(&self, ctx: &ControlContext) -> ControllerResult;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControllerResult {
    Ok,
    OkMsg(Cow<'static, str>),
    Err,
    ErrMsg(Cow<'static, str>),
    Abort,
    Lock(Cow<'static, str>),
}

impl ControllerResult {
    /// Returns `Self::OkMsg(value)` with static string literal.
    #[inline]
    pub const fn ok_msg(message: &'static str) -> Self {
        ControllerResult::OkMsg(Cow::Borrowed(message))
    }

    /// Returns `Self::ErrMsg(value)` with static string literal.
    #[inline]
    pub const fn err_msg(message: &'static str) -> Self {
        ControllerResult::ErrMsg(Cow::Borrowed(message))
    }

    /// Returns `Self::Lock(value)` with static string literal.
    #[inline]
    pub const fn lock(reason: &'static str) -> Self {
        ControllerResult::Lock(Cow::Borrowed(reason))
    }

    /// Checks if result is `Ok`.
    #[inline]
    pub const fn is_ok(&self) -> bool {
        matches!(self, ControllerResult::Ok | ControllerResult::OkMsg(_))
    }

    /// Checks if result is `Err`.
    #[inline]
    pub const fn is_err(&self) -> bool {
        matches!(self, ControllerResult::Err | ControllerResult::ErrMsg(_))
    }

    /// Checks if result is `Lock`.
    #[inline]
    pub const fn is_lock(&self) -> bool {
        matches!(self, ControllerResult::Lock(_))
    }
}

impl Display for ControllerResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControllerResult::Ok => write!(f, "Ok"),
            ControllerResult::OkMsg(val) => write!(f, "Ok: {}", val),
            ControllerResult::Err => write!(f, "Error"),
            ControllerResult::ErrMsg(err) => write!(f, "Error: {}", err),
            ControllerResult::Abort => write!(f, "Abort"),
            ControllerResult::Lock(msg) => write!(f, "Lock: {}", msg),
        }
    }
}

// Represents the current execution stage of the effector.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OpState {
    Started,
    Ok(Option<Cow<'static, str>>),
    Failed(Option<Cow<'static, str>>),
    Aborted,
    Locked(Option<Cow<'static, str>>),
    Panicked(String),
}

impl Display for OpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpState::Started => write!(f, "Started"),
            OpState::Ok(result) => match result {
                Some(val) => write!(f, "Ok: {}", val),
                None => write!(f, "Ok"),
            },
            OpState::Failed(result) => match result {
                Some(val) => write!(f, "Failed: {}", val),
                None => write!(f, "Failed"),
            },
            OpState::Aborted => write!(f, "Aborted"),
            OpState::Locked(result) => match result {
                Some(val) => write!(f, "Locked: {}", val),
                None => write!(f, "Locked"),
            },
            OpState::Panicked(val) => write!(f, "Panicked: {}", val),
        }
    }
}

/// Provides information about a controller and its state.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ControllerInfo {
    id: Cow<'static, str>,
    description: Cow<'static, str>,
    op_state: u8,
    sensing: bool,
    // TODO: Add last known state when the data API is ready
}

impl ControllerInfo {
    pub const fn new(
        id: Cow<'static, str>,
        description: Cow<'static, str>,
        op_state: u8,
        sensing: bool,
    ) -> Self {
        Self {
            id,
            description,
            op_state,
            sensing,
        }
    }

    pub const fn from_str(
        id: &'static str,
        description: &'static str,
        op_state: u8,
        sensing: bool,
    ) -> Self {
        Self {
            id: Cow::Borrowed(id),
            description: Cow::Borrowed(description),
            op_state,
            sensing,
        }
    }

    #[inline(always)]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[inline(always)]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[inline(always)]
    pub const fn performing(&self) -> bool {
        self.op_state == 1
    }

    #[inline(always)]
    pub const fn locked(&self) -> bool {
        self.op_state == 2
    }

    #[inline(always)]
    pub const fn sensing(&self) -> bool {
        self.sensing
    }
}

impl Display for ControllerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, {}, {}, {}, {}",
            self.id,
            self.description,
            self.performing(),
            self.locked(),
            self.sensing
        )
    }
}

pub struct ControlUnit {
    controller: Box<dyn Controller>,
    ctx: ControlContext,
    sig_stop: Signal,
    // State's value has the following semantics:
    // 0: Inactive
    // 1: Active
    // 2: Locked
    op_state: AtomicU8,
    sensing: AtomicBool,
}

impl ControlUnit {
    pub fn new(controller: impl Controller + 'static) -> Self {
        Self {
            controller: Box::new(controller),
            ctx: ControlContext::new(),
            sig_stop: Signal::new(),
            op_state: AtomicU8::new(0),
            sensing: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn id(&self) -> &'static str {
        self.controller.id()
    }

    #[inline(always)]
    pub fn description(&self) -> &'static str {
        self.controller.description()
    }

    pub fn info(&self) -> ControllerInfo {
        ControllerInfo::new(
            Cow::Borrowed(self.controller.id()),
            Cow::Borrowed(self.controller.description()),
            self.op_state.load(SeqCst),
            self.sensing.load(SeqCst),
        )
    }

    /// Checks if the control operation is currently performing.
    #[inline(always)]
    pub fn performing(&self) -> bool {
        self.op_state.load(SeqCst) == 1
    }

    /// Checks if operation is currently locked.
    #[inline(always)]
    pub fn locked(&self) -> bool {
        self.op_state.load(SeqCst) == 2
    }

    /// Locks the controller to prevent it from performing.
    /// > **Note**: It doesn't abort the operation if it is currently active.
    /// > Operation is allowed to complete, but no new activation will be allowed.
    #[inline]
    pub fn lock(&self) {
        self.op_state.store(2, SeqCst);
        trace_warn!(source = self.controller.id(), message = "Controller locked");
    }

    /// Unlocks the controller **only** if it is currently locked.
    #[inline]
    pub fn unlock(&self) {
        // Unlocking is only allowed if the operation is currently locked.
        if self.op_state.load(SeqCst) == 2 {
            self.op_state.store(0, SeqCst);
            trace_info!(
                source = self.controller.id(),
                message = "Controller unlocked"
            );
        }
    }

    /// Checks if the sensor is currently active.
    ///
    /// # Returns
    /// - `Ok(true)` - If sensing is active.
    /// - `Ok(false)` - If the sensing is not active.
    pub fn sensing(&self) -> bool {
        self.sensing.load(SeqCst)
    }

    fn panic_message(err: Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = err.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = err.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        }
    }

    /// Starts the control operation and streams its execution states.
    ///
    /// # Safety
    ///
    /// Controller must be checked by the manager if it is currently locked or if its op is active.
    pub fn perform(&'static self) -> Receiver<OpState> {
        let id = self.controller.id();
        // Set active to prevent another activation while running.
        self.op_state.store(1, SeqCst);
        trace_info!(source = id, message = "Started");
        let (tx, rx) = watch::channel(OpState::Started);
        tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(self.controller.perform(&self.ctx))
                .catch_unwind()
                .await;
            let state = match result {
                Ok(op_result) => match op_result {
                    ControllerResult::Ok => {
                        trace_info!(source = id, message = "Returned: Ok");
                        OpState::Ok(None)
                    }
                    ControllerResult::OkMsg(msg) => {
                        trace_info!(source = id, message = "Returned: Ok");
                        OpState::Ok(Some(msg))
                    }
                    ControllerResult::Err => {
                        trace_warn!(source = id, message = "Returned: Error");
                        OpState::Failed(None)
                    }
                    ControllerResult::ErrMsg(msg) => {
                        trace_warn!(source = id, message = "Returned: Error");
                        OpState::Failed(Some(msg))
                    }
                    ControllerResult::Abort => {
                        trace_warn!(source = id, message = "Returned: Abort");
                        OpState::Aborted
                    }
                    ControllerResult::Lock(msg) => {
                        self.op_state.store(2, SeqCst);
                        trace_warn!(source = id, message = "Returned: Lock");
                        let _ = tx.send(OpState::Locked(Some(msg)));
                        return; // <- Exit here.
                    }
                },
                Err(err) => {
                    self.op_state.store(2, SeqCst);
                    trace_error!(source = id, message = "Panicked");
                    let _ = tx.send(OpState::Panicked(Self::panic_message(err)));
                    return; // <- Exit here.
                }
            };
            // Dispatch final state
            self.op_state.store(0, SeqCst);
            let _ = tx.send(state);
        });
        rx
    }

    /// Aborts the execution of the control operation, if it is currently performing.
    /// > **Note**: It sends abort signal, but it does not guarantee the aborting.
    #[inline]
    pub fn abort(&self) {
        trace_trace!(source = self.controller.id(), message = "Abort requested");
        self.ctx.sig_abort.notify()
    }

    /// Activates the sensor of the controller.
    ///
    /// # Safety
    ///
    /// Controller must be checked by the manager if it is currently locked or sensing.
    pub fn start_sensor(&'static self) {
        let id = self.controller.id();
        self.sensing.store(true, SeqCst);
        trace_info!(source = id, message = "Sensor Activated");
        tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(async {
                loop {
                    if self.op_state.load(SeqCst) == 2 {
                        break;
                    }
                    tokio::select! {
                        _ = self.sig_stop.notified() => {
                            break;
                        }
                        _ = self.controller.notified() => {
                            if self.op_state.load(SeqCst) == 0 {
                                self.op_state.store(1, SeqCst);
                                let _ = self.controller.perform(&self.ctx).await;
                                self.op_state.store(0, SeqCst);
                            }
                        }
                    }
                }
            })
            .catch_unwind()
            .await;

            match result {
                Ok(_) => {
                    self.sensing.store(false, SeqCst);
                    trace_info!(source = id, message = "Sensor Deactivated");
                }
                Err(err) => {
                    self.sensing.store(false, SeqCst);
                    self.op_state.store(2, SeqCst);
                    trace_error!(
                        source = id,
                        message = format!(
                            "Control operation has panicked: {}",
                            Self::panic_message(err)
                        )
                    );
                }
            }
        });
    }

    /// Deactivates the sensor of the controller if it is currently active.
    pub fn stop_sensor(&self) {
        trace_trace!(
            source = self.controller.id(),
            message = "Sensor deactivation requested"
        );
        self.sig_stop.notify();
    }
}
