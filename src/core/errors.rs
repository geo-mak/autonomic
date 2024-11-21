use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum ActivationError {
    Active,
    NotSet,
    Locked,
}

impl ActivationError {
    #[inline]
    pub fn is_locked(&self) -> bool {
        matches!(self, ActivationError::Locked)
    }
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivationError::Active => write!(f, "Already active"),
            ActivationError::NotSet => write!(f, "Not set"),
            ActivationError::Locked => write!(f, "Operation is locked"),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum ControllerError {
    NotImplemented,
    Empty,
    OpNotFound,
    NoActiveOps,
    ActivationErr(ActivationError),
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControllerError::NotImplemented => write!(f, "Not implemented"),
            ControllerError::Empty => write!(f, "Controller is empty"),
            ControllerError::OpNotFound => write!(f, "Operation not found"),
            ControllerError::NoActiveOps => write!(f, "No active operations"),
            ControllerError::ActivationErr(err) => write!(f, "Activation error: {}", err),
        }
    }
}
