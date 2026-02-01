//! Asynchronous CSV reader with stream interface
//!
//! Provides a streaming interface over transaction records from a CSV file.
//! Supports batch reading for efficient async processing.
//!
//! # Design
//!
//! The AsyncReader uses:
//! - csv-async for streaming CSV parsing
//! - tokio for async runtime and concurrency primitives
//! - Batch reading for efficient processing
//!
//! # Architecture
//!
//! ```text
//! CSV Reader → AsyncReader → Batches of TransactionRecords
//!                  ↓
//!           csv_format module
//!           (CsvRecord, convert_csv_record)
//! ```

use crate::io::csv_format::{convert_csv_record, CsvRecord};
use crate::types::TransactionRecord;
use csv_async::AsyncReaderBuilder;
use futures::io::AsyncRead;
use futures::stream::StreamExt;

/// Asynchronous CSV reader
///
/// Provides batch reading interface over transaction records.
/// Maintains streaming behavior with constant memory usage.
pub struct AsyncReader<R: AsyncRead + Unpin> {
    csv_reader: csv_async::AsyncDeserializer<R>,
}

impl<R: AsyncRead + Unpin + Send + 'static> AsyncReader<R> {
    /// Create a new AsyncReader from an async reader
    ///
    /// # Arguments
    ///
    /// * `reader` - Async reader providing CSV data
    ///
    /// # Returns
    ///
    /// A new AsyncReader instance
    pub fn new(reader: R) -> Self {
        let csv_reader = AsyncReaderBuilder::new()
            .flexible(true)
            .trim(csv_async::Trim::All)
            .create_deserializer(reader);

        Self { csv_reader }
    }

    /// Read a batch of transaction records
    ///
    /// This method reads up to `batch_size` records from the CSV file,
    /// converting them to TransactionRecords. Invalid records are logged
    /// to stderr and skipped.
    ///
    /// # Arguments
    ///
    /// * `batch_size` - Maximum number of records to read
    ///
    /// # Returns
    ///
    /// A vector of successfully converted transaction records.
    /// Returns an empty vector when the end of the file is reached.
    pub async fn read_batch(&mut self, batch_size: usize) -> Vec<TransactionRecord> {
        let mut batch = Vec::with_capacity(batch_size);
        let mut records = self.csv_reader.deserialize::<CsvRecord>();

        while batch.len() < batch_size {
            match records.next().await {
                Some(Ok(csv_record)) => match convert_csv_record(csv_record) {
                    Ok(transaction_record) => batch.push(transaction_record),
                    Err(e) => eprintln!("Record conversion error: {}", e),
                },
                Some(Err(e)) => eprintln!("CSV parse error: {}", e),
                None => break,
            }
        }

        batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::Cursor;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn test_async_reader_read_batch() {
        let csv_content =
            "type,client,tx,amount\ndeposit,1,1,100.0\nwithdrawal,1,2,50.0\ndeposit,2,3,200.0\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(2).await;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].client, 1);
        assert_eq!(batch[0].tx, 1);
        assert_eq!(batch[1].client, 1);
        assert_eq!(batch[1].tx, 2);

        let batch = async_reader.read_batch(2).await;
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].client, 2);
        assert_eq!(batch[0].tx, 3);
    }

    #[tokio::test]
    async fn test_async_reader_empty_csv() {
        let csv_content = "type,client,tx,amount\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(10).await;
        assert_eq!(batch.len(), 0);
    }

    #[tokio::test]
    async fn test_async_reader_invalid_record() {
        let csv_content = "type,client,tx,amount\ninvalid,1,1,100.0\ndeposit,1,2,50.0\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        // First record should fail conversion (invalid type)
        // Second record should succeed
        let batch = async_reader.read_batch(10).await;
        // Only the valid record should be in the batch (invalid one is logged to stderr)
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].tx, 2);
    }

    #[tokio::test]
    async fn test_async_reader_dispute_flow() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\ndispute,1,1,\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(10).await;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].amount, Some(Decimal::new(1000, 1)));
        assert_eq!(batch[1].amount, None);
    }

    #[tokio::test]
    async fn test_async_reader_batch_size_larger_than_records() {
        let csv_content = "type,client,tx,amount\ndeposit,1,1,100.0\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(100).await;
        assert_eq!(batch.len(), 1);
    }

    #[tokio::test]
    async fn test_async_reader_multiple_batches() {
        let csv_content = "type,client,tx,amount\n\
            deposit,1,1,100.0\n\
            deposit,1,2,200.0\n\
            deposit,1,3,300.0\n\
            deposit,1,4,400.0\n\
            deposit,1,5,500.0\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch1 = async_reader.read_batch(2).await;
        assert_eq!(batch1.len(), 2);
        assert_eq!(batch1[0].tx, 1);
        assert_eq!(batch1[1].tx, 2);

        let batch2 = async_reader.read_batch(2).await;
        assert_eq!(batch2.len(), 2);
        assert_eq!(batch2[0].tx, 3);
        assert_eq!(batch2[1].tx, 4);

        let batch3 = async_reader.read_batch(2).await;
        assert_eq!(batch3.len(), 1);
        assert_eq!(batch3[0].tx, 5);

        let batch4 = async_reader.read_batch(2).await;
        assert_eq!(batch4.len(), 0);
    }

    #[tokio::test]
    async fn test_async_reader_whitespace_handling() {
        let csv_content = "type,client,tx,amount\n  deposit  ,  1  ,  1  ,  100.0  \n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(10).await;
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].client, 1);
        assert_eq!(batch[0].tx, 1);
    }

    #[tokio::test]
    async fn test_async_reader_case_insensitive_type() {
        let csv_content = "type,client,tx,amount\nDEPOSIT,1,1,100.0\nWithdrawal,1,2,50.0\n";
        let reader = Cursor::new(csv_content.as_bytes());
        let mut async_reader = AsyncReader::new(reader);

        let batch = async_reader.read_batch(10).await;
        assert_eq!(batch.len(), 2);
    }
}
