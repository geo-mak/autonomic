use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Guard that allows activation and deactivation without blocking the thread.
///
/// # Space-Complexity
/// `3` bytes.
pub struct AtomicGuard {
    active: AtomicBool,  // 1 Byte
    load_ord: Ordering,  // 1 Byte
    store_ord: Ordering, // 1 Byte
}

impl AtomicGuard {
    /// Creates new instance of `AtomicGuard`.
    ///
    /// # Parameters
    /// - `load_ord`: The ordering of the load operation
    /// - `store_ord`: The ordering of the store operation
    ///
    /// # Panics
    /// - if loading order is `Release` or `AcqRel`.
    /// - if storing order is `Acquire` or `AcqRel`.
    ///
    /// > **Note**: If you are unsure about the ordering, use `default()` method to create instance.
    pub fn new(load_ord: Ordering, store_ord: Ordering) -> Self {
        AtomicGuard {
            active: AtomicBool::new(false),
            load_ord,
            store_ord,
        }
    }

    /// Activates the guard.
    #[inline(always)]
    pub fn activate(&self) {
        self.active.store(true, self.store_ord);
    }

    /// Deactivates the guard.
    #[inline(always)]
    pub fn deactivate(&self) {
        self.active.store(false, self.store_ord);
    }

    /// Checks if the guard is currently active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.active.load(self.load_ord)
    }
}

impl Default for AtomicGuard {
    /// Create new `AtomicGuard` with `SeqCst` ordering for both `load` and `store` operations.
    fn default() -> Self {
        AtomicGuard::new(Ordering::SeqCst, Ordering::SeqCst)
    }
}

/// State that supports atomic operations without blocking the thread.
///
/// # Capacity
/// The **maximum** number of states that can be represented by this type is `2^8`, starting from `0` up to `255`.
///
/// # Space-Complexity
/// `3` bytes.
pub struct AtomicState {
    state: AtomicU8,     // 1 Byte
    load_ord: Ordering,  // 1 Byte
    store_ord: Ordering, // 1 Byte
}

impl AtomicState {
    /// Creates new instance of `AtomicState`.
    ///
    /// # Parameters
    /// - `load_ord`: The ordering of the load operation
    /// - `store_ord`: The ordering of the store operation
    ///
    /// # Panics
    /// - if loading order is `Release` or `AcqRel`.
    /// - if storing order is `Acquire` or `AcqRel`.
    ///
    /// > **Note**: If you are unsure about the ordering, use `default()` method to create instance.
    pub fn new(load_ord: Ordering, store_ord: Ordering, value: u8) -> Self {
        AtomicState {
            state: AtomicU8::new(value),
            load_ord,
            store_ord,
        }
    }

    /// Loads the state.
    #[inline(always)]
    pub fn load(&self) -> u8 {
        self.state.load(self.load_ord)
    }

    /// Stores the state.
    #[inline(always)]
    pub fn store(&self, value: u8) {
        self.state.store(value, self.store_ord)
    }

    /// Checks if the state is equal to the given value.
    #[inline(always)]
    pub fn is(&self, value: u8) -> bool {
        self.state.load(self.load_ord) == value
    }
}

impl Default for AtomicState {
    /// Create new `AtomicState` with `SeqCst` ordering for both `load` and `store` operations and `0` as initial value.
    fn default() -> Self {
        AtomicState {
            state: AtomicU8::default(),
            load_ord: Ordering::SeqCst,
            store_ord: Ordering::SeqCst,
        }
    }
}
