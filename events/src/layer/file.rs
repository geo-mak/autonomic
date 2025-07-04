use core::str;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use autonomic_core::thread_local_instance;
use chrono::Utc;

use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use tracing::{Event, Subscriber};

use tracing_subscriber::Layer;
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Context;

use autonomic_core::sync::Signal;

use crate::layer::filter::CallSiteFilter;
use crate::record::{CSVVisitor, DefaultDirective, JSONLVisitor, level_to_byte};
use crate::trace_error;
use crate::traits::{CachingEventRecorder, EventWriter, RecorderDirective};

#[derive(Default)]
struct BytesBufferCache {
    buffer: Vec<u8>,
}

thread_local_instance!(__TLS_BYTES_BUFFER_CACHE, BytesBufferCache);

impl BytesBufferCache {
    #[inline]
    fn thread_local<F, I>(f: F) -> I
    where
        F: FnOnce(&mut Self) -> I,
    {
        __TLS_BYTES_BUFFER_CACHE.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// Writes a bytes buffer to the specified file by appending all bytes.
async fn write_bytes_buffer(ctx: &FileContext, buffer: &[u8]) -> tokio::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ctx.path())
        .await?;

    file.write_all(buffer).await?;
    file.flush().await?;

    Ok(())
}

/// File format for recording and writing events in CSV format.
///
/// By default, it uses `DefaultDirective` for event filtering, but users can implement
/// and specify their own directive type to customize which events are recorded.
pub struct CSVFormat<T = DefaultDirective>(PhantomData<T>)
where
    T: RecorderDirective;

impl<T> CachingEventRecorder for CSVFormat<T>
where
    T: RecorderDirective,
{
    type Directive = T;

    type RecordCache = Vec<u8>;

    /// Records and transforms fields in the event and converts recorded fields to CSV record.
    fn record(event: &Event, record: &mut Self::RecordCache) {
        let _ = write!(record, "{},", level_to_byte(*event.metadata().level()));

        // TODO: Handle recording errors.
        let mut visitor = CSVVisitor::<Self::RecordCache>::new(record);

        event.record(&mut visitor);

        let _ = write!(
            record,
            "\"{}\",\"{}\"\n",
            event.metadata().target(),
            Utc::now().to_rfc3339()
        );
    }
}

impl<T> EventWriter for CSVFormat<T>
where
    T: RecorderDirective,
{
    type WriteContext = FileContext;
    type WriteBuffer = Vec<u8>;
    type WriteResult = tokio::io::Result<()>;

    /// Writes the buffer to the file in CSV format **without** header.
    async fn write(ctx: &Self::WriteContext, buffer: &Self::WriteBuffer) -> Self::WriteResult {
        write_bytes_buffer(ctx, buffer).await
    }
}

/// File format for recording and writing events as JSON objects.
///
/// By default, it uses `DefaultDirective` for event filtering, but users can implement
/// and specify their own directive type to customize which events are recorded.
pub struct JSONLFormat<T = DefaultDirective>(PhantomData<T>)
where
    T: RecorderDirective;

impl<T> CachingEventRecorder for JSONLFormat<T>
where
    T: RecorderDirective,
{
    type Directive = T;

    type RecordCache = Vec<u8>;

    /// Records and transforms fields in the event and converts recorded fields to a JSON object.
    fn record(event: &Event, record: &mut Self::RecordCache) {
        let _ = write!(
            record,
            "{{\"level\":{},",
            level_to_byte(*event.metadata().level())
        );

        // TODO: Handle recording errors.
        let mut visitor = JSONLVisitor::<Self::RecordCache>::new(record);

        event.record(&mut visitor);

        let _ = write!(
            record,
            "\"target\":\"{}\",\"timestamp\":\"{}\"}}\n",
            event.metadata().target(),
            Utc::now().to_rfc3339()
        );
    }
}

impl<T> EventWriter for JSONLFormat<T>
where
    T: RecorderDirective,
{
    type WriteContext = FileContext;
    type WriteBuffer = Vec<u8>;
    type WriteResult = tokio::io::Result<()>;

    /// Writes the buffer to the file in JSON Lines format.
    async fn write(ctx: &Self::WriteContext, buffer: &Self::WriteBuffer) -> Self::WriteResult {
        write_bytes_buffer(ctx, buffer).await
    }
}

/// This type is a wrapper around a raw pointer that implements `Send`.
struct UnsafeRef<T> {
    ptr: *mut T,
}

unsafe impl<T> Send for UnsafeRef<T> {}

impl<T> std::ops::Deref for UnsafeRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T> std::ops::DerefMut for UnsafeRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

struct SwapBuffer<T> {
    buffers: [T; 2],
    current: usize,
}

impl<T> SwapBuffer<T> {
    fn new(a: T, b: T) -> Self {
        Self {
            buffers: [a, b],
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
    const fn current(&self) -> &T {
        &self.buffers[self.current]
    }

    #[inline(always)]
    const fn current_mut(&mut self) -> &mut T {
        &mut self.buffers[self.current]
    }

    #[cfg(test)]
    const fn inactive(&self) -> &T {
        &self.buffers[self.inactive_index()]
    }

    #[inline]
    const fn inactive_mut(&mut self) -> &mut T {
        &mut self.buffers[self.inactive_index()]
    }

    /// Returns lifetime-erased reference to the inactive buffer.
    ///
    /// # Safety
    /// - The reference must not outlive the current instance.
    /// - Dereferencing must be **exclusive** to avoid a race condition.
    /// - This is only safe because the containing SwapBuffer is owned by Arc,
    ///   ensuring stable memory location.
    /// - The caller must ensure no other code accesses the inactive buffer
    ///   until this reference is dropped.
    #[inline(always)]
    unsafe fn leak_inactive_ref(&mut self) -> UnsafeRef<T> {
        let ptr = &mut self.buffers[self.inactive_index()] as *mut T;
        UnsafeRef { ptr }
    }
}

pub struct FileContext {
    path: PathBuf,
    // ADDENDUM
}

impl FileContext {
    #[inline(always)]
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self { path: path.into() }
    }

    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The shared data of the file store.
struct SharedState<T> {
    buffer: Mutex<SwapBuffer<T>>,
    limit: usize,
    enabled: AtomicBool,
    sig_write: Signal,
}

/// Executes memory protection logic, when the writing task is no longer running.
struct OnExit<'a> {
    buffer: &'a SharedState<Vec<u8>>,
}

impl<'a> OnExit<'a> {
    #[inline(always)]
    const fn set(state: &'a SharedState<Vec<u8>>) -> Self {
        Self { buffer: state }
    }
}

impl<'a> Drop for OnExit<'a> {
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
/// - `Limit`: The size of the buffer to trigger writing, regardless of the interval.
/// - `Recording Guard`: Prevents new recordings when the writer task stops due to an error.
///
/// > **Note**: It writes all events to single file named {prefix}.{extension},
/// > but this might be changed in the future with more writing options.
///
/// # Type Parameters
/// - `S`: The tracing subscriber that accepts `Filtered` types as layers.
/// - `F`: The format type to record and write events.
///
/// Events are recorded into an allocated record cache, which is flushed after each event is recorded.
/// The recording implementation should not be concerned with the state or clearing of the buffer,
/// as this is handled by the store logic.
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
    F: CachingEventRecorder + EventWriter,
{
    state: Arc<SharedState<F::WriteBuffer>>,
    _s: PhantomData<S>,
    _f: PhantomData<F>,
}

impl<S, F> EventsFileStore<S, F>
where
    S: Subscriber,
    F: CachingEventRecorder<RecordCache = Vec<u8>>
        + EventWriter<
            WriteContext = FileContext,
            WriteBuffer = Vec<u8>,
            WriteResult = tokio::io::Result<()>,
        > + 'static,
{
    /// Creates a new event store backed by a file.
    ///
    /// # Parameters
    /// - `interval`: The waiting period before writing events to the file.
    /// - `limit`: The allowed size of the buffer in bytes before forcing a write.
    /// - `dir`: The directory where event files will be stored.
    /// - `prefix`: The prefix for the event file name.
    /// - `ext`: The file extension for the event file.
    ///
    /// # Returns
    /// Returns the store fused with its filter as a `Filtered` layer.
    ///
    /// # Notes
    /// - The `limit` parameter is a soft limit. The buffer may be allowed to exceed this limit
    ///   and expand to maintain high throughput.
    /// - Graceful shutdown is not implemented. The current implementation assumes the exit will
    ///   be either a forced abort due to panic or write error incidences.
    pub fn new(
        interval: Duration,
        limit: u32,
        dir: PathBuf,
        prefix: &str,
        ext: &str,
    ) -> Filtered<EventsFileStore<S, F>, CallSiteFilter<S, F::Directive>, S> {
        let state = Arc::new(SharedState {
            buffer: Mutex::new(SwapBuffer::new(
                Vec::with_capacity(limit as usize),
                Vec::with_capacity(limit as usize),
            )),
            limit: limit as usize,
            enabled: AtomicBool::new(true),
            sig_write: Signal::new(),
        });

        let instance = Self {
            state: state.clone(),
            _s: PhantomData,
            _f: PhantomData,
        };

        // TODO: Add file options.
        let ctx = FileContext {
            path: dir.join(format!("{prefix}.{ext}")),
        };

        Self::start_writer(state, interval, ctx);

        instance.with_filter(CallSiteFilter::new())
    }

    // TODO: Graceful shutdown is not implemented.
    // The current implementation assumes the exit to be either forced abort because of panic,
    // or write error incidences.
    fn start_writer(state: Arc<SharedState<F::WriteBuffer>>, interval: Duration, ctx: FileContext) {
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
                //
                // The safety is upheld by these invariants:
                //
                // 1. Atomicity: The `Mutex` ensures that swapping the buffers and creating a raw pointer
                //    to the inactive buffer is an atomic operation.
                //
                // 2. Exclusivity: After the swap, the writer task has exclusive access to the data in the
                //    inactive buffer via `UnsafeRef`. The `on_event` function only ever accesses
                //    the *active* buffer. The reference to the inactive buffer (`inactive_ref`) is always
                //    dropped before the next swap occurs in the loop. There is no possibility of a data race.
                //
                // 3. Validity: The raw pointer is valid for the duration of its use because:
                //    - The `SharedState` is wrapped in an `Arc`, ensuring it (and the buffers it owns)
                //      lives as long as this task and has a stable memory location.
                //    - The `UnsafeRef` is a local variable and does not outlive the data it points to.
                let mut inactive_ref: UnsafeRef<F::WriteBuffer> = {
                    // If poisoned the task will panic and "OnExit" will run.
                    let mut protected_buffer = state.buffer.lock().unwrap();

                    if protected_buffer.current().is_empty() {
                        continue;
                    }

                    protected_buffer.swap();

                    // A `leaky` reference to the inactive buffer.
                    unsafe { protected_buffer.leak_inactive_ref() }
                    // Lock released here.
                };

                if let Err(e) = F::write(&ctx, &inactive_ref).await {
                    trace_error!(message = format!("EventsFileStore stopped: {}", e));
                    return; // <- Exit here.
                }

                (*inactive_ref).clear();
            }
        });
    }
}

impl<S, F> Layer<S> for EventsFileStore<S, F>
where
    S: Subscriber,
    F: CachingEventRecorder<RecordCache = Vec<u8>> + EventWriter<WriteBuffer = Vec<u8>> + 'static,
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
        BytesBufferCache::thread_local(|record| {
            F::record(event, &mut record.buffer);

            if let Ok(mut protected_buffer) = self.state.buffer.lock() {
                let buffer = protected_buffer.current_mut();

                // Bitwise non-overlapping copy.
                // `record.buffer` is cleared by `append`.
                buffer.append(&mut record.buffer);

                if buffer.len() >= self.state.limit {
                    self.state.sig_write.notify();
                }
            }
        });
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

    use crate::trace_info;

    #[tokio::test]
    async fn test_events_file_store_write_interval() {
        let store = EventsFileStore::<Registry, JSONLFormat>::new(
            Duration::from_millis(50),
            1000,
            PathBuf::from(""),
            "interval_events",
            "jsonl",
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 0);
        assert_eq!(buffer_lock.inactive_index(), 1);
        assert!(buffer_lock.current().is_empty());
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);
        let _guard = subscriber::set_default(subscriber);

        trace_info!(message = "first event");
        trace_info!(message = "second event");

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 0);
        assert_eq!(buffer_lock.inactive_index(), 1);
        // Not.
        assert!(!buffer_lock.current().is_empty());
        // Is.
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        tokio::time::sleep(Duration::from_millis(70)).await;

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 1);
        assert_eq!(buffer_lock.inactive_index(), 0);
        assert!(buffer_lock.current().is_empty());
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/interval_events.jsonl", cargo_manifest_dir);

        let file_content = tokio::fs::read_to_string(&full_path)
            .await
            .expect("Failed to read JSON Lines file");

        let json_objects: Vec<Value> = file_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("Failed to parse JSON line"))
            .collect();

        // Should have at least the first event written by interval
        assert!(json_objects.len() == 2, "File should contain 2 events");

        // Spring time.
        assert!(store_data.enabled.load(Ordering::Acquire));

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    #[tokio::test]
    async fn test_events_file_store_write_limit() {
        let store = EventsFileStore::<Registry, JSONLFormat>::new(
            // Long interval to ensure size limit triggers write.
            Duration::from_secs(60),
            300,
            PathBuf::from(""),
            "size_limit_events",
            "jsonl",
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 0);
        assert_eq!(buffer_lock.inactive_index(), 1);
        assert!(buffer_lock.current().is_empty());
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);
        let _guard = subscriber::set_default(subscriber);

        trace_info!(message = format!("event number 1"));
        trace_info!(message = format!("event number 2"));

        tokio::time::sleep(Duration::from_millis(10)).await;

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 0);
        assert_eq!(buffer_lock.inactive_index(), 1);
        // Not.
        assert!(!buffer_lock.current().is_empty());
        // Is.
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        trace_info!(message = "event number 3");

        tokio::time::sleep(Duration::from_millis(10)).await;

        let buffer_lock = store_data.buffer.lock().unwrap();
        assert_eq!(buffer_lock.current, 1);
        assert_eq!(buffer_lock.inactive_index(), 0);
        assert!(buffer_lock.current().is_empty());
        assert!(buffer_lock.inactive().is_empty());
        drop(buffer_lock);

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/size_limit_events.jsonl", cargo_manifest_dir);

        let file_content = tokio::fs::read_to_string(&full_path)
            .await
            .expect("Failed to read JSON Lines file");

        let json_objects: Vec<Value> = file_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("Failed to parse JSON line"))
            .collect();

        assert!(json_objects.len() == 3, "File should contain 3 events");

        // Spring time.
        assert!(store_data.enabled.load(Ordering::Acquire));

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    // Mock file format that simulates a write error.
    struct MockFileFormat<const PANIC: bool>;

    impl<const PANIC: bool> EventWriter for MockFileFormat<PANIC> {
        type WriteContext = FileContext;
        type WriteBuffer = Vec<u8>;
        type WriteResult = tokio::io::Result<()>;

        async fn write(
            _ctx: &Self::WriteContext,
            _buffer: &Self::WriteBuffer,
        ) -> Self::WriteResult {
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

    impl<const PANIC: bool> CachingEventRecorder for MockFileFormat<PANIC> {
        type Directive = DefaultDirective;

        type RecordCache = Vec<u8>;

        fn record(_event: &Event, record: &mut Self::RecordCache) {
            record.push(1_u8);
        }
    }

    async fn test_events_file_store_error<const PANIC: bool>() {
        let store = EventsFileStore::<Registry, MockFileFormat<PANIC>>::new(
            Duration::from_secs(60),
            1,
            PathBuf::from(""),
            "events",
            "mock",
        );

        let store_data = store.inner().state.clone();

        let buffer_lock = store_data.buffer.lock().unwrap();
        debug_assert_eq!(buffer_lock.inactive_index(), 1);
        drop(buffer_lock);

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        // Propagate event to trigger flushing the buffer.
        trace_info!(message = "info message");

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
        trace_info!(message = "info message");

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

    struct FilterFreeDirective;

    impl RecorderDirective for FilterFreeDirective {
        fn enabled(_meta: &tracing::Metadata<'_>) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_events_store_io_csv() {
        let store = EventsFileStore::<Registry, CSVFormat<FilterFreeDirective>>::new(
            Duration::from_secs(60),
            195,
            PathBuf::from(""),
            "schemaless_events",
            "csv",
        );

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        trace_info!(message = "happy info message");

        // Schemaless.
        tracing::info!(message = "info message", extra_field = "some extra data",);

        tokio::time::sleep(Duration::from_millis(10)).await;

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/schemaless_events.csv", cargo_manifest_dir);

        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true) // <- Read heterogeneous lines.
            .from_path(&full_path)
            .expect("Failed to open CSV file");

        let records = reader.records().map(|r| r.unwrap()).collect::<Vec<_>>();

        assert_eq!(
            records.len(),
            2,
            "Expected 2 entries in the events file, but found {}",
            records.len()
        );

        let mut normal_found = false;
        let mut schemaless_found = false;

        for record in records {
            let level = record.get(0).unwrap().trim();
            let message = record.get(1).unwrap().trim();

            if level == "2" && message == "happy info message" {
                normal_found = true;
            }

            if level == "2" && message == "info message" {
                let extra_field = record.get(2).map(|s| s.trim()).unwrap_or("");
                assert_eq!(
                    extra_field, "some extra data",
                    "Extra field not recorded correctly"
                );
                schemaless_found = true;
            }
        }

        assert!(normal_found, "INFO entry not found or schema mismatch");
        assert!(schemaless_found, "INFO entry with extra field not found");

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    #[tokio::test]
    async fn test_events_store_io_json() {
        let store = EventsFileStore::<Registry, JSONLFormat<FilterFreeDirective>>::new(
            Duration::from_secs(60),
            101,
            PathBuf::from(""),
            "schemaless_events",
            "jsonl",
        );

        let subscriber = Registry::default().with(store);

        let _guard = subscriber::set_default(subscriber);

        trace_info!(message = "happy info message");

        // Schemaless.
        tracing::info!(message = "info message", extra_field = "some extra data");

        tokio::time::sleep(Duration::from_millis(10)).await;

        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/schemaless_events.jsonl", cargo_manifest_dir);

        let file_content = tokio::fs::read_to_string(&full_path)
            .await
            .expect("Failed to read JSON Lines file");

        let json_objects: Vec<Value> = file_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("Failed to parse JSON line"))
            .collect();

        assert_eq!(
            json_objects.len(),
            2,
            "Expected 2 entries in the events file, but found {}",
            json_objects.len()
        );

        let mut normal_found = false;
        let mut schemaless_found = false;

        for entry in json_objects.iter() {
            let level = entry.get("level").and_then(|v| v.as_u64()).unwrap_or(5);

            let message = entry
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            if level == 2 && message == "happy info message" {
                normal_found = true;
            }

            if level == 2 && message == "info message" {
                let extra_field = entry
                    .get("extra_field")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(
                    extra_field, "some extra data",
                    "Extra field not recorded correctly in JSON"
                );

                schemaless_found = true;
            }
        }

        assert!(normal_found, "INFO entry not found or schema mismatch");
        assert!(schemaless_found, "INFO entry with extra field not found");

        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }
}
