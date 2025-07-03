pub const DEFAULT_EVENT_MARKER: &str = "DE";

/// General event tracing macro.
///
/// # Fields
/// - `level`: tracing::Level
/// - `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_event {
    ($level:expr, message = $message:expr) => {
        tracing::event!(name: "DE", $level, message = $message);
    };
    // Catch-all pattern for invalid usage
    ($($arg:tt)*) => {
        compile_error!(
            "Invalid arguments. Use: trace_event!(level, message = ...);"
        );
    };
}

/// Traces event with `TRACE` level.
///
/// # Fields
/// `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_trace {
    (message = $message:expr) => {
        $crate::trace_event!(tracing::Level::TRACE, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_trace!(message = ...);");
    };
}

/// Traces event with `DEBUG` level.
///
/// # Fields
/// `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_debug {
    (message = $message:expr) => {
        $crate::trace_event!(tracing::Level::DEBUG, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_debug!(message = ...);");
    };
}

/// Traces event with `INFO` level.
///
/// # Fields
/// `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_info {
    (message = $message:expr) => {
        $crate::trace_event!(tracing::Level::INFO, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_info!(message = ...);");
    };
}

/// Traces event with `WARN` level.
///
/// # Fields
/// `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_warn {
    (message = $message:expr) => {
        $crate::trace_event!(tracing::Level::WARN, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_warn!(message = ...);");
    };
}

/// Traces event with `ERROR` level.
///
/// # Fields
/// `message`: preferred `&str`, `String` or any `Debug`.
#[macro_export]
macro_rules! trace_error {
    (message = $message:expr) => {
        $crate::trace_event!(tracing::Level::ERROR, message = $message)
    };
    ($($arg:tt)*) => {
        compile_error!("Invalid arguments. Use: trace_error!(message = ...);");
    };
}
