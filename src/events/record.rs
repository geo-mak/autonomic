use std::fmt;

use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};

use tracing::field::Field;
use tracing::Event;
use tracing::Level;
use tracing_core::field::Visit;

use crate::events::traits::EventRecorder;

#[derive(Serialize, Deserialize, Clone)]
pub struct DefaultEvent {
    level: u8,
    source: String,
    message: String,
    target: String,
    timestamp: DateTime<Utc>,
}

impl DefaultEvent {
    pub fn new(
        level: &Level,
        source: String,
        message: String,
        target: String,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            level: Self::level_to_byte(level),
            source,
            message,
            target,
            timestamp,
        }
    }

    fn level_to_byte(level: &Level) -> u8 {
        match level {
            &Level::TRACE => 0,
            &Level::DEBUG => 1,
            &Level::INFO => 2,
            &Level::WARN => 3,
            &Level::ERROR => 4,
        }
    }

    pub fn tracing_level(&self) -> Level {
        match self.level {
            0 => Level::TRACE,
            1 => Level::DEBUG,
            2 => Level::INFO,
            3 => Level::WARN,
            4 => Level::ERROR,
            _ => unreachable!(), // Unreachable anyway, level is not provided as byte in constructor
        }
    }

    #[inline(always)]
    pub fn level(&self) -> &u8 {
        &self.level
    }

    #[inline(always)]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[inline(always)]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[inline(always)]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[inline(always)]
    pub fn timestamp(&self) -> &DateTime<Utc> {
        &self.timestamp
    }
}

/// Default fields visitor that records fields of `DefaultEvent`.
pub struct DefaultEventVisitor {
    source: String,
    message: String,
    published: bool, // `false` by default
}

impl DefaultEventVisitor {
    fn new() -> Self {
        Self {
            source: String::new(),
            message: String::new(),
            published: false,
        }
    }
}

impl Visit for DefaultEventVisitor {
    fn record_bool(&mut self, field: &Field, value: bool) {
        match field.name() {
            "published" => {
                self.published = value;
            }
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = value.trim_matches('"').to_string(),
            "source" => self.source = value.trim_matches('"').to_string(),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{:?}", value),
            "source" => self.source = format!("{:?}", value),
            _ => {}
        }
    }
}

pub struct DefaultRecorder;

impl EventRecorder for DefaultRecorder {
    type Schema = DefaultEvent;
    type FieldsVisitor = DefaultEventVisitor;

    /// Records and transforms fields in the intercepted event and converts recorded fields to `DefaultEvent`.
    ///
    /// # Returns
    /// - `DefaultEvent`: If event satisfies the schema requirements.
    /// - `None`: If event fails to satisfy the schema requirements.
    fn record(event: &Event) -> Option<Self::Schema> {
        let mut visitor = DefaultEventVisitor::new();

        // Visit and record required fields
        event.record(&mut visitor);

        // Return None, if required fields not found, or they are empty
        if visitor.source.is_empty() || visitor.message.is_empty() {
            return None;
        }

        let event_schema = DefaultEvent::new(
            event.metadata().level(),
            visitor.source,
            visitor.message,
            event.metadata().target().to_string(),
            Utc::now(),
        );

        Some(event_schema)
    }
}

pub struct PublisherRecorder;

impl EventRecorder for PublisherRecorder {
    type Schema = DefaultEvent;
    type FieldsVisitor = DefaultEventVisitor;

    /// Records and transforms fields in the intercepted event and converts recorded fields to `DefaultEvent`.
    ///
    /// # Returns
    /// - `DefaultEvent`: If event satisfies the schema requirements.
    /// - `None`: If event fails to satisfy the schema requirements.
    fn record(event: &Event) -> Option<Self::Schema> {
        let mut visitor = DefaultEventVisitor::new();

        // Visit and record required fields
        event.record(&mut visitor);

        // Return None, if required fields not found, or they are empty
        if !visitor.published || visitor.source.is_empty() || visitor.message.is_empty() {
            return None;
        }

        let event_schema = DefaultEvent::new(
            event.metadata().level(),
            visitor.source,
            visitor.message,
            event.metadata().target().to_string(),
            Utc::now(),
        );

        Some(event_schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_schema_fields_access() {
        let level = Level::INFO;
        let source = String::from("test_source");
        let message = String::from("test_message");
        let target = String::from("test_target");
        let timestamp = Utc::now();

        let event = DefaultEvent::new(
            &level,
            source.clone(),
            message.clone(),
            target.clone(),
            timestamp,
        );

        assert_eq!(*event.level(), DefaultEvent::level_to_byte(&level));
        assert_eq!(event.source(), source);
        assert_eq!(event.message(), message);
        assert_eq!(event.target(), target);
        assert_eq!(event.timestamp(), &timestamp);
    }

    #[test]
    fn test_default_schema_serde() {
        let original_schema = DefaultEvent::new(
            &Level::INFO,
            "test_source".to_string(),
            "test_message".to_string(),
            "test_target".to_string(),
            Utc::now(),
        );

        let serialized =
            serde_json::to_string(&original_schema).expect("Failed to serialize DefaultSchema");

        let deserialized_schema: DefaultEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize DefaultSchema");

        assert_eq!(original_schema.level(), deserialized_schema.level());
        assert_eq!(original_schema.source(), deserialized_schema.source());
        assert_eq!(original_schema.message(), deserialized_schema.message());
        assert_eq!(original_schema.target(), deserialized_schema.target());
        assert_eq!(original_schema.timestamp(), deserialized_schema.timestamp());
    }
}
