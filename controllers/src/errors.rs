use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum ControllerError {
    NotImplemented,
    NotFound,
    NoResults,
    Active,
    Locked,
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControllerError::NotImplemented => write!(f, "Not implemented"),
            ControllerError::NotFound => write!(f, "Not found"),
            ControllerError::NoResults => write!(f, "No results"),
            ControllerError::Active => write!(f, "Already active"),
            ControllerError::Locked => write!(f, "Controller is locked"),
        }
    }
}
