use std::borrow::Cow;
use std::fmt;
use std::fmt::{Debug, Display};
use std::sync::atomic::AtomicU16;
use std::sync::atomic::Ordering::SeqCst;

use futures_util::FutureExt;
use tokio::sync::watch::{self, Receiver};

use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use autonomic_core::sync::{NotifyLast, Signal};
use autonomic_events::{trace_error, trace_info, trace_warn};

use crate::errors::ControllerError;

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
    pub fn abort(&self) -> NotifyLast<'_> {
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
    Err(Cow<'static, str>),
    Abort,
}

impl ControllerResult {
    /// Returns `Self::ErrMsg(value)` with static string literal.
    #[inline]
    pub const fn err(message: &'static str) -> Self {
        ControllerResult::Err(Cow::Borrowed(message))
    }
}

impl Display for ControllerResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControllerResult::Ok => write!(f, "Ok"),
            ControllerResult::Err(err) => write!(f, "Error: {err}"),
            ControllerResult::Abort => write!(f, "Abort"),
        }
    }
}

// Represents the current execution stage of the effector.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OpState {
    Started,
    Ok,
    Failed(Option<Cow<'static, str>>),
    Aborted,
    Panicked(String),
}

impl Display for OpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpState::Started => write!(f, "Started"),
            OpState::Ok => write!(f, "Ok"),
            OpState::Failed(result) => match result {
                Some(val) => write!(f, "Failed: {val}"),
                None => write!(f, "Failed"),
            },
            OpState::Aborted => write!(f, "Aborted"),
            OpState::Panicked(val) => write!(f, "Panicked: {val}"),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum ControllerState {
    Inactive = 0,
    Active = 0b0000_0010,
    Locked = 0b0000_0100,
}

impl fmt::Display for ControllerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControllerState::Inactive => write!(f, "Inactive"),
            ControllerState::Active => write!(f, "Active"),
            ControllerState::Locked => write!(f, "Locked"),
        }
    }
}

/// Provides information about a controller and its state.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ControllerInfo {
    id: Cow<'static, str>,
    description: Cow<'static, str>,
    state: u8,
}

impl ControllerInfo {
    pub const fn new(id: Cow<'static, str>, description: Cow<'static, str>, state: u8) -> Self {
        Self {
            id,
            description,
            state,
        }
    }

    pub const fn from_str(id: &'static str, description: &'static str, state: u8) -> Self {
        Self {
            id: Cow::Borrowed(id),
            description: Cow::Borrowed(description),
            state,
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
    pub fn state(&self) -> ControllerState {
        match self.state {
            s if s & CTRL_LOCKED != 0 => ControllerState::Locked,
            s if s & CTRL_ACTIVE != 0 => ControllerState::Active,
            _ => ControllerState::Inactive,
        }
    }
}

impl Display for ControllerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}, {}", self.id, self.description, self.state())
    }
}

const OP_ACTIVE: u8 = 0b0000_0001;
const CTRL_ACTIVE: u8 = 0b0000_0010;
const CTRL_LOCKED: u8 = 0b0000_0100;

struct AtomicState {
    // [ 8 bits Version | 8 bits State ]
    inner: AtomicU16,
}

impl AtomicState {
    const fn new(state: u8) -> Self {
        Self {
            inner: AtomicU16::new(state as u16),
        }
    }

    /// Loads the raw state.
    #[inline(always)]
    fn load(&self) -> u8 {
        self.inner.load(SeqCst) as u8
    }

    /// Loads version and state as (version, state).
    #[inline(always)]
    fn load_versioned(&self) -> (u8, u8) {
        let val = self.inner.load(SeqCst);
        ((val >> 8) as u8, val as u8)
    }

    /// Non-versioned `compare_exchange`.
    #[inline(always)]
    fn compare_exchange(&self, current: u8, new: u8) -> bool {
        self.inner
            .compare_exchange(current as u16, new as u16, SeqCst, SeqCst)
            .is_ok()
    }

    /// Versioned `compare_exchange` that does NOT increment the version.
    /// Compares both version and state, and if equal, sets the new state with the same version.
    #[inline(always)]
    fn compare_versioned_exchange(
        &self,
        current_version: u8,
        current_state: u8,
        new_state: u8,
    ) -> bool {
        let current = ((current_version as u16) << 8) | (current_state as u16);
        let new = ((current_version as u16) << 8) | (new_state as u16);
        self.inner
            .compare_exchange(current, new, SeqCst, SeqCst)
            .is_ok()
    }

    /// Versioned `fetch_or`.
    #[inline(always)]
    fn fetch_or_versioned(&self, state: u8) -> bool {
        self.fetch_update_versioned(|current| {
            let new_state = current | state;
            if current == new_state {
                None
            } else {
                Some(new_state)
            }
        })
        .is_some()
    }

    /// Non-versioned `fetch_and`.
    #[inline(always)]
    fn fetch_and(&self, state: u8) {
        self.inner.fetch_and(state as u16, SeqCst);
    }

    /// Non-versioned `fetch_update`.
    #[inline(always)]
    fn fetch_update<F>(&self, f: F) -> Option<u8>
    where
        F: Fn(u8) -> Option<u8>,
    {
        self.inner
            .fetch_update(SeqCst, SeqCst, |val| {
                f(val as u8).map(|new_val| new_val as u16)
            })
            .map(|prev| prev as u8)
            .ok()
    }

    /// Versioned `fetch_update`.
    #[inline(always)]
    fn fetch_update_versioned<F>(&self, f: F) -> Option<u8>
    where
        F: Fn(u8) -> Option<u8>,
    {
        self.inner
            .fetch_update(SeqCst, SeqCst, |val| {
                let version = (val >> 8) as u8;
                let current_state = val as u8;

                if let Some(new_state) = f(current_state) {
                    if current_state == new_state {
                        return None;
                    }

                    let new_version = version.wrapping_add(1);
                    Some(((new_version as u16) << 8) | (new_state as u16))
                } else {
                    None
                }
            })
            .map(|prev| prev as u8)
            .ok()
    }
}

pub struct ControlUnit {
    controller: Box<dyn Controller>,
    ctx: ControlContext,
    sig_stop: Signal,
    state: AtomicState,
}

impl ControlUnit {
    pub fn new(controller: impl Controller + 'static) -> Self {
        Self {
            controller: Box::new(controller),
            ctx: ControlContext::new(),
            sig_stop: Signal::new(),
            state: AtomicState::new(0),
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
            self.state.load(),
        )
    }

    /// Checks if the control operation is currently performing.
    #[inline(always)]
    pub fn performing(&self) -> bool {
        self.state.load() & OP_ACTIVE != 0
    }

    /// Checks if operation is currently locked.
    #[inline(always)]
    pub fn locked(&self) -> bool {
        self.state.load() & CTRL_LOCKED != 0
    }

    /// Checks if the sensor is currently active.
    #[inline(always)]
    pub fn sensing(&self) -> bool {
        self.state.load() & CTRL_ACTIVE != 0
    }

    /// Aborts the execution of the control operation, if it is currently performing.
    /// **Note**: It sends abort signal, but it does not guarantee the aborting.
    #[inline]
    pub fn abort(&self) {
        // No-op without active waiter -> operation must be active AND waiting.
        self.ctx.sig_abort.notify_waiter()
    }

    /// Starts the control operation and streams its execution states.
    pub fn perform(&'static self) -> Result<Receiver<OpState>, ControllerError> {
        // Single load for all checks.
        let current = self.state.load();

        // Initial CMP.
        if (current & CTRL_LOCKED) != 0 {
            trace_error!(message = "Controller locked");
            return Err(ControllerError::Locked);
        }

        // Initial CMP.
        if current & OP_ACTIVE != 0 {
            trace_warn!(message = "Operation already performing");
            return Err(ControllerError::Active);
        }

        let cmp_state = CTRL_LOCKED | OP_ACTIVE;

        // ABA is not a problem, if it is A right now, set it to B and go ahead.
        if self
            .state
            .fetch_update(|val| {
                if (val & cmp_state) == (current & cmp_state) {
                    Some(val | OP_ACTIVE)
                } else {
                    None
                }
            })
            .is_none()
        {
            trace_warn!(message = "Access denied");
            return Err(ControllerError::AccessDenied);
        }

        // Safe to proceed.
        trace_info!(message = "Started");

        let (tx, rx) = watch::channel(OpState::Started);

        tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(self.controller.perform(&self.ctx))
                .catch_unwind()
                .await;

            let state = match result {
                Ok(op_result) => match op_result {
                    ControllerResult::Ok => {
                        trace_info!(message = "Returned: Ok");
                        OpState::Ok
                    }
                    ControllerResult::Err(err) => {
                        trace_warn!(message = "Returned: Error");
                        OpState::Failed(Some(err))
                    }
                    ControllerResult::Abort => {
                        trace_warn!(message = "Returned: Abort");
                        OpState::Aborted
                    }
                },
                Err(err) => {
                    self.state.fetch_or_versioned(CTRL_LOCKED);
                    trace_error!(message = "Panicked");
                    OpState::Panicked(Self::panic_message(err))
                }
            };

            // Dispatch final state
            let _ = tx.send(state);
            self.state.fetch_and(!OP_ACTIVE);
        });
        Ok(rx)
    }

    /// Locks the controller to prevent it from performing, if it is currently locked.
    /// **Note**: It doesn't abort the operation if it is currently active.
    /// Operation is allowed to complete, but no new activation will be allowed.
    #[inline]
    pub fn lock(&self) {
        let ctrl_active = self
            .state
            .fetch_update_versioned(|current| {
                let ctrl_locked = current | CTRL_LOCKED;
                if current == ctrl_locked {
                    None
                } else {
                    Some(ctrl_locked)
                }
            })
            .map(|prev| prev & CTRL_ACTIVE != 0)
            .unwrap_or(false);

        if ctrl_active {
            self.sig_stop.notify();
        }

        trace_warn!(message = "Controller locked");
    }

    /// Unlocks the controller **only** if it is currently locked.
    /// Unlocking is versioned, if the lock-state is updated while trying to unlock, the request is no-op.
    #[inline]
    pub fn unlock(&self) {
        let (ver, current) = self.state.load_versioned();
        // Initial CMP.
        if current & CTRL_LOCKED != 0 {
            let unlocked = current & !CTRL_LOCKED;
            // If still locked with the same version -> unlock.
            if self
                .state
                .compare_versioned_exchange(ver, current, unlocked)
            {
                trace_info!(message = "Controller unlocked");
            }
        }
    }

    /// Deactivates the sensor of the controller, if it is currently active.
    #[inline]
    pub fn stop_sensor(&self) {
        if self.sensing() {
            self.sig_stop.notify();
        }
    }

    /// Activates the sensor of the controller.
    pub fn start_sensor(&'static self) -> Result<(), ControllerError> {
        // Single load for all checks.
        let current = self.state.load();

        // Initial CMP.
        if (current & CTRL_LOCKED) != 0 {
            trace_error!(message = "Controller locked");
            return Err(ControllerError::Locked);
        }

        // Initial CMP.
        if current & CTRL_ACTIVE != 0 {
            trace_warn!(message = "Sensor already active");
            return Err(ControllerError::Active);
        }

        // Clear any stale stop signals before the new activation attempt.
        // Only after successful activation can new signals be considered valid.
        self.sig_stop.clear();

        let cmp_state = CTRL_LOCKED | CTRL_ACTIVE;

        // ABA is not a problem, if it is A right now, set it to B and go ahead.
        if self
            .state
            .fetch_update(|val| {
                if (val & cmp_state) == (current & cmp_state) {
                    Some(val | CTRL_ACTIVE)
                } else {
                    None
                }
            })
            .is_none()
        {
            trace_warn!(message = "Access denied");
            return Err(ControllerError::AccessDenied);
        }

        // Safe to proceed.
        trace_info!(message = "Sensor activated");
        tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(async {
                loop {
                    tokio::select! {
                        _ = self.sig_stop.notified() => {
                            break;
                        }
                        _ = self.controller.notified() => {
                            // Note: If the current attempt to perform has failed, the controller must
                            // reevaluate the state again and decide if performing is still required.
                            let current = self.state.load();
                            if current & OP_ACTIVE == 0 && self.state.compare_exchange(
                                    current,
                                    current | OP_ACTIVE,
                                ) {
                                    // TODO: The returned result is useful for manual activation,
                                    // which is a `backdoor` option, but it doesn't fit well here.
                                    // Behaviors are needed when the operation encounters difficulties.
                                    // These behaviors report back to supervisory bodies and resolvers.
                                    let _ = self.controller.perform(&self.ctx).await;
                                    self.state.fetch_and(!OP_ACTIVE);
                                }
                        }
                    }
                }
            })
            .catch_unwind()
            .await;

            match result {
                // Shutdown request.
                Ok(_) => {
                    self.state.fetch_and(!CTRL_ACTIVE);
                }
                // Panic case.
                Err(err) => {
                    let _ = self.state.fetch_update_versioned(|current| {
                        Some((current | CTRL_LOCKED) & !(OP_ACTIVE | CTRL_ACTIVE))
                    });
                    trace_error!(
                        message = format!(
                            "Control operation has panicked: {}",
                            Self::panic_message(err)
                        )
                    );
                }
            }

            trace_info!(message = "Sensor deactivated");
        });

        Ok(())
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
}
