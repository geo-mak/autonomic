use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

use tracing::{Event, Subscriber};

use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::events::record::{DefaultEvent, DefaultRecorder};
use crate::events::traits::{EventRecorder, FileExtension, FileWriter};
use crate::trace_error;

pub struct CSVFormat;

impl FileExtension for CSVFormat {
    fn extension() -> &'static str {
        "csv"
    }
}

impl FileWriter for CSVFormat {
    type BufferType = DefaultEvent;

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

pub struct JSONFormat;

impl FileExtension for JSONFormat {
    fn extension() -> &'static str {
        "json"
    }
}

impl FileWriter for JSONFormat {
    type BufferType = DefaultEvent;

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

/// File storage layer for tracing subscribers with memory buffer.
/// It stores events in in-memory buffer and writes them to file in intervals.
///
/// > **Note**: It writes all events to single file named `events.{extension}`,
/// but this might be changed in the future with more writing options.
///
/// # Generic Parameters
/// - `S`: The subscriber type to serve as layer.
/// - `F`: The file format like `CSVFormat` and `JSONFormat`.
pub struct BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter,
{
    _subscriber: PhantomData<S>,
    _format: PhantomData<F>,
    buffer: Arc<Mutex<Vec<F::BufferType>>>,
}

impl<S, F> BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter,
{
    /// Creates new `BufferedFileStore`.
    ///
    /// # Parameters
    /// - `directory`: The directory where events files shall be stored.
    /// - `write_interval`: The waiting period before writing events to file.
    pub fn new(mut directory: PathBuf, write_interval: Duration) -> Self
    where
        <F as FileWriter>::BufferType: Send + 'static,
    {
        let instance = Self {
            _subscriber: PhantomData,
            _format: PhantomData,
            buffer: Arc::new(Mutex::new(Vec::new())),
        };

        directory.push("events");
        directory.set_extension(F::extension()); // Compute once
        let buffer_ref = instance.buffer.clone();

        // Move data and start detached task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(write_interval).await;
                let mut buffer = buffer_ref.lock().await;
                if !buffer.is_empty() {
                    // An error indicates a series I/O error
                    if let Err(err) = F::write(&buffer, &directory).await {
                        // Record the event for other subscribers and stop writing
                        trace_error!(
                            source = "BufferedFileStore",
                            message = format!("Stopped: {}", err)
                        );
                        break;
                    }
                    buffer.clear(); // Events are written, clear the buffer
                }
            }
        });

        instance
    }
}

impl<S, F> Layer<S> for BufferedFileStore<S, F>
where
    S: Subscriber,
    F: FileExtension + FileWriter<BufferType = DefaultEvent> + 'static,
{
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        if let Some(schema) = DefaultRecorder::record(event) {
            let buffer_ref = self.buffer.clone();
            tokio::spawn(async move {
                let mut entries = buffer_ref.lock().await;
                entries.push(schema);
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
            Duration::from_millis(10),
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Wait some time to ensure writing
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Full file path of the events file
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
            Duration::from_millis(10),
        );

        // Initialize events subscriber with the layer
        let subscriber = Registry::default().with(store);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(subscriber);

        // Propagate two events
        trace_info!(source = "event 1", message = "info message");
        trace_error!(source = "event 2", message = "error message");

        // Wait some time to ensure writing
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Full file path of the events file
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
}
