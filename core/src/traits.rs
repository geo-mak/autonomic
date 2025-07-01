/// A trait for types that provide access to a thread-local instance of themselves.
///
/// This trait enables safe, mutable access to a thread-local instance of the implementing type
/// by passing a closure to the `thread_local` method. The closure receives a mutable reference
/// to the instance, allowing modifications within the current thread's context.
///
/// This is useful for scenarios where per-thread state is required, such as accumulating
/// event data or maintaining reusable buffers without synchronization overhead.
pub trait ThreadLocalInstance: Default {
    /// Provides access to a thread-local instance of the type, allowing a closure to operate on it.
    ///
    /// # Note
    ///
    /// The thread-local instance might retain any modifications made by the closure across subsequent calls
    /// within the same thread. It might not be automatically reset between uses.
    ///
    /// # Arguments
    ///
    /// * `f` - A closure that takes a mutable reference to the thread-local instance and returns a value of type `R`.
    ///
    /// # Returns
    ///
    /// Returns the result of the closure `f`.
    fn thread_local<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Self) -> R;
}
