use std::future::Future;
use std::path::PathBuf;

use tracing::Event;

use tracing_core::field::Visit;
use tracing_core::Metadata;

/// Trait representing a recorder directive.
///
/// Directive is a boolean condition that checks if the recorder is enabled based on the metadata.
///
/// Result of the directive shall be cached by layers or filters or collectors/subscribers
/// to avoid repeated checks.
pub trait RecorderDirective {
    /// Checks if the recorder is enabled based on the current metadata.
    /// This check can be **very expensive** and its result shall be cached by layers or filters or
    /// subscribers to avoid repeated calls for the same metadata.
    fn enabled(meta: &'_ Metadata<'_>) -> bool;
}

/// Trait representing an event recorder.
///
/// Recorder is a _reusable_ recording logic that binds together components, that must be aware
/// of each other.
///
/// Any type that implements this trait plays three roles:
///
/// - `Directive`: Checks if the recorder is enabled for the metadata through `RecorderDirective`.
/// - `Visitor`: Visits and records required fields through `FieldsVisitor`.
/// - `Formatter`: Transforms fields -if necessary and returns fields as `Self::Output`.
///
/// All three roles operate according to an _expected event structure_, with more or less strictness
/// in each role.
///
/// For example, `Directive` may only check for general compliance markers in metadata,
/// while `Visitor` and `Formatter` may record and transform only subset of the fields and ignore
/// the rest.
///
/// > **Note**: Directive is intended to be used as _lightweight condition_ in `Layer<S>`
/// > or `Filter<S>` types.
/// > Callers should ensure that the directive is enabled before recording.
/// > Recording events without considering the fields is a major **waste of resources**.
/// > Directive must take into account the visitor and the fields to be recorded.
pub trait EventRecorder {
    /// The directive type that checks if the recorder is enabled based on the current metadata.
    type Directive: RecorderDirective;

    /// The visitor type that visits and records required fields.
    type FieldsVisitor: Visit;

    /// The output type after recording.
    type Output: Clone; // Clone is necessary to support some async contexts

    /// Records and transforms fields in the event and returns fields as `Self::Output`.
    fn record(event: &Event) -> Self::Output;
}

/// Trait representing an event writer.
pub trait EventWriter {
    /// The buffer type the writer can process.
    type BufferType;

    /// Writes the buffer to the file at the specified path.
    /// Returns a future that resolves to the result of the write operation.
    fn write(
        buffer: &[Self::BufferType],
        file_path: &PathBuf,
    ) -> impl Future<Output = tokio::io::Result<()>> + Send;
}

/// Trait representing the file extension.
pub trait FileExtension {
    fn extension() -> &'static str;
}

/// Trait representing the file store format
/// This trait serves as an association between recorders and writers
/// to make them aware if each other for the highest possible efficiency.
pub trait FileStoreFormat: EventRecorder + EventWriter + FileExtension {}
