use chrono::{DateTime, Utc};
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use tracing::Event;
use tracing::Level;
use tracing::field::Field;

use tracing_core::Metadata;
use tracing_core::field::Visit;

use crate::traits::{EventRecorder, RecorderDirective};

/// Converts `Level` to byte representation.
pub fn level_to_byte(level: Level) -> u8 {
    match level {
        Level::TRACE => 0,
        Level::DEBUG => 1,
        Level::INFO => 2,
        Level::WARN => 3,
        Level::ERROR => 4,
    }
}

/// Default event schema to store recorded fields.
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
        level: Level,
        source: String,
        message: String,
        target: String,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            level: level_to_byte(level),
            source,
            message,
            target,
            timestamp,
        }
    }

    pub fn tracing_level(&self) -> Level {
        match self.level {
            0 => Level::TRACE,
            1 => Level::DEBUG,
            2 => Level::INFO,
            3 => Level::WARN,
            4 => Level::ERROR,
            // Level is not passed as byte in constructor.
            _ => unreachable!(),
        }
    }

    #[inline(always)]
    pub fn level(&self) -> u8 {
        self.level
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

/// Default fields visitor that records the following fields, if present:
/// - `source`
/// - `message`
pub struct DefaultEventVisitor {
    pub(crate) source: String,
    pub(crate) message: String,
}

// > Note: Types that don't have corresponding methods are recorded as `Debug` by default.
impl Visit for DefaultEventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        let len = value.len();

        // Empty values are skipped
        if len == 0 {
            return;
        }

        // Remove quotes if present, which is common when using `format!` macro with debug {:?}
        let val = if value.as_bytes()[0] == b'"' {
            let trimmed = &value[1..len - 1];
            // Empty values are skipped
            if trimmed.is_empty() {
                return;
            };
            trimmed
        } else {
            value
        };

        match field.name() {
            "source" => self.source = val.to_string(),
            "message" => self.message = val.to_string(),
            _ => {}
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        // Only message is allowed to be recorded as error
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "source" => self.source = format!("{:?}", value),
            "message" => self.message = format!("{:?}", value),
            _ => {}
        }
    }
}

/// Default directive enables recording default events.
pub struct DefaultDirective;

impl RecorderDirective for DefaultDirective {
    /// Checks if the recorder is enabled based on the current metadata.
    /// The result shall be cached by layers or filters or subscribers to avoid repeated calls
    /// for the same metadata.
    #[inline(always)]
    fn enabled(meta: &Metadata<'_>) -> bool {
        meta.name().starts_with("DE") // DE: Default Event
    }
}

/// Event recorder with `DefaultEventVisitor` as its recorder and `DefaultEvent` as its output.
///
/// # Type Parameters
/// - `T`: The directive type that checks if the recorder is enabled based on the current metadata.
/// If not provided, `DefaultDirective` is used by default.
///
/// > **Note**: Directive is intended to be used as _lightweight condition_ in `Layer<S>`
/// > or `Filter<S>` types.
/// > Callers should ensure that the directive is enabled before recording.
/// > Recording events without considering the fields is a major **waste of resources**.
/// > Directive must take into account the visitor and the fields to be recorded.
pub struct DefaultRecorder<T = DefaultDirective>
where
    T: RecorderDirective,
{
    _directive: PhantomData<T>,
}

impl<T> EventRecorder for DefaultRecorder<T>
where
    T: RecorderDirective,
{
    type Directive = T;
    type FieldsVisitor = DefaultEventVisitor;
    type Output = DefaultEvent;

    /// Records the fields in the event and returns recorded fields as `DefaultEvent`.
    fn record(event: &Event) -> Self::Output {
        let mut visitor = DefaultEventVisitor {
            source: String::new(),
            message: String::new(),
        };

        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        DefaultEvent::new(
            *event.metadata().level(),
            visitor.source,
            visitor.message,
            event.metadata().target().to_string(),
            Utc::now(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_schema_serde() {
        let original_schema = DefaultEvent::new(
            Level::INFO,
            "test_source".to_string(),
            "test_message".to_string(),
            "test_path".to_string(),
            Utc::now(),
        );

        let serialized =
            serde_json::to_string(&original_schema).expect("Failed to serialize DefaultEvent");

        let deserialized_schema: DefaultEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize DefaultEvent");

        assert_eq!(original_schema.level(), deserialized_schema.level());
        assert_eq!(original_schema.source(), deserialized_schema.source());
        assert_eq!(original_schema.message(), deserialized_schema.message());
        assert_eq!(original_schema.target(), deserialized_schema.target());
        assert_eq!(original_schema.timestamp(), deserialized_schema.timestamp());
    }
}
