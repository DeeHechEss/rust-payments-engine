//! Batch processing with client-based partitioning for async transaction processing
//!
//! This module provides the `BatchProcessor` struct, which manages concurrent batch
//! processing with client-based partitioning to enable parallel processing while
//! maintaining per-client transaction ordering.
//!
//! # Design
//!
//! The `BatchProcessor` partitions batches by client ID, allowing transactions for
//! different clients to be processed concurrently while maintaining sequential
//! ordering for each individual client's transactions.
//!
//! # Architecture
//!
//! ```text
//! BatchProcessor
//!     ├── Arc<AsyncTransactionEngine>  (shared transaction processor)
//!     └── BatchConfig                  (configuration parameters)
//! ```
//!
//! # Thread Safety
//!
//! The processor is cloneable and can be safely shared across async tasks.
//! All internal state is protected by Arc, and the underlying engine uses
//! thread-safe components.

use std::collections::HashMap;
use std::sync::Arc;

use super::AsyncTransactionEngine;
use crate::types::{ClientId, PaymentError, TransactionRecord};

/// Result of processing a single transaction
///
/// Contains the original transaction record and the result of processing it.
#[derive(Debug, Clone)]
pub struct ProcessingResult {
    /// The transaction record that was processed
    pub record: TransactionRecord,

    /// The result of processing (success or error)
    pub result: Result<(), PaymentError>,
}

/// Batch processor with client-based partitioning
///
/// `BatchProcessor` manages concurrent batch processing by partitioning
/// transactions by client ID. This enables parallel processing of transactions
/// for different clients while maintaining sequential ordering for each client.
#[derive(Debug, Clone)]
pub struct BatchProcessor {
    /// Thread-safe transaction processing engine
    ///
    /// Wrapped in Arc to enable sharing across async tasks.
    engine: Arc<AsyncTransactionEngine>,
}

impl BatchProcessor {
    /// Create a new BatchProcessor
    ///
    /// # Arguments
    ///
    /// * `engine` - Arc-wrapped AsyncTransactionEngine for transaction processing
    ///
    /// # Returns
    ///
    /// A new `BatchProcessor` that can be cloned and shared across async tasks.
    pub fn new(engine: Arc<AsyncTransactionEngine>) -> Self {
        Self { engine }
    }

    /// Partition a batch of transactions by client ID
    ///
    /// This method partitions a batch into sub-batches where each sub-batch contains
    /// only transactions for a single client. This enables parallel processing of
    /// transactions for different clients while maintaining sequential ordering for
    /// each client.
    ///
    /// # Arguments
    ///
    /// * `batch` - A vector of transaction records to partition
    ///
    /// # Returns
    ///
    /// A HashMap where:
    /// - Keys are client IDs
    /// - Values are vectors of transactions for that client (in original order)
    ///
    /// # Guarantees
    ///
    /// - Each transaction appears in exactly one sub-batch
    /// - No transactions are lost or duplicated
    /// - Transactions for each client maintain their original order
    /// - Sub-batches contain only transactions for a single client
    ///
    pub fn partition_by_client(
        &self,
        batch: Vec<TransactionRecord>,
    ) -> HashMap<ClientId, Vec<TransactionRecord>> {
        let mut client_batches: HashMap<ClientId, Vec<TransactionRecord>> = HashMap::new();

        for record in batch {
            client_batches
                .entry(record.client)
                .or_default()
                .push(record);
        }

        client_batches
    }

    /// Process all transactions for a single client sequentially
    ///
    /// This method processes all transactions for a single client in the order they
    /// appear in the input vector. This ensures that per-client transaction ordering
    /// is maintained even when multiple clients are being processed concurrently.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID whose transactions are being processed
    /// * `transactions` - A vector of transactions for this client (in order)
    ///
    /// # Returns
    ///
    /// A vector of `ProcessingResult` containing the outcome of each transaction.
    /// Results are in the same order as the input transactions.
    ///
    /// # Guarantees
    ///
    /// - Transactions are processed in the order they appear in the input vector
    /// - All transactions are processed, even if some fail
    /// - Errors are captured in the result and don't stop processing
    /// - Results maintain the same order as input transactions
    pub async fn process_client_transactions(
        &self,
        transactions: Vec<TransactionRecord>,
    ) -> Vec<ProcessingResult> {
        let mut results = Vec::with_capacity(transactions.len());

        for record in transactions {
            let result = self.engine.process_transaction(record.clone());
            results.push(ProcessingResult { record, result });
        }

        results
    }

    /// Process a batch of transactions with client-based partitioning
    ///
    /// This method processes a batch of transactions by:
    /// 1. Partitioning the batch by client ID
    /// 2. Spawning tokio tasks to process each client's transactions concurrently
    /// 3. Waiting for all tasks to complete
    /// 4. Collecting and returning all results
    ///
    /// # Arguments
    ///
    /// * `batch` - A vector of transaction records to process
    ///
    /// # Returns
    ///
    /// A vector of `ProcessingResult` containing the outcome of each transaction.
    /// Results may be in a different order than the input due to concurrent processing.
    ///
    /// # Guarantees
    ///
    /// - Transactions for different clients are processed concurrently
    /// - Transactions for the same client are processed sequentially in order
    /// - All transactions are processed, even if some fail
    /// - Errors are captured in results and don't stop processing
    pub async fn process_batch(&self, batch: Vec<TransactionRecord>) -> Vec<ProcessingResult> {
        // Partition batch by client ID
        let client_batches = self.partition_by_client(batch);

        // Spawn tokio tasks for each client's transactions
        let mut tasks = Vec::new();
        for (_client_id, transactions) in client_batches {
            let processor = self.clone();
            let task = tokio::spawn(async move {
                processor
                    .process_client_transactions(transactions)
                    .await
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete and collect results
        let mut results = Vec::new();
        for task in tasks {
            match task.await {
                Ok(client_results) => results.extend(client_results),
                Err(e) => {
                    eprintln!("Task panicked: {:?}", e);
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::r#async::{AsyncAccountManager, AsyncTransactionStore};

    #[test]
    fn test_new_creates_processor() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let _processor = BatchProcessor::new(Arc::clone(&engine));

        // Verify the processor was created (basic smoke test)
        assert!(Arc::strong_count(&engine) >= 2); // Original + processor
    }

    #[test]
    fn test_processor_is_cloneable() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(Arc::clone(&engine));

        // Clone the processor
        let _processor_clone = processor.clone();

        // Verify both processors share the same underlying engine
        assert!(Arc::strong_count(&engine) >= 3); // Original + processor + clone
    }

    #[test]
    fn test_processor_can_be_shared_across_threads() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        // Spawn threads that clone the processor
        let mut handles = vec![];
        for _ in 0..5 {
            let processor_clone = processor.clone();
            let handle = thread::spawn(move || {
                // Just verify we can access the cloned processor in another thread
                let _processor = processor_clone;
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Test passes if no panics occurred
    }

    // Partitioning tests

    #[test]
    fn test_partition_by_client_empty_batch() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![];
        let partitioned = processor.partition_by_client(batch);

        assert_eq!(partitioned.len(), 0);
    }

    #[test]
    fn test_partition_by_client_single_client() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Withdrawal,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(5000, 4)),
            },
        ];

        let partitioned = processor.partition_by_client(batch);

        // Should have exactly one client
        assert_eq!(partitioned.len(), 1);

        // Client 1 should have all 3 transactions
        let client1_txs = partitioned.get(&1).unwrap();
        assert_eq!(client1_txs.len(), 3);

        // Verify order is maintained
        assert_eq!(client1_txs[0].tx, 1);
        assert_eq!(client1_txs[1].tx, 2);
        assert_eq!(client1_txs[2].tx, 3);
    }

    #[test]
    fn test_partition_by_client_multiple_clients() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(5000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 3,
                tx: 4,
                amount: Some(Decimal::new(15000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 5,
                amount: Some(Decimal::new(8000, 4)),
            },
        ];

        let partitioned = processor.partition_by_client(batch);

        // Should have 3 clients
        assert_eq!(partitioned.len(), 3);

        // Client 1 should have 2 transactions
        let client1_txs = partitioned.get(&1).unwrap();
        assert_eq!(client1_txs.len(), 2);
        assert_eq!(client1_txs[0].tx, 1);
        assert_eq!(client1_txs[1].tx, 3);

        // Client 2 should have 2 transactions
        let client2_txs = partitioned.get(&2).unwrap();
        assert_eq!(client2_txs.len(), 2);
        assert_eq!(client2_txs[0].tx, 2);
        assert_eq!(client2_txs[1].tx, 5);

        // Client 3 should have 1 transaction
        let client3_txs = partitioned.get(&3).unwrap();
        assert_eq!(client3_txs.len(), 1);
        assert_eq!(client3_txs[0].tx, 4);
    }

    #[test]
    fn test_partition_by_client_maintains_order() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        // Create a batch with interleaved transactions for the same client
        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 10,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 20,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 11,
                amount: Some(Decimal::new(5000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 12,
                amount: Some(Decimal::new(3000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 21,
                amount: Some(Decimal::new(8000, 4)),
            },
        ];

        let partitioned = processor.partition_by_client(batch);

        // Verify client 1 transactions are in order
        let client1_txs = partitioned.get(&1).unwrap();
        assert_eq!(client1_txs.len(), 3);
        assert_eq!(client1_txs[0].tx, 10);
        assert_eq!(client1_txs[1].tx, 11);
        assert_eq!(client1_txs[2].tx, 12);

        // Verify client 2 transactions are in order
        let client2_txs = partitioned.get(&2).unwrap();
        assert_eq!(client2_txs.len(), 2);
        assert_eq!(client2_txs[0].tx, 20);
        assert_eq!(client2_txs[1].tx, 21);
    }

    #[test]
    fn test_partition_by_client_no_transactions_lost() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 3,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let original_count = batch.len();
        let partitioned = processor.partition_by_client(batch);

        // Count total transactions in all sub-batches
        let total_count: usize = partitioned.values().map(|v| v.len()).sum();

        // Verify no transactions were lost
        assert_eq!(total_count, original_count);
    }

    #[test]
    fn test_partition_by_client_no_duplicates() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;
        use std::collections::HashSet;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let partitioned = processor.partition_by_client(batch);

        // Collect all transaction IDs
        let mut tx_ids = HashSet::new();
        for transactions in partitioned.values() {
            for record in transactions {
                // If insert returns false, it means the ID was already present (duplicate)
                assert!(tx_ids.insert(record.tx), "Duplicate transaction ID found");
            }
        }

        // Verify we have all 3 unique transaction IDs
        assert_eq!(tx_ids.len(), 3);
        assert!(tx_ids.contains(&1));
        assert!(tx_ids.contains(&2));
        assert!(tx_ids.contains(&3));
    }

    #[test]
    fn test_partition_by_client_many_clients() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        // Create a batch with 100 clients, each with 1 transaction
        let mut batch = Vec::new();
        for i in 0..100 {
            batch.push(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: i,
                tx: i as u32,
                amount: Some(Decimal::new(10000, 4)),
            });
        }

        let partitioned = processor.partition_by_client(batch);

        // Should have 100 clients
        assert_eq!(partitioned.len(), 100);

        // Each client should have exactly 1 transaction
        for i in 0..100 {
            let client_txs = partitioned.get(&i).unwrap();
            assert_eq!(client_txs.len(), 1);
            assert_eq!(client_txs[0].client, i);
        }
    }

    #[test]
    fn test_partition_by_client_with_dispute_transactions() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
        ];

        let partitioned = processor.partition_by_client(batch);

        // Client 1 should have 2 transactions (deposit + dispute)
        let client1_txs = partitioned.get(&1).unwrap();
        assert_eq!(client1_txs.len(), 2);
        assert_eq!(client1_txs[0].tx_type, TransactionType::Deposit);
        assert_eq!(client1_txs[1].tx_type, TransactionType::Dispute);

        // Client 2 should have 1 transaction
        let client2_txs = partitioned.get(&2).unwrap();
        assert_eq!(client2_txs.len(), 1);
    }

    // Process client transactions tests

    #[tokio::test]
    async fn test_process_client_transactions_empty() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![];
        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_process_client_transactions_single_deposit() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        }];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].result.is_ok());

        // Verify account was updated
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_multiple_deposits() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(5000, 4)),
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 3);
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_ok());
        assert!(results[2].result.is_ok());

        // Verify account has correct total
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(35000, 4)); // 1.0 + 2.0 + 0.5
        assert_eq!(account.total, Decimal::new(35000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_deposit_and_withdrawal() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Withdrawal,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(3000, 4)),
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 2);
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_ok());

        // Verify account has correct balance
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(7000, 4)); // 1.0 - 0.3
        assert_eq!(account.total, Decimal::new(7000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_insufficient_funds() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Withdrawal,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)), // More than available
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 2);
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_err()); // Should fail due to insufficient funds

        // Verify account still has the deposit amount
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_continues_after_error() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Withdrawal,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)), // Will fail
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(5000, 4)), // Should still process
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 3);
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_err());
        assert!(results[2].result.is_ok()); // Should succeed despite previous error

        // Verify account has both deposits
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(15000, 4)); // 1.0 + 0.5
        assert_eq!(account.total, Decimal::new(15000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_dispute_flow() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        assert_eq!(results.len(), 2);
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_ok());

        // Verify funds are held
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[tokio::test]
    async fn test_process_client_transactions_maintains_order() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let transactions = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let results = processor.process_client_transactions(transactions).await;

        // Verify results are in the same order as input
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].record.tx, 1);
        assert_eq!(results[1].record.tx, 2);
        assert_eq!(results[2].record.tx, 3);
    }

    // Process batch tests

    #[tokio::test]
    async fn test_process_batch_empty() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![];
        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_process_batch_single_client() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.result.is_ok()));

        // Verify account has correct total
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(30000, 4));
        assert_eq!(account.total, Decimal::new(30000, 4));
    }

    #[tokio::test]
    async fn test_process_batch_multiple_clients() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 3,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.result.is_ok()));

        // Verify each account has correct balance
        let account1 = account_manager.get_or_create(1);
        assert_eq!(account1.available, Decimal::new(10000, 4));

        let account2 = account_manager.get_or_create(2);
        assert_eq!(account2.available, Decimal::new(20000, 4));

        let account3 = account_manager.get_or_create(3);
        assert_eq!(account3.available, Decimal::new(30000, 4));
    }

    #[tokio::test]
    async fn test_process_batch_interleaved_clients() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 3,
                amount: Some(Decimal::new(5000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 4,
                amount: Some(Decimal::new(8000, 4)),
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|r| r.result.is_ok()));

        // Verify each account has correct total
        let account1 = account_manager.get_or_create(1);
        assert_eq!(account1.available, Decimal::new(15000, 4)); // 1.0 + 0.5

        let account2 = account_manager.get_or_create(2);
        assert_eq!(account2.available, Decimal::new(28000, 4)); // 2.0 + 0.8
    }

    #[tokio::test]
    async fn test_process_batch_with_errors() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Withdrawal,
                client: 1,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)), // Will fail - insufficient funds
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 3);

        // Count successes and failures
        let successes = results.iter().filter(|r| r.result.is_ok()).count();
        let failures = results.iter().filter(|r| r.result.is_err()).count();

        assert_eq!(successes, 2);
        assert_eq!(failures, 1);

        // Verify accounts have correct balances
        let account1 = account_manager.get_or_create(1);
        assert_eq!(account1.available, Decimal::new(10000, 4)); // Only deposit succeeded

        let account2 = account_manager.get_or_create(2);
        assert_eq!(account2.available, Decimal::new(30000, 4));
    }

    #[tokio::test]
    async fn test_process_batch_partial_batch() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        // Small batch (less than typical batch size)
        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.result.is_ok()));
    }

    #[tokio::test]
    async fn test_process_batch_many_clients() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        // Create a batch with 50 clients, each with 2 transactions
        let mut batch = Vec::new();
        for i in 0..50 {
            batch.push(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: i,
                tx: i as u32 * 2,
                amount: Some(Decimal::new(10000, 4)),
            });
            batch.push(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: i,
                tx: i as u32 * 2 + 1,
                amount: Some(Decimal::new(5000, 4)),
            });
        }

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 100); // 50 clients * 2 transactions
        assert!(results.iter().all(|r| r.result.is_ok()));

        // Verify each client has correct total
        for i in 0..50 {
            let account = account_manager.get_or_create(i);
            assert_eq!(account.available, Decimal::new(15000, 4)); // 1.0 + 0.5
        }
    }

    #[tokio::test]
    async fn test_process_batch_dispute_flow() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            },
            TransactionRecord {
                tx_type: TransactionType::Resolve,
                client: 1,
                tx: 1,
                amount: None,
            },
        ];

        let results = processor.process_batch(batch).await;

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.result.is_ok()));

        // Verify funds are back to available after resolve
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[tokio::test]
    async fn test_process_batch_all_transactions_processed() {
        use crate::types::TransactionType;
        use rust_decimal::Decimal;
        use std::collections::HashSet;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = Arc::new(AsyncTransactionEngine::new(
            account_manager,
            transaction_store,
        ));

        let processor = BatchProcessor::new(engine);

        let batch = vec![
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            },
            TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 3,
                tx: 3,
                amount: Some(Decimal::new(30000, 4)),
            },
        ];

        let original_tx_ids: HashSet<u32> = batch.iter().map(|r| r.tx).collect();
        let results = processor.process_batch(batch).await;

        // Verify all transactions were processed
        let result_tx_ids: HashSet<u32> = results.iter().map(|r| r.record.tx).collect();
        assert_eq!(original_tx_ids, result_tx_ids);
    }
}
