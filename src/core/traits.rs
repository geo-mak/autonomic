use std::fmt::Display;
use std::sync::Arc;

use crate::core::sensor::Sensor;

/// Identification trait to identify types with valid identity.
pub trait Identity {
    // The type returned as a valid identity.
    type ID: Display + Clone;
    fn id(&self) -> Self::ID;
}

/// Description trait that gives information about the type.
pub trait Describe {
    // The type returned as a qualified description.
    type Description: Display + Clone;
    fn describe(&self) -> Self::Description;
}

/// Trait to convert a type into a sensor.
pub trait IntoSensor {
    /// Converts the type into a sensor.
    fn into_sensor(self) -> Sensor;
}

/// Trait to convert a type into a box.
pub trait IntoBox {
    /// Converts the type into a box.
    fn into_box(self) -> Box<Self>
    where
        Self: Sized;
}

/// Trait to convert a type into an Arc.
pub trait IntoArc {
    /// Converts the type into an Arc.
    fn into_arc(self) -> Arc<Self>
    where
        Self: Sized;
}
