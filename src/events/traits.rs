use std::future::Future;
use std::path::PathBuf;

use tracing::Event;
use tracing_core::field::Visit;

/// Trait representing an event recorder.
/// Each implementation is associated with schema,
/// that is returned if the event satisfies the requirements of the recorder.
pub trait EventRecorder {
    /// The output type after recording.
    type Schema: Clone; // Clone is necessary to support some async contexts

    /// The visitor type that visits and records required fields by the schema.
    type FieldsVisitor: Visit;

    /// Records and transforms fields in the intercepted event and returns fields as `Self::Schema`.
    ///
    /// # Returns
    /// - `Self::Schema`: If event satisfies the requirements.
    /// - `None`: If event fails to satisfy the requirements.
    fn record(event: &Event) -> Option<Self::Schema>;
}

/// Trait representing a file writer.
pub trait FileWriter {
    /// The source type that is written to the file.
    type SourceType;
    /// Writes the source to the file at the specified path.
    /// Returns a future that resolves to the result of the write operation.
    fn write(
        source: &[Self::SourceType],
        file_path: &PathBuf,
    ) -> impl Future<Output = tokio::io::Result<()>> + Send;
}

/// Trait representing the file extension.
pub trait FileExtension {
    fn extension() -> &'static str;
}
