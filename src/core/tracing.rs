/// General event tracing macro.
///
/// # Fields
/// - `level`: tracing::Level
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
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
    // Case for other levels with published as a marker
    ($level:expr, source = $source:expr, message = $message:expr, published) => {
        tracing::event!($level, source = $source, message = $message, published = true);
    };
    // Case for other levels without published
    ($level:expr, source = $source:expr, message = $message:expr) => {
        tracing::event!($level, source = $source, message = $message);
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
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
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
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
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
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_info {
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::INFO,
            source = $source,
            message = $message,
            published
        );
    };
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::INFO, source = $source, message = $message)
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
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_warn {
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::WARN,
            source = $source,
            message = $message,
            published
        )
    };
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::WARN, source = $source, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_warn!(source = ..., message = ..., Optional: published: bool [optional]);"
        );
    };
}

/// Traces event with `ERROR` level.
///
/// # Fields
/// - `source`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `message`: preferred `&str`, `String` or any `Debug` or `Display`.
/// - `published`: an optional marker field to indicate that an event can be published by event publishers.
#[macro_export]
macro_rules! trace_error {
    (source = $source:expr, message = $message:expr, published) => {
        $crate::trace_event!(
            tracing::Level::ERROR,
            source = $source,
            message = $message,
            published
        )
    };
    (source = $source:expr, message = $message:expr) => {
        $crate::trace_event!(tracing::Level::ERROR, source = $source, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_error!(source = ..., message = ..., Optional: published: bool [optional]);"
        );
    };
}
