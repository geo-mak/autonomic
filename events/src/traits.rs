use std::future::Future;

use tracing::{Event, field::Visit};

use tracing_core::Metadata;

/// A trait for types that can be partially reset to a reusable state.
///
/// Implementors of this trait must provide a method to reset only the relevant or
/// mutable parts of the instance, without necessarily restoring it to a pristine, initial state.
///
/// This is useful for reusing instances efficiently, such as in pools or performance-critical code,
/// where full reinitialization or resource deallocation is unnecessary or undesirable.
pub trait PartialReset {
    /// Resets a subset of the state of the implementor to its initial values.
    /// This method allows for partial reinitialization of an instance without requiring a full reset.
    ///
    /// The exact behavior depends on the implementor, and it is up to each type to define
    /// which parts of its state are affected.
    fn partial_reset(&mut self);
}

/// A trait for types that provide access to a thread-local instance of themselves.
///
/// This trait enables safe, mutable access to a thread-local instance of the implementing type
/// by passing a closure to the `thread_instance` method. The closure receives a mutable reference
/// to the instance, allowing modifications within the current thread's context.
///
/// This is useful for scenarios where per-thread state is required, such as accumulating
/// event data or maintaining reusable buffers without synchronization overhead.
pub trait ThreadLocalInstance {
    /// Provides access to a thread-local instance of the type, allowing a closure to operate on it.
    ///
    /// # Note
    ///
    /// The thread-local instance will retain any modifications made by the closure across subsequent calls
    /// within the same thread. It is not automatically reset between uses.
    ///
    /// # Arguments
    ///
    /// * `f` - A closure that takes a mutable reference to the thread-local instance and returns a value of type `R`.
    ///
    /// # Returns
    ///
    /// Returns the result of the closure `f`.
    fn thread_instance<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Self) -> R;
}

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
/// - `Schema`: Accumulates event fields via the [`Visit`] trait.
/// - `Formatter`: Transforms accumulated fields into the export type.
///
/// All roles operate according to an expected event structure, with varying strictness.
/// For example, `Directive` may only check for general compliance markers in metadata,
/// while `Schema` and formatting may record and transform only a subset of fields.
///
/// **Guidelines:**
/// - Avoid complex configurations; each implementation should focus on a single recording strategy.
/// - `Directive` is intended as a lightweight condition for use in `Layer<S>` or `Filter<S>`.
/// - Callers should check if the directive is enabled before recording.
/// - Recording events without considering their fields is discouraged and wasteful.
/// - `Directive` should be aware of the schema and its fields.
///
/// # Associated Types
/// - `Directive`: Implements [`RecorderDirective`] and determines if recording is enabled for an event.
/// - `Schema`: Implements [`Visit`], [`Default`], [`PartialReset`], [`ThreadLocalInstance`], and [`Clone`]; accumulates event data.
/// - `Export`: The final, formatted event data type.
pub trait EventRecorder {
    /// The directive type used to determine if the recorder is enabled for a given event.
    type Directive: RecorderDirective;

    /// The schema type used to accumulate and transform event fields.
    type Schema: Visit + Default + PartialReset + ThreadLocalInstance + Clone;

    /// The type representing the exported, formatted event data.
    /// This can be `()` if not required.
    type Export;

    /// Records the given event, transforming its fields and returning the formatted export.
    ///
    /// # Arguments
    /// * `event` - The event to record.
    /// * `schema` - A mutable reference to the schema for accumulating event data.
    ///
    /// # Returns
    /// The formatted event data as `Self::Export`.
    fn record(event: &Event, schema: &mut Self::Schema) -> Self::Export;
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

/// Trait representing the file store format.
/// This trait serves as an association between recorders and writers
/// to make them aware if each other for the highest possible efficiency.
///
/// Options and complex configurations are strongly discouraged.
/// Each implementation should focus on one way of recoding and writing.
pub trait FileStoreFormat: EventRecorder + EventWriter {}
