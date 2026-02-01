//! Synchronous processing strategy
//!
//! This module provides a synchronous, single-threaded implementation of the
//! ProcessingStrategy trait. It orchestrates transaction processing by coordinating
//! between the SyncReader (for CSV input) and TransactionEngine (for business logic).
//!
//! # Design
//!
//! The SyncProcessingStrategy focuses on orchestration, delegating:
//! - CSV parsing to `SyncReader` (iterator interface)
//! - Transaction processing to `TransactionEngine` (business logic)
//! - CSV output to `csv_format::write_accounts_csv` (format handling)
//!
//! This separation of concerns makes the code more maintainable and testable.
//!
//! # Memory Efficiency
//!
//! This strategy maintains constant memory usage:
//! - Processes CSV records one at a time (streaming via iterator)
//! - Does not load entire file into memory
//! - Memory usage is O(accounts + disputable_transactions), not O(all_transactions)
//!
//! # Thread Safety
//!
//! While this strategy is single-threaded, it implements Send + Sync to be
//! compatible with the ProcessingStrategy trait, allowing it to be used in
//! multi-threaded contexts if needed.

use crate::core::TransactionEngine;
use crate::io::csv_format::write_accounts_csv;
use crate::io::sync_reader::SyncReader;
use crate::strategy::ProcessingStrategy;
use crate::types::Account;
use std::io::Write;
use std::path::Path;

/// Synchronous processing strategy
///
/// Implements the ProcessingStrategy trait using single-threaded, synchronous
/// processing. Orchestrates the flow between CSV reading, transaction processing,
/// and output generation.
///
/// # Examples
///
/// ```no_run
/// use rust_payments_engine::strategy::{ProcessingStrategy, SyncProcessingStrategy};
/// use std::path::Path;
/// use std::io;
///
/// let strategy = SyncProcessingStrategy;
/// let mut output = io::stdout();
///
/// strategy.process(Path::new("transactions.csv"), &mut output)
///     .expect("Processing failed");
/// ```
///
/// # Thread Safety
///
/// SyncProcessingStrategy is Send + Sync, allowing it to be shared across threads
/// safely, even though it performs single-threaded processing.
///
/// # Backward Compatibility
///
/// This strategy maintains backward compatibility with the original implementation:
/// - Uses the same streaming approach for CSV reading
/// - Uses the same TransactionEngine for processing
/// - Produces identical output for the same input
/// - Has the same error handling behavior
#[derive(Debug, Clone, Copy)]
pub struct SyncProcessingStrategy;

impl ProcessingStrategy for SyncProcessingStrategy {
    /// Process transactions from input file and write results to output
    ///
    /// This method orchestrates the complete synchronous processing pipeline:
    /// 1. Creates a SyncReader to stream transaction records from the CSV file
    /// 2. Creates a TransactionEngine to process transactions
    /// 3. Iterates through records, processing each through the engine
    /// 4. Collects final account states from the engine
    /// 5. Writes account states to output using csv_format::write_accounts_csv
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
    /// Fatal errors (file not found, I/O errors) are returned immediately.
    /// Individual transaction errors are logged to stderr and processing continues.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rust_payments_engine::strategy::{ProcessingStrategy, SyncProcessingStrategy};
    /// use std::path::Path;
    /// use std::io;
    ///
    /// let strategy = SyncProcessingStrategy;
    /// let mut output = io::stdout();
    ///
    /// match strategy.process(Path::new("transactions.csv"), &mut output) {
    ///     Ok(()) => println!("Processing completed"),
    ///     Err(e) => eprintln!("Fatal error: {}", e),
    /// }
    /// ```
    fn process(&self, input_path: &Path, output: &mut dyn Write) -> Result<(), String> {
        // Create transaction engine
        let mut engine = TransactionEngine::new();

        // Create sync reader for streaming CSV input
        let reader = SyncReader::new(input_path)?;

        // Process each transaction record through the engine
        // The iterator interface allows us to process one record at a time
        for result in reader {
            match result {
                Ok(transaction_record) => {
                    // Process the transaction through the engine
                    // Individual transaction errors are handled by the engine
                    if let Err(e) = engine.process(transaction_record) {
                        // Log transaction processing errors to stderr
                        eprintln!("Transaction processing error: {}", e);
                    }
                }
                Err(e) => {
                    // Log CSV parsing/conversion errors to stderr
                    eprintln!("CSV parsing error: {}", e);
                }
            }
        }

        // Get final account states from the engine
        let account_refs = engine.get_accounts();

        // Convert references to owned accounts for CSV writing
        let accounts: Vec<Account> = account_refs.iter().map(|&a| a.clone()).collect();

        // Write account states to output using csv_format module
        write_accounts_csv(&accounts, output)?;

        Ok(())
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
    fn test_sync_strategy_processes_valid_deposit() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let file = create_temp_csv(csv_content);

        let strategy = SyncProcessingStrategy;
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Verify output contains account data
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("client"));
        assert!(output_str.contains("1"));
    }

    #[test]
    fn test_sync_strategy_processes_multiple_transactions() {
        let csv_content = "type,client,tx,amount\n\
                          deposit,1,1,100.0\n\
                          withdrawal,1,2,50.0\n\
                          deposit,2,3,200.0\n";
        let file = create_temp_csv(csv_content);

        let strategy = SyncProcessingStrategy;
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Verify output contains both clients
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("1"));
        assert!(output_str.contains("2"));
    }

    #[test]
    fn test_sync_strategy_handles_missing_file() {
        let strategy = SyncProcessingStrategy;
        let mut output = Vec::new();

        let result = strategy.process(Path::new("nonexistent.csv"), &mut output);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to open file"));
    }

    #[test]
    fn test_sync_strategy_handles_dispute_flow() {
        let csv_content = "type,client,tx,amount\n\
                          deposit,1,1,100.0\n\
                          dispute,1,1,\n";
        let file = create_temp_csv(csv_content);

        let strategy = SyncProcessingStrategy;
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Verify output was generated
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("client"));
    }

    #[test]
    fn test_sync_strategy_is_send_sync() {
        // Verify that SyncProcessingStrategy implements Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SyncProcessingStrategy>();
    }

    #[test]
    fn test_sync_strategy_can_be_cloned() {
        let strategy1 = SyncProcessingStrategy;
        let strategy2 = strategy1;

        // Both should work independently
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let file1 = create_temp_csv(csv_content);
        let file2 = create_temp_csv(csv_content);

        let mut output1 = Vec::new();
        let mut output2 = Vec::new();

        assert!(strategy1.process(file1.path(), &mut output1).is_ok());
        assert!(strategy2.process(file2.path(), &mut output2).is_ok());
    }

    #[test]
    fn test_sync_strategy_continues_on_malformed_record() {
        // Second record has invalid amount, but processing should continue
        let csv_content = "type,client,tx,amount\n\
                          deposit,1,1,100.0\n\
                          deposit,2,2,invalid\n\
                          deposit,3,3,50.0\n";
        let file = create_temp_csv(csv_content);

        let strategy = SyncProcessingStrategy;
        let mut output = Vec::new();

        let result = strategy.process(file.path(), &mut output);
        assert!(result.is_ok());

        // Should have processed client 1 and client 3, but not client 2
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("1"));
        assert!(output_str.contains("3"));
    }
}
