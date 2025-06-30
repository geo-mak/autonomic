use std::error::Error;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use tracing::Event;
use tracing::Level;
use tracing::field::Field;

use tracing_core::Metadata;
use tracing_core::field::Visit;

use chrono::{DateTime, Utc};

use autonomic_core::thread_local_instance;
use autonomic_core::traits::ThreadLocalInstance;

use crate::traits::{EventRecorder, RecorderDirective};

#[derive(Default)]
pub struct DefaultEventSchema {
    pub source: String,
    pub message: String,
}

impl DefaultEventSchema {
    #[inline]
    pub fn clear(&mut self) {
        self.source.clear();
        self.message.clear();
    }
}

thread_local_instance!(__TLS_DEFAULT_EVENT_SCHEMA, DefaultEventSchema);

impl ThreadLocalInstance for DefaultEventSchema {
    #[inline]
    fn thread_local<F, I>(f: F) -> I
    where
        F: FnOnce(&mut Self) -> I,
    {
        __TLS_DEFAULT_EVENT_SCHEMA.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// Converts `Level` to byte representation.
#[inline]
pub const fn level_to_byte(level: Level) -> u8 {
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
    pub(crate) level: u8,
    pub(crate) source: String,
    pub(crate) message: String,
    pub(crate) target: String,
    pub(crate) timestamp: DateTime<Utc>,
}

impl DefaultEvent {
    pub const fn new(
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

    pub const fn tracing_level(&self) -> Level {
        match self.level {
            0 => Level::TRACE,
            1 => Level::DEBUG,
            2 => Level::INFO,
            3 => Level::WARN,
            4 => Level::ERROR,
            _ => unreachable!(),
        }
    }

    #[inline(always)]
    pub const fn level(&self) -> u8 {
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

/// Creates a new `DefaultSchema` instance with default values for all fields.
///
/// - `level`: Set to `0`.
/// - `source`: An empty `String`.
/// - `message`: An empty `String`.
/// - `target`: An empty `String`.
/// - `timestamp`: Set to the current UTC time at the moment of creation.
///
/// This provides a baseline event with no specific source, message, or target,
/// and a level of zero.
impl Default for DefaultEvent {
    #[inline]
    fn default() -> Self {
        Self {
            level: 0,
            source: String::new(),
            message: String::new(),
            target: String::new(),
            timestamp: Utc::now(),
        }
    }
}

/// A visitor struct that holds mutable references to a source and message string.
///
/// # Recording Fields
/// - `source`: As str or debug only.
/// - `message`: As str, debug or error.
pub struct DefaultEventVisitor<'a> {
    source: &'a mut String,
    message: &'a mut String,
}

impl<'a> DefaultEventVisitor<'a> {
    #[inline(always)]
    pub const fn new(source: &'a mut String, message: &'a mut String) -> Self {
        Self { source, message }
    }
}

// > Note: Types that don't have corresponding methods are recorded as `Debug` by default.
impl<'a> Visit for DefaultEventVisitor<'a> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let len = value.len();

        // Empty values are skipped
        if len == 0 {
            return;
        }

        // Remove quotes if present, which is common when using `format!` macro with debug {:?}
        let val = if len >= 2 {
            let bytes = value.as_bytes();
            if bytes[0] == b'"' && bytes[len - 1] == b'"' {
                let trimmed = &value[1..len - 1];
                // Empty values are skipped
                if trimmed.is_empty() {
                    return;
                }
                trimmed
            } else {
                value
            }
        } else {
            value
        };

        match field.name() {
            "source" => {
                self.source.push_str(val);
            }
            "message" => {
                self.message.push_str(val);
            }
            _ => {}
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        // Only message is allowed to be recorded as error
        if field.name() == "message" {
            *self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "source" => *self.source = format!("{value:?}"),
            "message" => *self.message = format!("{value:?}"),
            _ => {}
        }
    }
}

const DEFAULT_EVENT: &str = "DE";

/// Default directive enables recording default events.
pub struct DefaultDirective;

impl RecorderDirective for DefaultDirective {
    /// Checks if the recorder is enabled based on the current metadata.
    /// The result shall be cached by layers or filters or subscribers to avoid repeated calls
    /// for the same metadata.
    #[inline(always)]
    fn enabled(meta: &Metadata<'_>) -> bool {
        meta.name().starts_with(DEFAULT_EVENT)
    }
}

/// Event recorder with `DefaultEventVisitor` as its recorder and `DefaultEvent` as its output.
///
/// # Type Parameters
/// - `T`: The directive type that checks if the recorder is enabled based on the current metadata.
///   If not provided, `DefaultDirective` is used by default.
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

    type Record = DefaultEvent;

    /// Records the fields in the event and returns recorded fields as `DefaultEvent`.
    /// Note: Timestamp is not recorded.
    #[inline]
    fn record(event: &Event) -> Self::Record {
        let mut record = Self::Record::default();

        let mut visitor = DefaultEventVisitor::new(&mut record.source, &mut record.message);

        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        record.level = level_to_byte(*event.metadata().level());
        record.target.push_str(event.metadata().target());

        record
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
