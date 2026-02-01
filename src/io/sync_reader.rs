//! Synchronous CSV reader with iterator interface
//!
//! Provides a streaming iterator over transaction records from a CSV file.
//! Delegates CSV format concerns to the csv_format module.
//!
//! # Design
//!
//! The SyncReader uses csv::Reader to read and deserialize CSV records sequentially,
//! delegating parsing and conversion to the csv_format module. It maintains streaming
//! behavior by processing CSV records one at a time without loading the entire file
//! into memory.
//!
//! # Iterator Interface
//!
//! SyncReader implements the Iterator trait, yielding Result<TransactionRecord, String>
//! for each CSV row. This allows for idiomatic Rust iteration patterns:
//!
//! ```no_run
//! use rust_payments_engine::io::sync_reader::SyncReader;
//! use std::path::Path;
//!
//! let reader = SyncReader::new(Path::new("transactions.csv")).unwrap();
//! for result in reader {
//!     match result {
//!         Ok(record) => println!("Processing transaction: {:?}", record),
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! ```
//!
//! # Error Handling
//!
//! - Fatal errors (file not found, I/O errors) are returned from `new()`
//! - Individual record parsing errors are yielded as Err variants in the iterator
//! - Line numbers are included in error messages for debugging
//!
//! # Memory Efficiency
//!
//! The reader maintains streaming behavior:
//! - Reads CSV records one at a time
//! - Does not load entire file into memory
//! - Memory usage is O(1) per record, not O(file_size)

use crate::io::csv_format::{convert_csv_record, CsvRecord};
use crate::types::TransactionRecord;
use csv::{ReaderBuilder, Trim};
use std::fs::File;
use std::path::Path;

/// Synchronous CSV reader
///
/// Provides an iterator interface over transaction records.
/// Maintains streaming behavior with constant memory usage.
///
/// # Examples
///
/// ```no_run
/// use rust_payments_engine::io::sync_reader::SyncReader;
/// use std::path::Path;
///
/// let reader = SyncReader::new(Path::new("transactions.csv")).unwrap();
/// let records: Vec<_> = reader.filter_map(Result::ok).collect();
/// println!("Successfully parsed {} records", records.len());
/// ```
#[derive(Debug)]
pub struct SyncReader {
    reader: csv::Reader<File>,
    line_num: usize,
}

impl SyncReader {
    /// Create a new SyncReader from a file path
    ///
    /// Opens the CSV file and prepares it for streaming iteration.
    /// The CSV reader is configured to:
    /// - Trim whitespace from all fields
    /// - Allow flexible field counts (for optional amount field)
    /// - Use an 8KB buffer for efficient I/O
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the CSV file
    ///
    /// # Returns
    ///
    /// * `Ok(SyncReader)` if file opened successfully
    /// * `Err(String)` if file could not be opened
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rust_payments_engine::io::sync_reader::SyncReader;
    /// use std::path::Path;
    ///
    /// match SyncReader::new(Path::new("transactions.csv")) {
    ///     Ok(reader) => println!("File opened successfully"),
    ///     Err(e) => eprintln!("Failed to open file: {}", e),
    /// }
    /// ```
    pub fn new(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("Failed to open file '{}': {}", path.display(), e))?;

        let reader = ReaderBuilder::new()
            .trim(Trim::All)
            .flexible(true)
            .buffer_capacity(8 * 1024)
            .from_reader(file);

        Ok(Self {
            reader,
            line_num: 0,
        })
    }
}

impl Iterator for SyncReader {
    type Item = Result<TransactionRecord, String>;

    /// Get the next transaction record from the CSV file
    ///
    /// This method:
    /// 1. Reads the next CSV row and deserializes it to CsvRecord
    /// 2. Converts the CsvRecord to TransactionRecord using csv_format::convert_csv_record
    /// 3. Includes line numbers in error messages for debugging
    ///
    /// # Returns
    ///
    /// * `Some(Ok(TransactionRecord))` - Successfully parsed record
    /// * `Some(Err(String))` - Parse or conversion error with line number
    /// * `None` - End of file reached
    fn next(&mut self) -> Option<Self::Item> {
        // Get next CSV record
        let mut deserializer = self.reader.deserialize::<CsvRecord>();

        match deserializer.next()? {
            Ok(csv_record) => {
                self.line_num += 1;
                // Convert CSV record to TransactionRecord
                // Add line number context to any conversion errors
                Some(
                    convert_csv_record(csv_record)
                        .map_err(|e| format!("Line {}: {}", self.line_num + 1, e)),
                )
            }
            Err(e) => {
                self.line_num += 1;
                Some(Err(format!(
                    "Line {}: CSV parse error: {}",
                    self.line_num + 1,
                    e
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransactionType;
    use rust_decimal::Decimal;
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
    fn test_sync_reader_new_opens_file() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let file = create_temp_csv(csv_content);

        let result = SyncReader::new(file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_sync_reader_new_fails_on_missing_file() {
        let result = SyncReader::new(Path::new("nonexistent.csv"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to open file"));
    }

    #[test]
    fn test_sync_reader_iterates_valid_deposit() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 1);
        assert!(records[0].is_ok());

        let record = records[0].as_ref().unwrap();
        assert_eq!(record.tx_type, TransactionType::Deposit);
        assert_eq!(record.client, 1);
        assert_eq!(record.tx, 1);
        assert_eq!(record.amount, Some(Decimal::new(1000, 1)));
    }

    #[test]
    fn test_sync_reader_iterates_multiple_records() {
        let csv_content =
            "type,client,tx,amount\ndeposit,1,1,100.0\nwithdrawal,1,2,50.0\ndispute,1,1,\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 3);
        assert!(records[0].is_ok());
        assert!(records[1].is_ok());
        assert!(records[2].is_ok());
    }

    #[test]
    fn test_sync_reader_handles_malformed_record() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,invalid\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 1);
        assert!(records[0].is_err());
        let error = records[0].as_ref().unwrap_err();
        assert!(error.contains("Line 2"));
        assert!(error.contains("Invalid amount"));
    }

    #[test]
    fn test_sync_reader_includes_line_numbers_in_errors() {
        let csv_content =
            "type,client,tx,amount\ndeposit,1,1,100.0\ndeposit,2,2,invalid\ndeposit,3,3,50.0\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 3);
        assert!(records[0].is_ok());
        assert!(records[1].is_err());
        assert!(records[2].is_ok());

        let error = records[1].as_ref().unwrap_err();
        assert!(error.contains("Line 3")); // Line 3 because of header
    }

    #[test]
    fn test_sync_reader_handles_whitespace() {
        let csv_content = "type,client,tx,amount\n  deposit  ,  1  ,  1  ,  100.0  \n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 1);
        assert!(records[0].is_ok());

        let record = records[0].as_ref().unwrap();
        assert_eq!(record.client, 1);
        assert_eq!(record.amount, Some(Decimal::new(1000, 1)));
    }

    #[test]
    fn test_sync_reader_handles_all_transaction_types() {
        let csv_content = "type,client,tx,amount\n\
            deposit,1,1,100.0\n\
            withdrawal,1,2,50.0\n\
            dispute,1,1,\n\
            resolve,1,1,\n\
            chargeback,1,2,\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.filter_map(Result::ok).collect();

        assert_eq!(records.len(), 5);
        assert_eq!(records[0].tx_type, TransactionType::Deposit);
        assert_eq!(records[1].tx_type, TransactionType::Withdrawal);
        assert_eq!(records[2].tx_type, TransactionType::Dispute);
        assert_eq!(records[3].tx_type, TransactionType::Resolve);
        assert_eq!(records[4].tx_type, TransactionType::Chargeback);
    }

    #[test]
    fn test_sync_reader_handles_empty_file_after_header() {
        let csv_content = "type,client,tx,amount\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_sync_reader_continues_after_error() {
        let csv_content = "type,client,tx,amount\n\
            deposit,1,1,100.0\n\
            invalid_type,2,2,50.0\n\
            deposit,3,3,75.0\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.collect();

        assert_eq!(records.len(), 3);
        assert!(records[0].is_ok());
        assert!(records[1].is_err());
        assert!(records[2].is_ok());
    }

    #[test]
    fn test_sync_reader_filter_map_pattern() {
        let csv_content = "type,client,tx,amount\n\
            deposit,1,1,100.0\n\
            deposit,2,2,invalid\n\
            deposit,3,3,50.0\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let valid_records: Vec<_> = reader.filter_map(Result::ok).collect();

        assert_eq!(valid_records.len(), 2);
        assert_eq!(valid_records[0].client, 1);
        assert_eq!(valid_records[1].client, 3);
    }

    #[test]
    fn test_sync_reader_case_insensitive_types() {
        let csv_content = "type,client,tx,amount\n\
            DEPOSIT,1,1,100.0\n\
            Withdrawal,1,2,50.0\n\
            DiSpUtE,1,1,\n";
        let file = create_temp_csv(csv_content);

        let reader = SyncReader::new(file.path()).unwrap();
        let records: Vec<_> = reader.filter_map(Result::ok).collect();

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].tx_type, TransactionType::Deposit);
        assert_eq!(records[1].tx_type, TransactionType::Withdrawal);
        assert_eq!(records[2].tx_type, TransactionType::Dispute);
    }
}
