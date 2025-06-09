use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::dynamic::GenericSerializable;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TestCustomSchema {
    pub retries: u8,
    pub delay_ms: u32,
}

impl TestCustomSchema {
    pub fn new(retries: u8, delay_ms: u32) -> Self {
        Self { retries, delay_ms }
    }
}

impl GenericSerializable for TestCustomSchema {
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
