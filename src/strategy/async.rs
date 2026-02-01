//! Asynchronous batch processing strategy
//!
//! This module provides an asynchronous, multi-threaded implementation of the
//! ProcessingStrategy trait. It processes transactions in batches using thread-based
//! parallelism with client-based partitioning.
//!
//! # Architecture
//!
//! ```text
//! AsyncProcessingStrategy
//!     ├── BatchConfig (batch_size, max_concurrent_batches)
//!     ├── AsyncReader (batch CSV reading)
//!     ├── BatchProcessor (client partitioning + threading)
//!     └── AsyncTransactionEngine (thread-safe processing)
//!         ├── AsyncAccountManager (thread-safe account state)
//!         └── AsyncTransactionStore (thread-safe transaction history)
//! ```
//!
//! # Thread-Based Parallelism
//!
//! This strategy uses true thread-based parallelism:
//! - Processes batches sequentially to maintain per-client ordering across entire file
//! - Within each batch, partitions by client ID for parallel processing
//! - Spawns worker threads via tokio multi-threaded runtime
//! - Maintains per-client transaction ordering both within and across batches
//! - Uses Arc + DashMap for thread-safe shared state

use crate::core::r#async::{
    AsyncAccountManager, AsyncTransactionEngine, AsyncTransactionStore, BatchProcessor,
};
use crate::io::async_reader::AsyncReader;
use crate::io::csv_format::write_accounts_csv;
use crate::strategy::ProcessingStrategy;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// Configuration for batch processing
///
/// Controls how transactions are batched and the number of worker threads
/// for parallel processing within each batch.
#[derive(Clone, Debug)]
pub struct BatchConfig {
    /// Number of transactions per batch
    pub batch_size: usize,
    /// Maximum number of batches processing concurrently
    pub max_concurrent_batches: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            max_concurrent_batches: num_cpus::get(),
        }
    }
}

impl BatchConfig {
    /// Create a new BatchConfig with custom values
    pub fn new(batch_size: usize, max_concurrent_batches: usize) -> Self {
        let default = Self::default();

        let batch_size = if batch_size == 0 {
            eprintln!(
                "Warning: Invalid batch_size ({}), using default ({})",
                batch_size, default.batch_size
            );
            default.batch_size
        } else {
            batch_size
        };

        let max_concurrent_batches = if max_concurrent_batches == 0 {
            eprintln!(
                "Warning: Invalid max_concurrent_batches ({}), using default ({})",
                max_concurrent_batches, default.max_concurrent_batches
            );
            default.max_concurrent_batches
        } else {
            max_concurrent_batches
        };

        Self {
            batch_size,
            max_concurrent_batches,
        }
    }
}

/// Asynchronous batch processing strategy
///
/// Implements the ProcessingStrategy trait using multi-threaded, asynchronous
/// batch processing. Transactions are read in batches and processed sequentially
/// (batch-by-batch) to maintain ordering guarantees. Within each batch, transactions
/// are partitioned by client ID and processed in parallel across multiple threads.
///
/// # Thread Safety
///
/// AsyncProcessingStrategy is Send + Sync and uses thread-safe components
/// internally (Arc-wrapped AsyncTransactionEngine with DashMap-based state).
///
/// # Configuration
///
/// The strategy accepts a BatchConfig with:
/// - `batch_size`: Number of transactions per batch (default: 1000)
/// - `max_concurrent_batches`: Number of worker threads (default: CPU cores)
#[derive(Debug, Clone)]
pub struct AsyncProcessingStrategy {
    /// Batch processing configuration
    config: BatchConfig,
}

impl AsyncProcessingStrategy {
    /// Create a new AsyncProcessingStrategy with the specified configuration
    ///
    /// # Arguments
    ///
    /// * `config` - BatchConfig with batch_size and max_concurrent_batches
    ///
    /// # Returns
    ///
    /// A new `AsyncProcessingStrategy` configured for batch processing
    pub fn new(config: BatchConfig) -> Self {
        Self { config }
    }
}

impl ProcessingStrategy for AsyncProcessingStrategy {
    /// Process transactions from input file and write results to output
    ///
    /// This method implements the complete asynchronous batch processing pipeline:
    /// 1. Creates thread-safe engine components (AsyncTransactionEngine, etc.)
    /// 2. Creates a BatchProcessor for client-based partitioning
    /// 3. Creates a tokio multi-threaded runtime
    /// 4. Reads transactions in batches from CSV using AsyncReader
    /// 5. Processes each batch sequentially (waits for completion before next batch)
    /// 6. Within each batch, processes different clients in parallel
    /// 7. Collects final account states
    /// 8. Writes account states to output using csv_format module
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input CSV file
    /// * `output` - Mutable reference to a writer for outputting account states
    ///
    /// # Returns
    ///
    /// * `Ok(())` if processing completed successfully
    /// * `Err(String)` if a fatal error occurred
    ///
    /// # Error Handling
    ///
    /// Fatal errors (file not found, I/O errors, runtime errors) are returned immediately.
    /// Individual transaction errors are logged to stderr and processing continues.
    fn process(&self, input_path: &Path, output: &mut dyn Write) -> Result<(), String> {
        // Create tokio runtime for async execution
        // Use multi-threaded runtime with configured number of worker threads
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.config.max_concurrent_batches)
            .build()
            .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

        // Execute async processing within the runtime
        runtime.block_on(async {
            // Create thread-safe engine components
            let account_manager = Arc::new(AsyncAccountManager::new());
            let transaction_store = Arc::new(AsyncTransactionStore::new());
            let engine = Arc::new(AsyncTransactionEngine::new(
                Arc::clone(&account_manager),
                Arc::clone(&transaction_store),
            ));

            // Create batch processor
            let processor = BatchProcessor::new(Arc::clone(&engine));

            // Open the CSV file
            let file = tokio::fs::File::open(input_path)
                .await
                .map_err(|e| format!("Failed to open file '{}': {}", input_path.display(), e))?;

            // Wrap tokio file in a compatibility layer for csv-async
            let compat_file = tokio_util::compat::TokioAsyncReadCompatExt::compat(file);

            // Create async CSV reader
            let mut reader = AsyncReader::new(compat_file);

            // Process batches sequentially to maintain per-client ordering across entire file
            // Each batch is still processed in parallel across different clients
            loop {
                // Read a batch of records using AsyncReader
                let batch = reader.read_batch(self.config.batch_size).await;

                // If batch is empty, we've reached end of file
                if batch.is_empty() {
                    break;
                }

                // Process batch and wait for completion before reading next batch
                // This ensures that if a client's transactions span multiple batches,
                // they are processed in the correct order
                let _results = processor.process_batch(batch).await;
            }

            // Get final account states
            let accounts = account_manager.get_all_accounts();

            // Write account states to output using csv_format module
            write_accounts_csv(&accounts, output)?;

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Helper function to create a temporary CSV file for testing
    fn create_temp_csv(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[test]
    fn test_async_strategy_processes_valid_deposit() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let file = create_temp_csv(csv_content);

        let config = BatchConfig::default();
        let strategy = AsyncProcessingStrategy::new(config);
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Verify output contains account data
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("client"));
        assert!(output_str.contains("1"));
    }

    #[test]
    fn test_async_strategy_processes_multiple_clients() {
        let csv_content = "type,client,tx,amount\n\
                          deposit,1,1,100.0\n\
                          deposit,2,2,200.0\n\
                          deposit,1,3,50.0\n";
        let file = create_temp_csv(csv_content);

        let config = BatchConfig::default();
        let strategy = AsyncProcessingStrategy::new(config);
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Verify output contains both clients
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("1"));
        assert!(output_str.contains("2"));
    }

    #[test]
    fn test_async_strategy_handles_missing_file() {
        let config = BatchConfig::default();
        let strategy = AsyncProcessingStrategy::new(config);
        let mut output = Vec::new();

        let result = strategy.process(Path::new("nonexistent.csv"), &mut output);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to open file"));
    }

    #[test]
    fn test_async_strategy_maintains_ordering_across_batches() {
        // This test verifies that sequential batch processing maintains
        // per-client ordering even when a client's transactions span multiple batches
        let csv_content = "type,client,tx,amount\n\
                          deposit,1,1,100.0\n\
                          deposit,2,2,50.0\n\
                          withdrawal,1,3,30.0\n\
                          deposit,2,4,25.0\n\
                          withdrawal,1,5,20.0\n";
        let file = create_temp_csv(csv_content);

        // Use a small batch size to force multiple batches
        let config = BatchConfig::new(2, num_cpus::get());
        let strategy = AsyncProcessingStrategy::new(config);
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Parse output to verify final balances
        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.lines().collect();

        // Find client 1's balance (should be 100 - 30 - 20 = 50)
        let client1_line = lines.iter().find(|line| line.starts_with("1,")).unwrap();
        assert!(client1_line.contains("50.0000"), "Client 1 should have 50.0000, got: {}", client1_line);

        // Find client 2's balance (should be 50 + 25 = 75)
        let client2_line = lines.iter().find(|line| line.starts_with("2,")).unwrap();
        assert!(client2_line.contains("75.0000"), "Client 2 should have 75.0000, got: {}", client2_line);
    }
}
