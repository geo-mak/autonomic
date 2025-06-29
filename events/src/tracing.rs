/// General event tracing macro.
///
/// # Fields
/// - `level`: tracing::Level
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
///
/// > **Note**: Publishing is not available for `TRACE` and `DEBUG` levels.
#[macro_export]
macro_rules! trace_event {
    // Case for TRACE level (no published)
    (tracing::Level::TRACE, source = $source:expr, message = $message:expr, published) => {
        compile_error!("The 'published' field cannot be used with TRACE level.");
    };
    // Case for DEBUG level (no published)
    (tracing::Level::DEBUG, source = $source:expr, message = $message:expr, published) => {
        compile_error!("The 'published' field cannot be used with DEBUG level.");
    };
    // Case for other levels without published
    // DE: Default Event
    ($level:expr, source = $source:expr, message = $message:expr) => {
        tracing::event!(name: "DE", $level, source = $source, message = $message);
    };
    // Case for other levels with published as a marker
    // DEP: Default Event Published
    ($level:expr, source = $source:expr, message = $message:expr, published) => {
        tracing::event!(name: "DEP", $level, source = $source, message = $message);
    };
    // Catch-all pattern for invalid usage
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_event!(level, source = ..., message = ..., published [optional]);"
        );
    };
}

/// Traces event with `TRACE` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
///
/// > **Note**: Publishing is not available for this event.
#[macro_export]
macro_rules! trace_trace {
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::TRACE, source = $source, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_trace!(source = ..., message = ...);");
    };
}

/// Traces event with `DEBUG` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
///
/// > **Note**: Publishing is not available for this event.
#[macro_export]
macro_rules! trace_debug {
    (source = $source:expr, message = $message:expr) => {
        // Not published
        $crate::trace_event!(tracing::Level::DEBUG, source = $source, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_debug!(source = ..., message = ...);");
    };
}

/// Traces event with `INFO` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_info {
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::INFO, source = $source, message = $message)
    };
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::INFO,
            source = $source,
            message = $message,
            published
        );
    };
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_info!(source = ..., message = ..., published [optional]]);"
        );
    };
}

/// Traces event with `WARN` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_warn {
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::WARN, source = $source, message = $message)
    };
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::WARN,
            source = $source,
            message = $message,
            published
        )
    };
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_warn!(source = ..., message = ..., published [optional]);"
        );
    };
}

/// Traces event with `ERROR` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug`.
/// - `message`: preferred `&str`, `String` or any `Debug`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_error {
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::ERROR, source = $source, message = $message)
    };
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::ERROR,
            source = $source,
            message = $message,
            published
        )
    };
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_error!(source = ..., message = ..., published [optional]);"
        );
    };
}

/// A macro to create a thread-local instance of a type that implements `Default`.
///
/// This macro defines a thread-local static variable with the given name and type,
/// wrapping the value in a `RefCell` to allow for interior mutability within each thread.
/// The value is initialized using the type's `Default` implementation.
///
/// # Arguments
///
/// * `$name` - The unique identifier for the thread-local variable.
/// * `$ty` - The type of the thread-local variable. Must implement `Default`.
#[macro_export]
macro_rules! thread_local_instance {
    ($name:ident, $ty:ty) => {
        use std::cell::RefCell;
        thread_local! {
            static $name: RefCell<$ty> = RefCell::new(<$ty>::default());
        }
    };
}
