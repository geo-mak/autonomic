use core::fmt::Debug;
use std::error::Error;
use std::fmt;
use std::fmt::Write;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use tracing::Event;
use tracing::Level;
use tracing::field::Field;

use tracing_core::Metadata;
use tracing_core::field::Visit;

use chrono::{DateTime, Utc};

use crate::tracing::DEFAULT_EVENT_MARKER;
use crate::traits::{EventRecorder, RecorderDirective};

/// A visitor that serializes records as JSON Lines (JSONL) into the provided buffer.
///
/// This struct writes each record as a single JSON object per line, following the JSONL format.
/// It does not enforce any schema on the data being written; the structure and validity of each
/// record is determined by the caller. As a result, the responsibility for correct reading and
/// interpretation of the output is left to the parsers that consume the JSONL data.
pub struct JSONLVisitor<'a, T: std::io::Write> {
    buffer: &'a mut T,
}

impl<'a, T: std::io::Write> JSONLVisitor<'a, T> {
    #[inline(always)]
    pub fn new(buffer: &'a mut T) -> Self {
        Self { buffer }
    }
}

// TODO: Handle errors.
impl<'a, T: std::io::Write> Visit for JSONLVisitor<'a, T> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let _ = write!(self.buffer, "\"{}\":\"{value}\",", field.name());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        let _ = write!(self.buffer, "\"{}\":\"{value}\",", field.name());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        let _ = write!(self.buffer, "\"{}\":\"{value:?}\",", field.name());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let _ = write!(self.buffer, "\"{}\":{value},", field.name());
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        let _ = write!(self.buffer, "\"{}\":\"{value:02x?}\",", field.name());
    }
}

/// A visitor that serializes records as CSV into the provided buffer.
///
/// This struct writes each record as a comma-separated line, following the CSV format.
/// No schema is enforced on the data being written; the structure and validity of each
/// record is determined by the caller. As a result, the responsibility for correct reading
/// and interpretation of the output is left to the parsers that consume the CSV data.
pub struct CSVVisitor<'a, T: std::io::Write> {
    buffer: &'a mut T,
}

impl<'a, T: std::io::Write> CSVVisitor<'a, T> {
    #[inline(always)]
    pub fn new(buffer: &'a mut T) -> Self {
        Self { buffer }
    }
}

// TODO: Handle errors.
impl<'a, T: std::io::Write> Visit for CSVVisitor<'a, T> {
    fn record_str(&mut self, _field: &Field, value: &str) {
        let _ = write!(self.buffer, "\"{value}\",");
    }

    fn record_error(&mut self, _field: &Field, value: &(dyn Error + 'static)) {
        let _ = write!(self.buffer, "\"{value}\",");
    }

    fn record_debug(&mut self, _field: &Field, value: &dyn Debug) {
        let _ = write!(self.buffer, "\"{value:?}\",");
    }
    fn record_f64(&mut self, _field: &Field, value: f64) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_i64(&mut self, _field: &Field, value: i64) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_u64(&mut self, _field: &Field, value: u64) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_i128(&mut self, _field: &Field, value: i128) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_u128(&mut self, _field: &Field, value: u128) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_bool(&mut self, _field: &Field, value: bool) {
        let _ = write!(self.buffer, "{value},");
    }

    fn record_bytes(&mut self, _field: &Field, value: &[u8]) {
        let _ = write!(self.buffer, "\"{value:02x?}\",");
    }
}

/// A visitor that concatenates all record fields into a single string buffer.
///
/// This struct writes each field value directly to the provided string buffer without any separators
/// or field names. All values are concatenated sequentially as they are visited. No schema is enforced
/// on the data being written; the structure and validity of each record is determined by the caller.
/// As a result, the responsibility for correct reading and interpretation of the output is left to
/// the consumers that process the concatenated string data.
pub struct StringVisitor<'a> {
    message: &'a mut String,
}

impl<'a> StringVisitor<'a> {
    #[inline(always)]
    pub const fn new(message: &'a mut String) -> Self {
        Self { message }
    }
}

// TODO: Handle errors.
impl<'a> Visit for StringVisitor<'a> {
    fn record_str(&mut self, _field: &Field, value: &str) {
        let _ = write!(self.message, "{value}");
    }

    fn record_error(&mut self, _field: &Field, value: &(dyn Error + 'static)) {
        let _ = write!(self.message, "{value}");
    }

    fn record_debug(&mut self, _field: &Field, value: &dyn fmt::Debug) {
        let _ = write!(self.message, "{value:?}");
    }

    fn record_f64(&mut self, _field: &Field, value: f64) {
        let _ = write!(self.message, "{value}");
    }

    fn record_i64(&mut self, _field: &Field, value: i64) {
        let _ = write!(self.message, "{value}");
    }

    fn record_u64(&mut self, _field: &Field, value: u64) {
        let _ = write!(self.message, "{value}");
    }

    fn record_i128(&mut self, _field: &Field, value: i128) {
        let _ = write!(self.message, "{value}");
    }

    fn record_u128(&mut self, _field: &Field, value: u128) {
        let _ = write!(self.message, "{value}");
    }

    fn record_bool(&mut self, _field: &Field, value: bool) {
        let _ = write!(self.message, "{value}");
    }

    fn record_bytes(&mut self, _field: &Field, value: &[u8]) {
        let _ = write!(self.message, "{value:02x?}");
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
    pub(crate) message: String,
    pub(crate) target: String,
    pub(crate) timestamp: DateTime<Utc>,
}

impl DefaultEvent {
    pub const fn new(
        level: Level,
        message: String,
        target: String,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            level: level_to_byte(level),
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
/// - `message`: An empty `String`.
/// - `target`: An empty `String`.
/// - `timestamp`: Set to the current UTC time at the moment of creation.
impl Default for DefaultEvent {
    #[inline]
    fn default() -> Self {
        Self {
            level: 0,
            message: String::new(),
            target: String::new(),
            timestamp: Utc::now(),
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
        meta.name() == DEFAULT_EVENT_MARKER
    }
}

/// Event recorder with `StringVisitor` as its recorder and `DefaultEvent` as its output.
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

        // TODO: Handle recording errors.
        let mut visitor = StringVisitor::new(&mut record.message);

        event.record(&mut visitor);

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
            "message".to_string(),
            "path".to_string(),
            Utc::now(),
        );

        let serialized =
            serde_json::to_string(&original_schema).expect("Failed to serialize DefaultEvent");

        let deserialized_schema: DefaultEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize DefaultEvent");

        assert_eq!(original_schema.level(), deserialized_schema.level());
        assert_eq!(original_schema.message(), deserialized_schema.message());
        assert_eq!(original_schema.target(), deserialized_schema.target());
        assert_eq!(original_schema.timestamp(), deserialized_schema.timestamp());
    }
}
