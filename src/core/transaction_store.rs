//! Transaction storage for dispute resolution
//!
//! This module provides the TransactionStore component that maintains a history
//! of disputable transactions (deposits and withdrawals). The store enables
//! dispute, resolve, and chargeback operations by allowing lookup of original
//! transactions by their transaction ID.
//!
//! # Memory Optimization
//!
//! Only deposits and withdrawals are stored, as these are the only transaction
//! types that can be disputed. Dispute, resolve, and chargeback operations are
//! not stored, reducing memory usage.
//!
//! # Duplicate Handling
//!
//! If a duplicate transaction ID is encountered, only the
//! first occurrence is stored. Subsequent transactions with the same ID are ignored.

use crate::types::{PaymentError, StoredTransaction, TransactionId};
use std::collections::HashMap;

/// Transaction store for dispute resolution
///
/// Maintains a HashMap of transaction ID to stored transaction data.
/// Supports storing, retrieving, and updating dispute status of transactions.
pub struct TransactionStore {
    /// Map of transaction ID to stored transaction
    transactions: HashMap<TransactionId, StoredTransaction>,
}

impl TransactionStore {
    /// Create a new empty transaction store
    ///
    /// # Returns
    ///
    /// A new TransactionStore with no stored transactions
    pub fn new() -> Self {
        TransactionStore {
            transactions: HashMap::new(),
        }
    }

    /// Store a disputable transaction (deposit or withdrawal)
    ///
    /// If a transaction with the same ID already exists, the new transaction
    /// is ignored.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The unique transaction identifier
    /// * `tx` - The transaction data to store
    ///
    pub fn store(&mut self, tx_id: TransactionId, tx: StoredTransaction) {
        // Only store if not already present (first occurrence wins)
        self.transactions.entry(tx_id).or_insert(tx);
    }

    /// Get an immutable reference to a stored transaction
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction identifier to lookup
    ///
    /// # Returns
    ///
    /// * `Some(&StoredTransaction)` - If the transaction exists
    /// * `None` - If the transaction ID is not found
    pub fn get(&self, tx_id: TransactionId) -> Option<&StoredTransaction> {
        self.transactions.get(&tx_id)
    }

    /// Get a mutable reference to a stored transaction
    ///
    /// Used for updating dispute status of transactions.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction identifier to lookup
    ///
    /// # Returns
    ///
    /// * `Some(&mut StoredTransaction)` - If the transaction exists
    /// * `None` - If the transaction ID is not found
    pub fn get_mut(&mut self, tx_id: TransactionId) -> Option<&mut StoredTransaction> {
        self.transactions.get_mut(&tx_id)
    }

    /// Mark a transaction as under dispute
    ///
    /// Sets the `under_dispute` flag to true for the specified transaction.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction identifier to mark as disputed
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the transaction was successfully marked as disputed
    /// * `Err(PaymentError)` - If the transaction ID is not found
    /// ```
    pub fn mark_disputed(&mut self, tx_id: TransactionId) -> Result<(), PaymentError> {
        let tx = self
            .get_mut(tx_id)
            .ok_or_else(|| PaymentError::transaction_not_found(tx_id, "mark_disputed"))?;
        tx.under_dispute = true;
        Ok(())
    }

    /// Mark a transaction as resolved (no longer disputed)
    ///
    /// Sets the `under_dispute` flag to false for the specified transaction.
    ///
    /// # Arguments
    ///
    /// * `tx_id` - The transaction identifier to mark as resolved
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the transaction was successfully marked as resolved
    /// * `Err(PaymentError)` - If the transaction ID is not found
    pub fn mark_resolved(&mut self, tx_id: TransactionId) -> Result<(), PaymentError> {
        let tx = self
            .get_mut(tx_id)
            .ok_or_else(|| PaymentError::transaction_not_found(tx_id, "mark_resolved"))?;
        tx.under_dispute = false;
        Ok(())
    }
}

impl Default for TransactionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransactionType;
    use rust_decimal::Decimal;

    #[test]
    fn test_store_and_retrieve_transaction() {
        let mut store = TransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        store.store(1, tx.clone());

        let retrieved = store.get(1);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.client, 1);
        assert_eq!(retrieved.amount, Decimal::new(10000, 4));
        assert_eq!(retrieved.tx_type, TransactionType::Deposit);
        assert!(!retrieved.under_dispute);
    }

    #[test]
    fn test_duplicate_transaction_id_first_wins() {
        let mut store = TransactionStore::new();

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

        // Store first transaction
        store.store(1, tx1);

        // Try to store duplicate with different data
        store.store(1, tx2);

        // First transaction should still be there
        let retrieved = store.get(1).unwrap();
        assert_eq!(retrieved.client, 1);
        assert_eq!(retrieved.amount, Decimal::new(10000, 4));
        assert_eq!(retrieved.tx_type, TransactionType::Deposit);
        assert!(!retrieved.under_dispute);
    }

    #[test]
    fn test_mark_disputed_success() {
        let mut store = TransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        store.store(1, tx);

        // Mark as disputed
        let result = store.mark_disputed(1);
        assert!(result.is_ok());
        assert!(store.get(1).unwrap().under_dispute);
    }

    #[test]
    fn test_mark_disputed_nonexistent_transaction() {
        let mut store = TransactionStore::new();

        // Try to mark non-existent transaction
        let result = store.mark_disputed(999);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotFound { .. }
        ));
    }

    #[test]
    fn test_mark_resolved_success() {
        let mut store = TransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: true,
        };

        store.store(1, tx);

        // Mark as resolved
        let result = store.mark_resolved(1);
        assert!(result.is_ok());
        assert!(!store.get(1).unwrap().under_dispute);
    }

    #[test]
    fn test_mark_resolved_nonexistent_transaction() {
        let mut store = TransactionStore::new();

        // Try to mark non-existent transaction
        let result = store.mark_resolved(999);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotFound { .. }
        ));
    }

    #[test]
    fn test_dispute_state_transitions() {
        let mut store = TransactionStore::new();

        let tx = StoredTransaction {
            client: 1,
            amount: Decimal::new(10000, 4),
            tx_type: TransactionType::Deposit,
            under_dispute: false,
        };

        store.store(1, tx);

        // Initial state: not disputed
        assert!(!store.get(1).unwrap().under_dispute);

        // Mark as disputed
        store.mark_disputed(1).unwrap();
        assert!(store.get(1).unwrap().under_dispute);

        // Mark as resolved
        store.mark_resolved(1).unwrap();
        assert!(!store.get(1).unwrap().under_dispute);

        // Mark as disputed again
        store.mark_disputed(1).unwrap();
        assert!(store.get(1).unwrap().under_dispute);
    }

    #[test]
    fn test_store_multiple_transactions() {
        let mut store = TransactionStore::new();

        // Store multiple transactions
        for i in 1..=10 {
            let tx = StoredTransaction {
                client: i,
                amount: Decimal::new(i as i64 * 1000, 4),
                tx_type: if i % 2 == 0 {
                    TransactionType::Deposit
                } else {
                    TransactionType::Withdrawal
                },
                under_dispute: false,
            };
            store.store(i as u32, tx);
        }

        // Verify all transactions are stored
        for i in 1..=10 {
            let tx = store.get(i as u32);
            assert!(tx.is_some());
            assert_eq!(tx.unwrap().client, i);
        }
    }
}
