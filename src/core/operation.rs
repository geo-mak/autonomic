use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::fmt::{Debug, Display};
use std::sync::Arc;

use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use crate::core::serde::{AnySerializable, GenericSerializable, IntoAnySerializable};
use crate::core::traits::{Describe, Identity, IntoArc, IntoBox};

/// Operation trait to define operations.
/// Operations are concurrent and independent execution contexts that encapsulate metadata and execution logic.
///
/// > **Notes**:
/// > - Keep the operation simple and focused on its core functionality.
///   Narrow-focused implementation improves understanding its side effects, leading to more predictable
///   behavior, easier debugging, and cleaner interactions within the system.
/// > - If you need conditional self-initiated activation, consider using the [`Sensor`] component.
///   The [`Sensor`] provides a structured and managed approach to self-activation, making the process
///   predictable, consistent, and easier to maintain.
///
/// [`Sensor`]: crate::core::sensor::Sensor
#[async_trait]
pub trait Operation:
    Send + Sync + Identity<ID = &'static str> + Describe<Description = &'static str>
{
    /// Starts the operation with optional parameters.
    /// Parameters can be any type that implements `OperationParameters` trait.
    async fn perform(&self, parameters: Option<&dyn OperationParameters>) -> OperationResult;
}

/// The returned result type of operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OperationResult {
    Ok,
    OkStr(Cow<'static, str>),
    Err,
    ErrStr(Cow<'static, str>),
    Lock(Cow<'static, str>),
}

impl OperationResult {
    /// Returns `Self::OkStr(value)` with static string literal.
    #[inline]
    pub fn ok_str(message: &'static str) -> Self {
        OperationResult::OkStr(Cow::Borrowed(message))
    }

    /// Returns `Self::ErrStr(value)` with static string literal.
    #[inline]
    pub fn err_str(message: &'static str) -> Self {
        OperationResult::ErrStr(Cow::Borrowed(message))
    }

    /// Returns `Self::Lock(value)` with static string literal.
    #[inline]
    pub fn lock(reason: &'static str) -> Self {
        OperationResult::Lock(Cow::Borrowed(reason))
    }

    /// Checks if result is `Ok`.
    #[inline]
    pub fn is_ok(&self) -> bool {
        matches!(self, OperationResult::Ok | OperationResult::OkStr(_))
    }

    /// Checks if result is `Err`.
    #[inline]
    pub fn is_err(&self) -> bool {
        matches!(self, OperationResult::Err | OperationResult::ErrStr(_))
    }

    /// Checks if result is `Lock`.
    #[inline]
    pub fn is_lock(&self) -> bool {
        matches!(self, OperationResult::Lock(_))
    }
}

impl Display for OperationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationResult::Ok => write!(f, "Ok"),
            OperationResult::OkStr(val) => write!(f, "Ok: {}", val),
            OperationResult::Err => write!(f, "Error"),
            OperationResult::ErrStr(err) => write!(f, "Error: {}", err),
            OperationResult::Lock(msg) => write!(f, "Lock: {}", msg),
        }
    }
}

pub trait OperationParameters: Any + Send + Sync + Debug {
    fn as_parameters(&self) -> &dyn Any;
}

impl<T> IntoBox for T
where
    T: OperationParameters,
{
    #[inline]
    fn into_box(self) -> Box<Self> {
        Box::new(self)
    }
}

impl<T> IntoArc for T
where
    T: OperationParameters,
{
    #[inline]
    fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl<T> GenericSerializable for T
where
    T: OperationParameters + Serialize + for<'de> Deserialize<'de> + Clone,
{
    #[inline]
    fn serialize(&self) -> Result<Vec<u8>, String> {
        match serde_json::to_vec(self) {
            Ok(s) => Ok(s),
            Err(err) => Err(err.to_string()),
        }
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn clone_boxed(&self) -> Box<dyn GenericSerializable> {
        Box::new(self.clone())
    }
}

impl<T> IntoAnySerializable for T
where
    T: OperationParameters + Serialize + for<'de> Deserialize<'de> + Clone,
{
    #[inline]
    fn into_any_serializable(self) -> AnySerializable {
        AnySerializable::new(self)
    }
}

impl OperationParameters for () {
    #[inline]
    fn as_parameters(&self) -> &dyn Any {
        self
    }
}

// `OpState` represents the current execution stage of an operation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OpState {
    Active,
    Ok(Option<Cow<'static, str>>),
    Failed(Option<Cow<'static, str>>),
    Locked(Option<Cow<'static, str>>),
    Aborted,
    Panicked(String), // Will be owned string anyway
}

impl Display for OpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpState::Active => write!(f, "Active"),
            OpState::Ok(result) => match result {
                Some(val) => write!(f, "Ok: {}", val),
                None => write!(f, "Ok"),
            },
            OpState::Failed(result) => match result {
                Some(val) => write!(f, "Failed: {}", val),
                None => write!(f, "Failed"),
            },
            OpState::Locked(result) => match result {
                Some(val) => write!(f, "Locked: {}", val),
                None => write!(f, "Locked"),
            },
            OpState::Aborted => write!(f, "Aborted"),
            OpState::Panicked(val) => write!(f, "Panicked: {}", val),
        }
    }
}

/// Lightweight operation information that is fully serializable.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct OperationInfo {
    id: Cow<'static, str>,
    description: Cow<'static, str>,
    active: bool,
    locked: bool,
    active_sensor: bool,
    // TODO: Add last known state when the data API is ready
}

impl OperationInfo {
    pub fn new(
        id: Cow<'static, str>,
        description: Cow<'static, str>,
        active: bool,
        locked: bool,
        active_sensor: bool,
    ) -> Self {
        Self {
            id,
            description,
            active,
            locked,
            active_sensor,
        }
    }

    pub fn from_str(
        id: &'static str,
        description: &'static str,
        active: bool,
        locked: bool,
        active_sensor: bool,
    ) -> Self {
        Self {
            id: Cow::Borrowed(id),
            description: Cow::Borrowed(description),
            active,
            locked,
            active_sensor,
        }
    }

    #[inline]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[inline]
    pub fn active(&self) -> bool {
        self.active
    }

    #[inline]
    pub fn locked(&self) -> bool {
        self.locked
    }

    #[inline]
    pub fn active_sensor(&self) -> bool {
        self.active_sensor
    }
}

impl Display for OperationInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OperationInfo {{ id: {}, description: {}, active: {}, locked: {}, active_sensor: {} }}",
            self.id, self.description, self.active, self.locked, self.active_sensor
        )
    }
}
