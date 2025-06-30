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
