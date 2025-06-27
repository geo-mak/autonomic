use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use tracing::{Event, Subscriber};

use tracing_subscriber::Layer;
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Context;

use autonomic_core::sync::Signal;

use crate::layer::filter::CallSiteFilter;
use crate::record::{DefaultDirective, DefaultEventVisitor, level_to_byte};
use crate::trace_error;
use crate::traits::{EventRecorder, EventWriter, FileExtension, FileStoreFormat};

/// File format for recording and writing events in CSV format.
/// The output file has table-like structure with a header row.
pub struct CSVFormat;

impl FileStoreFormat for CSVFormat {}

impl EventRecorder for CSVFormat {
    type Directive = DefaultDirective;

    type Output = Vec<u8>;

    /// Records and transforms fields in the event and converts recorded fields to CSV record.
    ///
    /// # Returns
    /// - `Vec<u8>`: CSV record as bytes-ready string buffer with a newline character.
    fn record(event: &Event) -> Self::Output {
        let mut visitor = DefaultEventVisitor {
            source: String::new(),
            message: String::new(),
        };

        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        format!(
            "{},\"{}\",\"{}\",\"{}\",{}\n",
            level_to_byte(*event.metadata().level()),
            visitor.source,
            visitor.message,
            event.metadata().target(),
            Utc::now().to_rfc3339(),
        )
        .into_bytes()
    }
}

impl EventWriter for CSVFormat {
    type BufferType = u8;

    /// Writes the buffer to the file in CSV format.
    ///
    /// > **Note**: No internal strategy for handling errors when writing.
    /// > It only returns an error when writing fails.
    ///
    /// # Parameters
    /// - `buffer`: The source buffer containing events to write.
    /// - `file_path`: The path to the file to write the events. Extension is expected to be part of the file name.
    ///
    /// # Returns
    /// - `Ok(())`: if the write operation is successful.
    /// - `Err(io::Error)`: if the write operation has failed.
    async fn write(buffer: &[Self::BufferType], file_path: &Path) -> tokio::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .await?;

        // Write CSV header if the file is empty
        if file.metadata().await?.len() == 0 {
            let header: &[u8; 38] = b"Level,Source,Message,Target,Timestamp\n";
            file.write_all(header).await?;
        }

        file.write_all(buffer).await?;
        Ok(())
    }
}

impl FileExtension for CSVFormat {
    fn extension() -> &'static str {
        "csv"
    }
}

/// File format for recording and writing events as JSON objects.
/// The output file has an array structure with each event as an element.
pub struct JSONFormat;

impl FileStoreFormat for JSONFormat {}

impl EventRecorder for JSONFormat {
    type Directive = DefaultDirective;

    type Output = Vec<u8>;

    /// Records and transforms fields in the event and converts recorded fields to a JSON object.
    ///
    /// # Returns
    /// - `Vec<u8>`: JSON object as bytes-ready string buffer with comma and newline characters.
    fn record(event: &Event) -> Self::Output {
        let mut visitor = DefaultEventVisitor {
            source: String::new(),
            message: String::new(),
        };

        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        format!(
            ",\n{{\n  \"level\": {},\n  \"source\": \"{}\",\n  \"message\": \"{}\",\n  \"target\": \"{}\",\n  \"timestamp\": \"{}\"\n}}",
            level_to_byte(*event.metadata().level()),
            visitor.source,
            visitor.message,
            event.metadata().target(),
            Utc::now().to_rfc3339(),
        ).into_bytes()
    }
}

impl EventWriter for JSONFormat {
    type BufferType = u8;

    /// Writes the buffer to the file in JSON format.
    ///
    /// # Parameters
    /// - `buffer`: The source buffer containing events to write.
    /// - `file_path`: The path to the file to write the events with the extension.
    ///
    /// # Returns
    /// - `Ok(())`: if the write operation is successful.
    /// - `Err(io::Error)`: if the write operation has failed.
    async fn write(buffer: &[Self::BufferType], file_path: &Path) -> tokio::io::Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(file_path)
            .await?;

        let mut buffer = buffer;

        if file.metadata().await?.len() == 0 {
            // If the file is empty, write the opening bracket
            file.write_all(b"[\n").await?;
            // Skip the first comma and newline characters
            buffer = &buffer[2..];
        } else {
            let mut buf = [0; 1];
            // Move to the last character
            file.seek(tokio::io::SeekFrom::End(-1)).await?;
            file.read_exact(&mut buf).await?;

            // If the last character is "]", it must be overwritten
            if buf[0] == b']' {
                // Move the file cursor back two bytes to overwrite it
                file.seek(tokio::io::SeekFrom::End(-2)).await?;
            }
        }

        // Write the entire buffer to the file
        file.write_all(buffer).await?;

        // Write the closing bracket
        file.write_all(b"\n]").await?;

        Ok(())
    }
}

impl FileExtension for JSONFormat {
    fn extension() -> &'static str {
        "json"
    }
}

/// This type is a wrapper around a raw pointer that implements `Send`.
struct UnsafeBufferRef<T> {
    ptr: *mut Vec<T>,
}

unsafe impl<T: Send> Send for UnsafeBufferRef<T> {}

impl<T> std::ops::Deref for UnsafeBufferRef<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T> std::ops::DerefMut for UnsafeBufferRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

struct SwapBuffer<T> {
    buffers: [Box<Vec<T>>; 2],
    current: usize,
}

impl<T> SwapBuffer<T> {
    fn new(capacity: usize) -> Self {
        Self {
            buffers: [
                Box::new(Vec::with_capacity(capacity)),
                Box::new(Vec::with_capacity(capacity)),
            ],
            current: 0,
        }
    }

    /// Changes the access flag of the current buffer.
    #[inline(always)]
    const fn swap(&mut self) {
        self.current ^= 1;
    }

    #[inline(always)]
    const fn inactive_index(&self) -> usize {
        self.current ^ 1
    }

    #[inline(always)]
    const fn current(&self) -> &Vec<T> {
        &self.buffers[self.current]
    }

    #[inline(always)]
    const fn current_mut(&mut self) -> &mut Vec<T> {
        &mut self.buffers[self.current]
    }

    #[cfg(test)]
    const fn inactive(&self) -> &Vec<T> {
        &self.buffers[self.inactive_index()]
    }

    #[inline]
    const fn inactive_mut(&mut self) -> &mut Vec<T> {
        &mut self.buffers[self.inactive_index()]
    }

    /// Returns lifetime-erased reference to the inactive buffer.
    ///
    /// # Safety
    /// - The reference must not outlive the current instance.
    /// - Dereferencing must be **exclusive** to avoid a race condition.
    #[inline(always)]
    unsafe fn leak_inactive_ref(&mut self) -> UnsafeBufferRef<T> {
        UnsafeBufferRef {
            ptr: Box::as_mut(&mut self.buffers[self.inactive_index()]) as *mut Vec<T>,
        }
    }
}

/// The shared data of the file store.
struct SharedState<T> {
    buffer: Mutex<SwapBuffer<T>>,
    limit: usize,
    sig_write: Signal,
    enabled: AtomicBool,
}

/// Executes memory protection logic, when the writing task is no longer running.
struct OnExit<'a, T> {
    buffer: &'a SharedState<T>,
}

impl<'a, T> OnExit<'a, T> {
    #[inline(always)]
    const fn set(state: &'a SharedState<T>) -> Self {
        Self { buffer: state }
    }
}

impl<'a, T> Drop for OnExit<'a, T> {
    fn drop(&mut self) {
        // Set to disabled to prevent new recording.
        self.buffer.enabled.store(false, Ordering::Release);

        // If poisoned, the data behind the guard must be assumed to be invalid.
        if let Ok(mut buffer_guard) = self.buffer.buffer.lock() {
            // Some garbage collection (currently).
            let current = buffer_guard.current_mut();
            current.clear();
            current.shrink_to_fit();

            let swap = buffer_guard.inactive_mut();
            swap.clear();
            swap.shrink_to_fit();
        }
    }
}

/// File storage layer for tracing subscribers with memory buffer.
/// It stores events in in-memory buffer and writes them to file in intervals.
///
/// This store has the following memory-protection mechanisms to prevent buffer overflow:
/// - `Capacity`: The size of the buffer to trigger writing, regardless of the interval.
/// - `Recording Guard`: Prevents new recordings when the writer task stops due to an error.
///
/// > **Note**: It writes all events to single file named events.{extension},
/// > but this might be changed in the future with more writing options.
///
/// # Type Parameters
/// - `S`: The tracing subscriber that accepts `Filtered` types as layers.
/// - `F`: The format type to record and write events.
///
/// `F` is constrained with additional constraints:
/// The `Output` of its recorder must be `Vec<u8>` and the `BufferType` of its writer must be `u8`.
/// This allows the same buffer to be used for both recording and writing, where writer can write
/// the entire buffer with single write operation.
///
/// This store is fused with `CallSiteFilter` to filter events in this layer per call-site using
/// the directive of its recorder before recording.
///
/// This fusion allows subscribers that support caching to cache the filtering result per call-site
/// to avoid repeated checks and improve performance.
///
/// Filtering results are only valid for this layer, and they don't affect other layers.
pub struct EventsFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat,
{
    state: Arc<SharedState<u8>>,
    _s: PhantomData<S>,
    _f: PhantomData<F>,
}

impl<S, F> EventsFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat<Output = Vec<u8>, BufferType = u8> + 'static,
{
    /// Creates new BufferedFileStore.
    ///
    /// # Parameters
    /// - `dir`: The directory where events files shall be stored.
    /// - `interval`: The waiting period before writing events to file.
    /// - `limit`: The allowed size of the buffer in `bytes` before forcing writing.
    ///
    /// # Returns
    /// The store fused with its filter as `Filtered` layer.
    pub fn new(
        mut dir: PathBuf,
        interval: Duration,
        limit: u32,
    ) -> Filtered<EventsFileStore<S, F>, CallSiteFilter<S, F::Directive>, S> {
        let state = Arc::new(SharedState {
            buffer: Mutex::new(SwapBuffer::new(limit as usize)),
            limit: limit as usize,
            sig_write: Signal::new(),
            enabled: AtomicBool::new(true),
        });

        let instance = Self {
            state: state.clone(),
            _s: PhantomData,
            _f: PhantomData,
        };

        // TODO: Add writing options for writing events to multiple files.
        dir.push("events");
        dir.set_extension(F::extension());

        Self::start_writer(dir, interval, state);

        instance.with_filter(CallSiteFilter::new())
    }

    // TODO: Graceful shutdown is not implemented.
    // The current implementation assumes the exit to be either forced abort because of panic,
    // or write error incidences.
    fn start_writer(
        directory: PathBuf,
        interval: Duration,
        state: Arc<SharedState<F::BufferType>>,
    ) {
        tokio::spawn(async move {
            // The referenced state should remain valid and safe to access until `OnExit` finishes.
            let _on_exit = OnExit::set(&state);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {},
                    _ = state.sig_write.notified() => {},
                };

                // SAFETY: The following logic relies on `unsafe` code to avoid holding a Mutex lock during
                // the I/O-bound write operation.
                // The safety is upheld by these invariants:
                // 1. Atomicity: The `Mutex` ensures that swapping the buffers and creating a raw pointer
                //    to the inactive buffer is an atomic operation.
                // 2. Exclusivity: After the swap, the writer task has exclusive access to the data in the
                //    inactive buffer via `UnsafeBufferRef`. The `on_event` function only ever accesses
                //    the *active* buffer. There is no possibility of a data race.
                // 3. Validity: The raw pointer is valid for the duration of its use because:
                //    - The buffer `Vec` is within a `Box`, giving it a stable memory location.
                //    - The `SharedState` is wrapped in an `Arc`, ensuring it (and the buffers it owns)
                //      lives as long as this task. The `UnsafeBufferRef` is a local variable and does
                //      not outlive the data it points to.
                let mut inactive_ref: UnsafeBufferRef<F::BufferType> = {
                    // If poisoned the task will panic and "OnExit" will run.
                    let mut buffer_lock = state.buffer.lock().unwrap();

                    if buffer_lock.current().is_empty() {
                        continue;
                    }

                    buffer_lock.swap();

                    // A `leaky` reference to the inactive buffer.
                    unsafe { buffer_lock.leak_inactive_ref() }
                    // Lock released here.
                };

                if let Err(e) = F::write(&inactive_ref, &directory).await {
                    trace_error!(
                        source = "EventsFileStore",
                        message = format!("Stopped: {}", e)
                    );
                    return;
                }

                (*inactive_ref).clear();
            }
        });
    }
}

impl<S, F> Layer<S> for EventsFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat<Output = Vec<u8>, BufferType = u8> + 'static,
{
    // > Note: Disabling event per call-site for this layer is done by the filter.
    // > It can't be done in layer using method `register_callsite`, because it will disable it
    // globally for all layers, and this is the only reason why layer is fused with the filter.

    // This method is called before `on_event` and doesn't disable recording permanently.
    #[inline]
    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> bool {
        self.state.enabled.load(Ordering::Acquire)
    }

    #[inline]
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        let mut record = F::record(event);

        if let Ok(mut buffer_guard) = self.state.buffer.lock() {
            let buffer = buffer_guard.current_mut();

            // Bitwise non-overlapping copy.
            buffer.append(&mut record);

            if buffer.len() >= self.state.limit {
                self.state.sig_write.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    use tracing::subscriber;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use csv::ReaderBuilder;
    use serde_json::Value;

    use crate::{trace_error, trace_info};

    // Mock file format that simulates a write error
    struct MockFileFormat<const PANIC: bool>;

    impl<const PANIC: bool> FileStoreFormat for MockFileFormat<PANIC> {}

    impl<const PANIC: bool> FileExtension for MockFileFormat<PANIC> {
        fn extension() -> &'static str {
            "mock"
        }
    }

    impl<const PANIC: bool> EventWriter for MockFileFormat<PANIC> {
        type BufferType = u8;

        async fn write(_buffer: &[Self::BufferType], _file_path: &Path) -> tokio::io::Result<()> {
            if PANIC {
                panic!("Writer thread has panicked")
            } else {
                Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    "Simulated write error",
                ))
            }
        }
    }

    impl<const PANIC: bool> EventRecorder for MockFileFormat<PANIC> {
        type Directive = DefaultDirective;

        type Output = Vec<u8>;

        fn record(_event: &Event) -> Self::Output {
            vec![1_u8] // Single byte
        }
    }

    async fn test_events_file_store_error<const PANIC: bool>() {
        let store = EventsFileStore::<Registry, MockFileFormat<PANIC>>::new(
            PathBuf::from(""),
            Duration::from_secs(60),
            1,
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        debug_assert_eq!(buffer_lock.inactive_index(), 1);
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        // Propagate event to trigger flushing the buffer.
        trace_info!(source = "event 1", message = "info message");

        // Wait some time for writing to be triggered.
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Recording guard must have been activated as a result of the write error.
        assert!(!store_data.enabled.load(Ordering::Acquire));

        let buffer_guard = store_data.buffer.lock().unwrap();

        debug_assert_eq!(buffer_guard.inactive_index(), 0);

        let writing_buf = buffer_guard.inactive();
        assert!(writing_buf.is_empty());
        assert_eq!(writing_buf.capacity(), 0);
        drop(buffer_guard); // <- will block recording if not dropped.

        // Try to propagate another event.
        trace_info!(source = "event 3", message = "info message");

        tokio::time::sleep(Duration::from_millis(10)).await;

        let buffer_guard = store_data.buffer.lock().unwrap();
        let recording_buf = buffer_guard.current();

        // No recording should have been done.
        assert!(recording_buf.is_empty());
        assert_eq!(recording_buf.capacity(), 0);
    }

    #[tokio::test]
    async fn test_events_file_store_on_error() {
        test_events_file_store_error::<false>().await;
    }

    #[tokio::test]
    async fn test_events_file_store_on_panic() {
        test_events_file_store_error::<true>().await;
    }

    #[tokio::test]
    async fn test_events_file_store_write_csv() {
        let store = EventsFileStore::<Registry, CSVFormat>::new(
            PathBuf::from(""),
            Duration::from_secs(60),
            198,
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        debug_assert_eq!(buffer_lock.inactive_index(), 1);
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        // Propagate two events.
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Some time to ensure writing.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/events.csv", cargo_manifest_dir);

        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .from_path(&full_path)
            .expect("Failed to open CSV file");

        let records = reader.records().map(|r| r.unwrap()).collect::<Vec<_>>();

        // We must have only 2 entries for 2 events
        assert_eq!(
            records.len(),
            2,
            "Expected 2 entries in the events file, but found {}",
            records.len()
        );

        // Flags to track if we found the expected log entries
        let mut info_found = false;
        let mut error_found = false;

        // Expected values (without timestamps)
        let expected_info = ("2", "event 1", "info message");
        let expected_error = ("4", "event 2", "error message");

        // Validating records ignoring Target and Timestamp
        for record in records {
            let level = record.get(0).unwrap().trim();
            let source = record.get(1).unwrap().trim();
            let message = record.get(2).unwrap().trim();

            if level == expected_info.0 && source == expected_info.1 && message == expected_info.2 {
                info_found = true;
            }

            if level == expected_error.0
                && source == expected_error.1
                && message == expected_error.2
            {
                error_found = true;
            }
        }

        // Assert the expected entries are found
        assert!(info_found, "INFO entry not found or schema mismatch");
        assert!(error_found, "ERROR entry not found or schema mismatch");

        // Recording must remain enabled.
        assert!(store_data.enabled.load(Ordering::Acquire));

        let buffer_guard = store_data.buffer.lock().unwrap();

        debug_assert_eq!(buffer_guard.inactive_index(), 0);

        // Both should be empty now, because the active written has been flushed,
        // and swap is not yet used, but their capacity must not be 0.
        let swap_buf = buffer_guard.inactive();
        assert!(swap_buf.is_empty());
        assert_ne!(swap_buf.capacity(), 0);

        let current_buf = buffer_guard.current();
        assert!(current_buf.is_empty());
        assert_ne!(current_buf.capacity(), 0);

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    #[tokio::test]
    async fn test_events_file_store_write_json() {
        let store = EventsFileStore::<Registry, JSONFormat>::new(
            PathBuf::from(""),
            Duration::from_secs(60),
            346,
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        debug_assert_eq!(buffer_lock.inactive_index(), 1);
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Some time to ensure writing
        tokio::time::sleep(Duration::from_millis(10)).await;

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/events.json", cargo_manifest_dir);

        let file_content = tokio::fs::read_to_string(&full_path)
            .await
            .expect("Failed to read JSON file");

        let json_array: Vec<Value> =
            serde_json::from_str(&file_content).expect("Failed to parse JSON file as an array");

        // We must have only 2 entries for 2 events
        assert_eq!(
            json_array.len(),
            2,
            "Expected 2 entries in the events file, but found {}",
            json_array.len()
        );

        // Flags to track if we found the expected log entries
        let mut info_found = false;
        let mut error_found = false;

        // Expected values (without target and timestamp)
        let expected_info = (2, "event 1", "info message");
        let expected_error = (4, "event 2", "error message");

        // Iterate over each JSON object in the array
        for entry in json_array.iter() {
            let level = entry.get("level").and_then(|v| v.as_u64()).unwrap_or(5);

            let source = entry
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            let message = entry
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            // Check if the entries matches expected events
            if level == expected_info.0 && source == expected_info.1 && message == expected_info.2 {
                info_found = true;
            }

            if level == expected_error.0
                && source == expected_error.1
                && message == expected_error.2
            {
                error_found = true;
            }
        }

        assert!(info_found, "INFO entry not found or schema mismatch");
        assert!(error_found, "ERROR entry not found or schema mismatch");

        // Recording must remain enabled.
        assert!(store_data.enabled.load(Ordering::Acquire));

        let buffer_guard = store_data.buffer.lock().unwrap();

        debug_assert_eq!(buffer_guard.inactive_index(), 0);

        // Both should be empty now, because the active written has been flushed,
        // and swap is not yet used, but their capacity must not be 0.
        let swap_buf = buffer_guard.inactive();
        assert!(swap_buf.is_empty());
        assert_ne!(swap_buf.capacity(), 0);

        let current_buf = buffer_guard.current();
        assert!(current_buf.is_empty());
        assert_ne!(current_buf.capacity(), 0);

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }
}
