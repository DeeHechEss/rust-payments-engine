//! Thread-safe transaction storage for async batch processing
//!
//! This module provides the `AsyncTransactionStore` struct, which stores transaction
//! history using concurrent data structures to enable safe multi-threaded access.
//!
//! # Design
//!
//! The `AsyncTransactionStore` uses `DashMap` (a concurrent HashMap) to provide thread-safe
//! transaction storage with fine-grained locking. This allows multiple threads to safely
//! access different transactions concurrently while maintaining consistency for operations
//! on the same transaction.
//!
//! # Purpose
//!
//! Transaction storage is required for dispute resolution. When a dispute, resolve, or
//! chargeback is processed, the system needs to look up the original transaction to:
//! - Verify the transaction exists
//! - Check the transaction amount
//! - Verify the client ID matches
//! - Track dispute state (disputed, resolved, charged back)
//!
//! # Thread Safety
//!
//! All operations are thread-safe and prevent data races through DashMap's internal
//! synchronization. The Rust type system ensures that shared references cannot be
//! used to mutate state, and mutable operations are properly synchronized.

use crate::types::{StoredTransaction, TransactionId};
use dashmap::DashMap;

/// Thread-safe transaction store for async batch processing
///
/// `AsyncTransactionStore` provides concurrent access to transaction history using
/// `DashMap` for fine-grained locking. Multiple threads can safely access different
/// transactions simultaneously, while operations on the same transaction are
/// automatically serialized.
///
/// # Purpose
///
/// Only deposits and withdrawals are stored, as these are the only transaction types
/// that can be disputed. This optimizes memory usage by not storing dispute, resolve,
/// or chargeback operations.
///
/// # Thread Safety
///
/// All methods are safe to call from multiple threads concurrently. The internal
/// `DashMap` ensures that:
/// - Concurrent reads to different transactions don't block each other
/// - Concurrent writes to different transactions don't block each other
/// - Operations on the same transaction are properly synchronized
///
/// # Performance
///
/// For multi-threaded workloads with many different transactions, `AsyncTransactionStore`
/// provides excellent scalability. However, for single-threaded workloads, the synchronous
/// `TransactionStore` is more efficient.
#[derive(Debug)]
pub struct AsyncTransactionStore {
    /// Concurrent HashMap storing transaction history by transaction ID
    ///
    /// DashMap provides fine-grained locking through internal sharding,
    /// allowing concurrent access to different transactions without global locks.
    transactions: DashMap<TransactionId, StoredTransaction>,
}

impl AsyncTransactionStore {
    /// Create a new empty AsyncTransactionStore
    ///
    /// # Returns
    ///
    /// A new `AsyncTransactionStore` with no transactions. Transactions will be stored
    /// as they are processed (deposits and withdrawals only).
    pub fn new() -> Self {
        Self {
            transactions: DashMap::new(),
        }
    }
}

impl Default for AsyncTransactionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncTransactionStore {
    /// Store a transaction in the store (thread-safe)
    ///
    /// This method inserts a transaction into the store, making it available for
    /// future dispute operations. Only deposits and withdrawals should be stored,
    /// as these are the only transaction types that can be disputed.
    ///
    /// If a transaction with the same ID already exists, the new transaction
    /// is ignored (first occurrence wins).
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The unique transaction ID
    /// * `transaction` - The transaction data to store
    ///
    /// # Thread Safety
    ///
    /// This method is safe to call from multiple threads concurrently. If multiple
    /// threads attempt to store the same transaction ID simultaneously, one will
    /// win and the others will be ignored.
    pub fn store(&self, tx_id: TransactionId, transaction: StoredTransaction) {
        // Only store if not already present (first occurrence wins)
        self.transactions.entry(tx_id).or_insert(transaction);
    }

    /// Get a transaction from the store (read-only, thread-safe)
    ///
    /// This method retrieves a transaction by its ID. The transaction is cloned
    /// to avoid holding locks longer than necessary.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction ID to look up
    ///
    /// # Returns
    ///
    /// * `Some(StoredTransaction)` - If the transaction exists
    /// * `None` - If the transaction is not found
    ///
    /// # Thread Safety
    ///
    /// This method is safe to call from multiple threads concurrently. Multiple
    /// threads can read different transactions simultaneously without blocking.
    pub fn get(&self, tx_id: TransactionId) -> Option<StoredTransaction> {
        self.transactions
            .get(&tx_id)
            .map(|entry| entry.value().clone())
    }

    /// Update a transaction with a closure (atomic operation, thread-safe)
    ///
    /// This method allows atomic updates to a transaction's state. The closure
    /// receives a mutable reference to the transaction and can modify it. The
    /// modification is atomic - no other thread can access the transaction while
    /// the closure is executing.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction ID to update
    /// * `f` - A closure that receives a mutable reference to the transaction
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the transaction was found and updated successfully
    /// * `Err(PaymentError::TransactionNotFound)` - If the transaction doesn't exist
    /// * `Err(...)` - If the closure returns an error
    ///
    /// # Thread Safety
    ///
    /// This method is safe to call from multiple threads concurrently. The closure
    /// executes while holding a lock on the specific transaction, ensuring atomicity.
    /// Other threads attempting to access the same transaction will wait, while
    /// threads accessing different transactions can proceed concurrently.
    pub fn update<F>(&self, tx_id: TransactionId, f: F) -> Result<(), crate::types::PaymentError>
    where
        F: FnOnce(&mut StoredTransaction) -> Result<(), crate::types::PaymentError>,
    {
        match self.transactions.get_mut(&tx_id) {
            Some(mut entry) => f(entry.value_mut()),
            None => Err(crate::types::PaymentError::transaction_not_found(
                tx_id, "update",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PaymentError, TransactionType};
    use rust_decimal::Decimal;

    #[test]
    fn test_store_and_retrieve_transaction() {
        let store = AsyncTransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4), // 1.0000
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        store.store(123, tx.clone());

        let retrieved = store.get(123);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.client, 1);
        assert_eq!(retrieved.amount, Decimal::new(10000, 4));
        assert_eq!(retrieved.tx_type, TransactionType::Deposit);
        assert!(!retrieved.under_dispute);
    }

    #[test]
    fn test_get_nonexistent_transaction() {
        let store = AsyncTransactionStore::new();
        assert!(store.get(999).is_none());
    }

    #[test]
    fn test_store_multiple_transactions() {
        let store = AsyncTransactionStore::new();

        let tx1 = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        let tx2 = StoredTransaction {
            client: 2,
            amount: Decimal::new(20000, 4),
            tx_type: TransactionType::Withdrawal,
            under_dispute: false,
        };

        store.store(1, tx1);
        store.store(2, tx2);

        let retrieved1 = store.get(1).unwrap();
        let retrieved2 = store.get(2).unwrap();

        assert_eq!(retrieved1.client, 1);
        assert_eq!(retrieved1.amount, Decimal::new(10000, 4));
        assert_eq!(retrieved2.client, 2);
        assert_eq!(retrieved2.amount, Decimal::new(20000, 4));
    }

    #[test]
    fn test_update_transaction_dispute_state() {
        let store = AsyncTransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        store.store(123, tx);

        // Mark as disputed
        let result = store.update(123, |tx| {
            tx.under_dispute = true;
            Ok(())
        });

        assert!(result.is_ok());

        // Verify the update
        let updated = store.get(123).unwrap();
        assert!(updated.under_dispute);
    }

    #[test]
    fn test_update_nonexistent_transaction() {
        let store = AsyncTransactionStore::new();

        let result = store.update(999, |tx| {
            tx.under_dispute = true;
            Ok(())
        });

        assert!(result.is_err());
        match result {
            Err(PaymentError::TransactionNotFound { tx, operation }) => {
                assert_eq!(tx, 999);
                assert_eq!(operation, "update");
            }
            _ => panic!("Expected TransactionNotFound error"),
        }
    }

    #[test]
    fn test_update_with_validation_error() {
        let store = AsyncTransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: true, // Already disputed
        };

        store.store(123, tx);

        // Try to dispute again
        let result = store.update(123, |tx| {
            if tx.under_dispute {
                return Err(PaymentError::transaction_already_disputed(123, tx.client));
            }
            tx.under_dispute = true;
            Ok(())
        });

        assert!(result.is_err());
        match result {
            Err(PaymentError::TransactionAlreadyDisputed { tx, client }) => {
                assert_eq!(tx, 123);
                assert_eq!(client, 1);
            }
            _ => panic!("Expected TransactionAlreadyDisputed error"),
        }

        // Verify transaction state unchanged
        let unchanged = store.get(123).unwrap();
        assert!(unchanged.under_dispute);
    }

    #[test]
    fn test_update_resolve_dispute() {
        let store = AsyncTransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: true,
        };

        store.store(123, tx);

        // Resolve the dispute
        let result = store.update(123, |tx| {
            if !tx.under_dispute {
                return Err(PaymentError::transaction_not_disputed(
                    123, tx.client, "resolve",
                ));
            }
            tx.under_dispute = false;
            Ok(())
        });

        assert!(result.is_ok());

        // Verify the update
        let resolved = store.get(123).unwrap();
        assert!(!resolved.under_dispute);
    }

    #[test]
    fn test_store_ignores_duplicate_transaction_id() {
        let store = AsyncTransactionStore::new();

        let tx1 = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        let tx2 = StoredTransaction {
            client: 2,
            amount: Decimal::new(20000, 4),
            tx_type: TransactionType::Withdrawal,
            under_dispute: true,
        };

        store.store(123, tx1);
        store.store(123, tx2); // Should be ignored

        let retrieved = store.get(123).unwrap();
        assert_eq!(retrieved.client, 1); // Should be the first transaction
        assert_eq!(retrieved.amount, Decimal::new(10000, 4));
        assert!(!retrieved.under_dispute);
    }

    #[test]
    fn test_concurrent_access_to_different_transactions() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(AsyncTransactionStore::new());

        // Store initial transactions
        for i in 0u32..10u32 {
            let tx = StoredTransaction {
                client: i as u16,
                amount: Decimal::new(10000 * i as i64, 4),
                tx_type: TransactionType::Deposit,
                under_dispute: false,
            };
            store.store(i, tx);
        }

        // Spawn threads to access different transactions
        let mut handles = vec![];
        for i in 0u32..10u32 {
            let store_clone = Arc::clone(&store);
            let handle = thread::spawn(move || {
                let tx = store_clone.get(i).unwrap();
                assert_eq!(tx.client, i as u16);
                assert_eq!(tx.amount, Decimal::new(10000 * i as i64, 4));
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_updates_to_different_transactions() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(AsyncTransactionStore::new());

        // Store initial transactions
        for i in 0u32..10u32 {
            let tx = StoredTransaction {
                client: i as u16,
                amount: Decimal::new(10000 * i as i64, 4),
                tx_type: TransactionType::Deposit,
                under_dispute: false,
            };
            store.store(i, tx);
        }

        // Spawn threads to update different transactions
        let mut handles = vec![];
        for i in 0u32..10u32 {
            let store_clone = Arc::clone(&store);
            let handle = thread::spawn(move || {
                store_clone
                    .update(i, |tx| {
                        tx.under_dispute = true;
                        Ok(())
                    })
                    .unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all transactions were updated
        for i in 0u32..10u32 {
            let tx = store.get(i).unwrap();
            assert!(tx.under_dispute);
        }
    }
}
