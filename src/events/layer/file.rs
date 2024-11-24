use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Notify};

use tracing::{Event, Subscriber};

use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::events::record::{DefaultEvent, DefaultRecorder};
use crate::events::traits::{EventRecorder, FileExtension, FileWriter};
use crate::trace_error;

/// File writer for CSV format.
/// It writes events to a CSV file in a row format.
/// Each event is serialized as a row with columns separated by commas.
pub struct CSVFormat;

impl FileExtension for CSVFormat {
    fn extension() -> &'static str {
        "csv"
    }
}

impl FileWriter for CSVFormat {
    type BufferType = DefaultEvent;

    /// Writes the buffer to the file in CSV format.
    ///
    /// > **Note**: No internal strategy for handling errors when writing.
    /// > It only returns an error when writing fails.
    ///
    /// # Parameters
    /// - `buffer`: The buffer containing events to write.
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
        // Prepare CSV header if the file is new or empty
        if file.metadata().await?.len() == 0 {
            let header: &[u8; 38] = b"Level,Source,Message,Target,Timestamp\n";
            file.write_all(header).await?;
        }
        // Convert each entry in the buffer to a CSV string
        for entry in buffer {
            let csv_row = format!(
                "{},{},{},{},{}\n",
                entry.level(),
                entry.source(),
                entry.message(),
                entry.target(),
                entry.timestamp().to_rfc3339(),
            );

            // Write the CSV row to the file
            file.write_all(csv_row.as_bytes()).await?;
        }
        Ok(())
    }
}

/// File writer for JSON format.
/// It writes events to a JSON file in an array format.
/// Each event is serialized as a JSON object.
pub struct JSONFormat;

impl FileExtension for JSONFormat {
    fn extension() -> &'static str {
        "json"
    }
}

impl FileWriter for JSONFormat {
    type BufferType = DefaultEvent;

    /// Writes the buffer to the file in JSON format.
    ///
    /// > **Note**: No internal strategy for handling errors when writing.
    /// > It only returns an error when writing fails.
    ///
    /// # Parameters
    /// - `buffer`: The buffer containing events to write.
    /// - `file_path`: The path to the file to write the events. Extension is expected to be part of the file name.
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
        // Check if the file is new or empty
        if file.metadata().await?.len() == 0 {
            // If the file is empty, write the opening bracket
            file.write_all(b"[\n").await?;
        } else {
            // If not empty, we need to remove the last character if it's a closing bracket
            let mut buf: Vec<u8> = vec![0; 1]; // Buffer to read the last character
            file.seek(tokio::io::SeekFrom::End(-1)).await?; // Move to the last character
            file.read_exact(&mut buf).await?; // Read it

            // If the last character is "]", we need to remove it
            if buf[0] == b']' {
                // Move the file cursor back one byte to overwrite it
                file.seek(tokio::io::SeekFrom::End(-1)).await?;
                file.write_all(b",").await?; // Write a comma
            }
        }
        // Write entry in the buffer
        for (i, entry) in buffer.iter().enumerate() {
            // Serialize entry to bytes
            let entry_bytes: Vec<u8> = serde_json::to_vec_pretty(entry)
                .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e))?;

            // Write entry's bytes
            file.write_all(&entry_bytes).await?;

            // Append a comma after each entry except the last one
            if i < buffer.len() - 1 {
                file.write_all(b",\n").await?;
            }
        }
        // Add the closing bracket
        file.write_all(b"\n]").await?;
        Ok(())
    }
}

struct StoreData<T> {
    // Async `Mutex` is required because there is no guarantee that
    // the locking-time could be near-instant.
    buffer: Mutex<Vec<T>>,
    threshold: u16,
    write_alert: Notify,
    guard: AtomicBool,
}

/// File storage layer for tracing subscribers with memory buffer.
/// It stores events in in-memory buffer and writes them to file in intervals.
///
/// > **Note**: It writes all events to single file named events.{extension},
/// but this might be changed in the future with more writing options.
///
/// # Generic Parameters
/// - `S`: The subscriber type to serve as layer.
/// - `F`: The file format like CSVFormat and JSONFormat.
pub struct BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter,
{
    _subscriber: PhantomData<S>,
    _format: PhantomData<F>,
    data: Arc<StoreData<F::BufferType>>,
}

impl<S, F> BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter,
{
    /// Creates new BufferedFileStore.
    ///
    /// # Parameters
    /// - `directory`: The directory where events files shall be stored.
    /// - `write_interval`: The waiting period before writing events to file.
    /// - `threshold`: The number of events to hold in the buffer before flushing to file, regardless of the interval.
    /// - `allocate`: If true, the buffer will pre-allocate memory for `threshold` events.
    pub fn new(
        mut directory: PathBuf,
        write_interval: Duration,
        threshold: u16,
        allocate: bool,
    ) -> Self
    where
        <F as FileWriter>::BufferType: Send + 'static,
    {
        // Shared data
        let data = Arc::new(StoreData {
            buffer: Mutex::new(if allocate {
                Vec::with_capacity(threshold as usize)
            } else {
                Vec::new()
            }),
            guard: AtomicBool::new(false),
            write_alert: Notify::new(),
            threshold,
        });
        // Add the file name to the directory
        directory.push("events");
        directory.set_extension(F::extension()); // Compute once
        // Start the writer task
        Self::write(directory, write_interval, data.clone());
        // The new instance
        Self {
            _subscriber: PhantomData,
            _format: PhantomData,
            data,
        }
    }

    fn write(directory: PathBuf, write_interval: Duration, data: Arc<StoreData<F::BufferType>>)
    where
        <F as FileWriter>::BufferType: Send + 'static,
    {
        tokio::spawn(async move {
            loop {
                if data.guard.load(Ordering::Acquire) {
                    // This task is the last waiter to acquire the lock after the guard has been activated
                    let mut buffer = data.buffer.lock().await;
                    // TODO: No strategy for dealing with the current data in the buffer.
                    // garbage collection
                    buffer.clear();
                    buffer.shrink_to(0);
                    return; // Stop writing
                }
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
                match F::write(&buffer, &directory).await {
                    Ok(_) => {
                        buffer.clear(); // Events are written, clear the buffer
                    }
                    // TODO: No strategy for handling errors when writing.
                    Err(err) => {
                        /*
                        Since there is no strategy for handling errors when writing,
                        we just stop writing and prevent further recordings, but
                        tasks that are currently waiting to acquire the lock have passed the guard check already,
                        so they will write the buffer.
                        We allow one more iteration, to make sure this task is the last one acquiring the lock.
                        */
                        data.guard.store(true, Ordering::Release);
                        // Propagate the event for other subscribers
                        trace_error!(
                            source = "BufferedFileStore",
                            message = format!("Stopped: {}", err)
                        );
                    }
                };
            }
        });
    }
}

impl<S, F> Layer<S> for BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter<BufferType = DefaultEvent> + 'static,
{
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        // Active guard means no writing, so we skip recording to prevent memory overflow
        if self.data.guard.load(Ordering::Acquire) {
            return;
        }
        if let Some(schema) = DefaultRecorder::record(event) {
            let data_ref = self.data.clone();
            tokio::spawn(async move {
                // Wait to acquire the lock
                let mut buffer = data_ref.buffer.lock().await;
                // During waiting to acquire the lock, the guard might have been activated,
                // but we let it write without rechecking anyway. Only new recordings will be skipped.
                buffer.push(schema);
                // This check must be done only after acquiring the lock and after pushing the event
                if buffer.len() as u16 >= data_ref.threshold {
                    // If the writer task is sleeping, it will wake up waiting for the lock to write the buffer,
                    // otherwise, it will write the buffer as soon as it acquires the lock.
                    data_ref.write_alert.notify_one();
                }
            });
        }
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
            2, // Writing must be triggered after the second event is recorded
            true,
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Wait some time to ensure writing
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

        // Initialize flags to track if we found the expected log entries
        let mut info_found = false;
        let mut error_found = false;

        // Expected values (without timestamps)
        let expected_info = ("2", "event 1", "info message");
        let expected_error = ("4", "event 2", "error message");

        // Validating records ignoring Target and Timestamp
        for record in records {
            // Access the fields within the CSV record
            let entry_type = record.get(0).unwrap().trim();
            let source = record.get(1).unwrap().trim();
            let message = record.get(2).unwrap().trim();

            // Check if the entry matches expected values
            if entry_type == expected_info.0
                && source == expected_info.1
                && message == expected_info.2
            {
                info_found = true;
            }
            if entry_type == expected_error.0
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
            2, // Writing must be triggered after the second event is recorded
            true,
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Wait some time to ensure writing
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

        // Initialize flags to track if we found the expected log entries
        let mut info_found = false;
        let mut error_found = false;

        // Expected values (without timestamps)
        let expected_info = (2, "info message", "event 1");
        let expected_error = (4, "error message", "event 2");

        // Iterate over each JSON object in the array
        for entry in json_array.iter() {
            // Access the fields within the JSON object

            let entry_type = entry
                .get("level")
                .and_then(|v| v.as_u64()) // Parse as u_int
                .unwrap_or(5);

            let message = entry
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            let source = entry
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            // Check if the entries matches expected events
            if entry_type == expected_info.0
                && message == expected_info.1
                && source == expected_info.2
            {
                info_found = true;
            }

            if entry_type == expected_error.0
                && message == expected_error.1
                && source == expected_error.2
            {
                error_found = true;
            }
        }

        // We must have only 2 entries for 2 events
        assert_eq!(
            json_array.len(),
            2,
            "Expected 2 entries in the events file, but found {}",
            json_array.len()
        );

        // Assert the expected entries are found
        assert!(info_found, "INFO entry not found or schema mismatch");
        assert!(error_found, "ERROR entry not found or schema mismatch");

        // Delete the events file
        tokio::fs::remove_file(full_path)
            .await
            .expect("Failed to delete events file");
    }

    // Mock FileWriter that simulates a write error
    struct MockFileWriter;

    impl FileExtension for MockFileWriter {
        fn extension() -> &'static str {
            "mock"
        }
    }

    impl FileWriter for MockFileWriter {
        type BufferType = DefaultEvent;

        async fn write(_buffer: &[Self::BufferType], _file_path: &PathBuf) -> tokio::io::Result<()> {
            Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, "Simulated write error"))
        }
    }

    struct AtomicStoreReference {
        store: Arc<BufferedFileStore<Registry, MockFileWriter>>,
    }

    impl AtomicStoreReference {
        fn new(store: BufferedFileStore<Registry, MockFileWriter>) -> Self {
            Self {
                store: Arc::new(store),
            }
        }
    }

    impl Layer<Registry> for AtomicStoreReference
    {
        fn on_event(&self, event: &Event, _ctx: Context<Registry>) {
            self.store.on_event(event, _ctx);
        }
    }

    impl Clone for AtomicStoreReference {
        fn clone(&self) -> Self {
            Self {
                store: self.store.clone(),
            }
        }
    }

    #[tokio::test]
    async fn test_buffered_store_write_error_handling() {
        // Buffered file layer with the log file path
        let store = BufferedFileStore::<Registry, MockFileWriter>::new(
            PathBuf::from(""),
            Duration::from_secs(60), // We rely on the threshold for this test
            2, // Writing must be triggered after the second event is recorded
            true,
        );

        let store_ref = AtomicStoreReference::new(store);

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store_ref.clone());

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events to reach the threshold of 2
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Wait some time for writing to be triggered
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Recording guard must have been activated as a result of the write error
        assert!(store_ref.store.data.guard.load(Ordering::Acquire));

        // Garbage collection must have been done
        assert!(store_ref.store.data.buffer.lock().await.is_empty());
        assert_eq!(store_ref.store.data.buffer.lock().await.capacity(), 0);

        // Try to propagate another event
        trace_info!(source = "event 3", message = "info message");

        // No recording should have been done
        assert!(store_ref.store.data.buffer.lock().await.is_empty());
    }
}
