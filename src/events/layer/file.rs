use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Notify};

use tracing::{Event, Subscriber};

use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::events::layer::filter::CallSiteFilter;
use crate::events::record::{level_to_byte, DefaultDirective, DefaultEventVisitor};
use crate::events::traits::{EventRecorder, EventWriter, FileExtension, FileStoreFormat};
use crate::trace_error;

/// File format for recording and writing events in CSV format.
/// The output file has table-like structure with a header row.
pub struct CSVFormat;

impl FileStoreFormat for CSVFormat {}

impl EventRecorder for CSVFormat {
    type Directive = DefaultDirective;
    type FieldsVisitor = DefaultEventVisitor;
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

        // Visit and record required fields
        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        // Format as CSV record
        format!(
            "{},\"{}\",\"{}\",\"{}\",{}\n",
            level_to_byte(event.metadata().level()),
            visitor.source,
            visitor.message,
            event.metadata().target(),
            Utc::now().to_rfc3339(),
        )
        .into_bytes() // Cheap conversion, only gets its Vec<u8> internal buffer
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
    async fn write(buffer: &[Self::BufferType], file_path: &PathBuf) -> tokio::io::Result<()> {
        // Open the file for appending (or create it if it doesn't exist)
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

        // Write the CSV rows to the file
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
    type FieldsVisitor = DefaultEventVisitor;
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

        // Visit and record required fields
        event.record(&mut visitor);

        // TODO: Should events with no source or message remain allowed?
        // Format as JSON object member of an array
        format!(
            ",\n{{\n  \"level\": {},\n  \"source\": \"{}\",\n  \"message\": \"{}\",\n  \"target\": \"{}\",\n  \"timestamp\": \"{}\"\n}}",
            level_to_byte(event.metadata().level()),
            visitor.source,
            visitor.message,
            event.metadata().target(),
            Utc::now().to_rfc3339(),
        ).into_bytes() // Cheap conversion, only gets its Vec<u8> internal buffer
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
    async fn write(buffer: &[Self::BufferType], file_path: &PathBuf) -> tokio::io::Result<()> {
        // Open/Create the file in read/write mode
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
            let mut buf: Vec<u8> = vec![0; 1];
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

struct StoreData<T> {
    // Async `Mutex` is required because there is no guarantee that
    // the locking-time could be near-instant.
    buffer: Mutex<Vec<T>>,
    capacity: u32,
    write_alert: Notify,
    guard: AtomicBool,
}

/// File storage layer for tracing subscribers with memory buffer.
/// It stores events in in-memory buffer and writes them to file in intervals.
///
/// This store has the following memory-protection mechanisms to prevent buffer overflow:
/// - `Capacity`: The size of the buffer to trigger writing, regardless of the interval.
/// - `Recording Guard`: Prevents new recordings when the writer task stops due to an error.
///
/// > **Note**: It writes all events to single file named events.{extension},
/// but this might be changed in the future with more writing options.
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
pub struct BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat,
{
    _subscriber: PhantomData<S>,
    _format: PhantomData<F>,
    data: Arc<StoreData<u8>>,
}

impl<S, F> BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat<Output = Vec<u8>, BufferType = u8> + 'static,
{
    /// Creates new BufferedFileStore.
    ///
    /// # Parameters
    /// - `directory`: The directory where events files shall be stored.
    /// - `write_interval`: The waiting period before writing events to file.
    /// - `capacity`: The size of the buffer in `bytes` to trigger writing.
    /// - `allocate`: If true, the buffer will pre-allocate memory for `capacity` bytes.
    ///
    /// # Returns
    /// The store fused with its filter as `Filtered` type.
    pub fn new(
        mut directory: PathBuf,
        write_interval: Duration,
        capacity: u32,
        allocate: bool,
    ) -> Filtered<BufferedFileStore<S, F>, CallSiteFilter<S, F::Directive>, S> {
        // TODO: Should capacity remain the size of the buffer or the count of events?
        // Shared data
        let data = Arc::new(StoreData {
            buffer: Mutex::new(if allocate {
                Vec::with_capacity(capacity as usize)
            } else {
                Vec::new()
            }),
            guard: AtomicBool::new(false),
            write_alert: Notify::new(),
            capacity,
        });

        // The new instance
        let instance = Self {
            _subscriber: PhantomData,
            _format: PhantomData,
            data: data.clone(),
        };

        // Add the file name to the directory
        directory.push("events");
        directory.set_extension(F::extension()); // Compute once

        // Start the writer task
        Self::write(directory, write_interval, data);

        // Return the instance fused with the filter
        instance.with_filter(CallSiteFilter::new())
    }

    fn write(directory: PathBuf, write_interval: Duration, data: Arc<StoreData<F::BufferType>>) {
        tokio::spawn(async move {
            loop {
                // Mutex guard acquired here
                let mut buffer = tokio::select! {
                    _ = tokio::time::sleep(write_interval) =>
                    {
                        let buffer = data.buffer.lock().await;
                        if buffer.is_empty() {
                            continue;
                        };
                        buffer
                    },
                    _ = data.write_alert.notified() => {
                        // The buffer has reached the threshold
                        data.buffer.lock().await
                    }
                };

                // TODO: No strategy for handling errors when writing.
                if let Err(e) = F::write(&buffer, &directory).await {
                    // Activate the guard to prevent new recordings
                    data.guard.store(true, Ordering::Release);
                    // Note: Waiters before the activation of the guard,
                    // will be able to write to the buffer when current lock is released.
                    drop(buffer); // <-- Release the current lock or it will deadlock
                                  // TODO: No strategy for dealing with current data in the buffer.
                                  // Acquire the lock again as the last waiter this time
                    let mut buffer = data.buffer.lock().await;
                    // Do some garbage collection
                    buffer.clear();
                    buffer.shrink_to(0);
                    // Propagate the event for other subscribers
                    trace_error!(
                        source = "BufferedFileStore",
                        message = format!("Stopped: {}", e)
                    );
                    return; // Exit the task
                };

                // Clear the buffer after successful write
                buffer.clear();
            }
        });
    }

    #[inline]
    fn record(&self, mut record: F::Output) {
        let data_ref = self.data.clone();
        tokio::spawn(async move {
            let mut buffer = data_ref.buffer.lock().await;
            // During waiting to acquire the lock, the guard might have been activated,
            // but we let it write without rechecking anyway. Only new recordings will be skipped.
            buffer.append(&mut record);
            // This check must be done only after acquiring the lock and after pushing the event
            if buffer.len() >= data_ref.capacity as usize {
                // If the writer task is sleeping, it will wake up waiting for the lock to write the buffer,
                // otherwise, it will write the buffer as soon as it acquires the lock.
                data_ref.write_alert.notify_one();
            }
        });
    }
}

impl<S, F> Layer<S> for BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileStoreFormat<Output = Vec<u8>, BufferType = u8> + 'static,
{
    // > Note: Disabling event per call-site for this layer is done by the filter.
    // > It can't be done in layer using method `register_callsite`, because it will disable it
    // globally for all layers, and this is the only reason why layer is fused with the filter.

    // This method is called before `on_event` and doesn't disable recording permanently
    #[inline]
    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> bool {
        // Active guard means that writer has stopped,
        // so no recording should be done to avoid buffer overflow
        !self.data.guard.load(Ordering::Acquire)
    }

    #[inline]
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        self.record(F::record(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    use tracing::subscriber;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    use csv::ReaderBuilder;
    use serde_json::Value;

    use crate::{trace_error, trace_info};

    #[tokio::test]
    async fn test_buffered_store_write_csv() {
        // Buffered file layer with the log file path
        let store = BufferedFileStore::<Registry, CSVFormat>::new(
            PathBuf::from(""),
            Duration::from_secs(60), // We rely on the threshold for this test
            201,                     // Writing must be triggered after the second event is recorded
            true,
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Some time to ensure writing
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Full file path of the expected events file
        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/events.csv", cargo_manifest_dir);

        // Read the CSV file
        let mut reader = ReaderBuilder::new()
            .has_headers(true) // Skip the header
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

            // Check if the entry matches expected values
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

        // Delete the events file
        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    #[tokio::test]
    async fn test_buffered_store_write_json() {
        // Buffered file layer with the log file path
        let store = BufferedFileStore::<Registry, JSONFormat>::new(
            PathBuf::from(""),
            Duration::from_secs(60), // We rely on the threshold for this test
            349,                     // Writing must be triggered after the second event is recorded
            true,
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Some time to ensure writing
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Full file path of the expected events file
        let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let full_path = format!("{}/events.json", cargo_manifest_dir);

        // Read the entire JSON file as a string
        let file_content = tokio::fs::read_to_string(&full_path)
            .await
            .expect("Failed to read JSON file");

        // Parse the entire file content as a JSON array
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
            let level = entry
                .get("level")
                .and_then(|v| v.as_u64()) // Parse as u_int
                .unwrap_or(5);

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

        // Assert the expected entries are found
        assert!(info_found, "INFO entry not found or schema mismatch");
        assert!(error_found, "ERROR entry not found or schema mismatch");

        // Delete the events file
        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    // Mock file format that simulates a write error
    struct MockFileFormat;

    impl FileStoreFormat for MockFileFormat {}

    impl FileExtension for MockFileFormat {
        fn extension() -> &'static str {
            "mock"
        }
    }

    impl EventWriter for MockFileFormat {
        type BufferType = u8;

        async fn write(
            _buffer: &[Self::BufferType],
            _file_path: &PathBuf,
        ) -> tokio::io::Result<()> {
            Err(tokio::io::Error::new(
                tokio::io::ErrorKind::Other,
                "Simulated write error",
            ))
        }
    }

    impl EventRecorder for MockFileFormat {
        type Directive = DefaultDirective;
        type FieldsVisitor = DefaultEventVisitor;
        type Output = Vec<u8>;

        fn record(_event: &Event) -> Self::Output {
            vec![1_u8] // Single byte
        }
    }

    #[tokio::test]
    async fn test_buffered_store_write_error_handling() {
        // Buffered file layer with the log file path
        let store = BufferedFileStore::<Registry, MockFileFormat>::new(
            PathBuf::from(""),
            Duration::from_secs(60), // We rely on the threshold for this test
            1,                       // Writing must be triggered after the first event is recorded
            true,
        );

        let store_data = store.inner().data.clone();

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate event to trigger flushing the buffer
        trace_info!(source = "event 1", message = "info message");

        // Wait some time for writing to be triggered
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Recording guard must have been activated as a result of the write error
        assert!(store_data.guard.load(Ordering::Acquire));

        // Garbage collection must have been done
        assert!(store_data.buffer.lock().await.is_empty());
        assert_eq!(store_data.buffer.lock().await.capacity(), 0);

        // Try to propagate another event
        trace_info!(source = "event 3", message = "info message");

        // No recording should have been done
        assert!(store_data.buffer.lock().await.is_empty());
    }
}
