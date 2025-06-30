use std::future::Future;

use tracing::Event;

use tracing_core::Metadata;

/// Trait representing a recorder directive.
///
/// Directive is a boolean condition that checks if the recorder is enabled based on the metadata.
pub trait RecorderDirective {
    /// Checks if the recorder is enabled based on the current metadata.
    /// This check can be **very expensive** and its result shall be cached by layers or filters or
    /// subscribers to avoid repeated calls for the same metadata.
    fn enabled(meta: &'_ Metadata<'_>) -> bool;
}

/// Trait for recording and transforming tracing events into an exportable format.
///
/// Types implementing this trait play three roles:
/// - `Directive`: Checks if the recorder is enabled for the event metadata.
/// - `Formatter`: Transforms event fields into the export type, if needed.
///
/// All roles operate according to an expected event structure, with varying strictness.
/// For example, `Directive` may only check for general compliance markers in metadata,
/// while formatting may record and transform only a subset of fields.
///
/// **Guidelines:**
/// - Avoid complex configurations; each implementation should focus on a single recording strategy.
/// - `Directive` is intended as a lightweight condition for use in `Layer<S>` or `Filter<S>`.
/// - Callers should check if the directive is enabled before recording.
/// - Recording events without considering their fields is discouraged and wasteful.
/// - `Directive` should be aware of the event fields.
///
/// # Associated Types
/// - `Directive`: Implements [`RecorderDirective`] and determines if recording is enabled for an event.
/// - `Record`: The final, formatted event data type.
pub trait EventRecorder {
    /// The directive type used to determine if the recorder is enabled for a given event.
    type Directive: RecorderDirective;

    /// The type representing the exported, formatted event data.
    type Record;

    /// Records the given event, transforming its fields and returning the formatted export.
    ///
    /// # Arguments
    /// * `event` - The event to record.
    ///
    /// # Returns
    /// The formatted event data as `Self::Record`.
    fn record(event: &Event) -> Self::Record;
}

/// Trait for implementing event writers.
///
/// Options and complex configurations are strongly discouraged.
/// Each implementation should focus on one way of writing to files.
pub trait EventWriter {
    /// The context provided by the supervisory body. It can simply be `self`.
    type Context;

    /// The buffer type the writer can process.
    type WriteBuffer;

    /// Writes the buffer to the file at the specified path.
    /// Returns a future that resolves to the result of the write operation.
    fn write(
        ctx: &Self::Context,
        buffer: &Self::WriteBuffer,
    ) -> impl Future<Output = tokio::io::Result<()>> + Send;
}
