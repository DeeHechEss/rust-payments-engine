//! Transaction processing orchestration for async batch processing
//!
//! This module provides the `AsyncTransactionEngine` struct, which orchestrates
//! transaction processing using thread-safe `AsyncAccountManager` and
//! `AsyncTransactionStore` components.
//!
//! # Design
//!
//! The `AsyncTransactionEngine` coordinates between account management and transaction
//! storage to process all transaction types (deposits, withdrawals, disputes, resolves,
//! and chargebacks). It uses Arc-wrapped components to enable safe sharing across
//! async tasks.
//!
//! # Architecture
//!
//! ```text
//! AsyncTransactionEngine
//!     ├── Arc<AsyncAccountManager>  (thread-safe account state)
//!     └── Arc<AsyncTransactionStore> (thread-safe transaction history)
//! ```
//!
//! # Thread Safety
//!
//! The engine itself is cloneable (via Clone trait) and can be safely shared across
//! multiple async tasks. All internal state is protected by Arc, and the underlying
//! components use DashMap for thread-safe concurrent access.
use std::sync::Arc;

use crate::types::{PaymentError, StoredTransaction};

use super::{AsyncAccountManager, AsyncTransactionStore};

/// Transaction processing orchestrator for async batch processing
///
/// `AsyncTransactionEngine` coordinates transaction processing across thread-safe
/// account management and transaction storage components. It can be cloned and
/// shared across multiple async tasks for concurrent processing.
///
/// # Thread Safety
///
/// The engine is safe to clone and use from multiple threads/tasks concurrently.
/// All operations on accounts and transactions are properly synchronized through
/// the underlying DashMap structures in AsyncAccountManager and AsyncTransactionStore.
#[derive(Debug, Clone)]
pub struct AsyncTransactionEngine {
    /// Thread-safe account state manager
    ///
    /// Wrapped in Arc to enable sharing across async tasks. The AsyncAccountManager
    /// uses DashMap internally for fine-grained locking per account.
    account_manager: Arc<AsyncAccountManager>,

    /// Thread-safe transaction history store
    ///
    /// Wrapped in Arc to enable sharing across async tasks. The AsyncTransactionStore
    /// uses DashMap internally for fine-grained locking per transaction.
    transaction_store: Arc<AsyncTransactionStore>,
}

impl AsyncTransactionEngine {
    /// Create a new AsyncTransactionEngine
    ///
    /// # Arguments
    ///
    /// * `account_manager` - Arc-wrapped AsyncAccountManager for account state management
    /// * `transaction_store` - Arc-wrapped AsyncTransactionStore for transaction history
    ///
    /// # Returns
    ///
    /// A new `AsyncTransactionEngine` that can be cloned and shared across async tasks.
    pub fn new(
        account_manager: Arc<AsyncAccountManager>,
        transaction_store: Arc<AsyncTransactionStore>,
    ) -> Self {
        Self {
            account_manager,
            transaction_store,
        }
    }

    /// Process a deposit transaction
    ///
    /// This method processes a deposit by:
    /// 1. Storing the transaction for potential future disputes
    /// 2. Updating the account balance with checked arithmetic
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record containing deposit details
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the deposit was processed successfully
    /// * `Err(PaymentError::MissingAmount)` - If the amount field is missing
    /// * `Err(PaymentError::ArithmeticOverflow)` - If the deposit would cause overflow
    pub fn process_deposit(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        // Extract amount or return error if missing
        let amount = record
            .amount
            .ok_or_else(|| PaymentError::missing_amount("deposit", record.tx, record.client))?;

        // Check for duplicate transaction ID
        if self.transaction_store.get(record.tx).is_some() {
            return Err(PaymentError::duplicate_transaction(
                record.tx,
                record.client,
            ));
        }

        // Store transaction for potential disputes
        self.transaction_store.store(
            record.tx,
            StoredTransaction {
                client: record.client,
                amount,
                tx_type: record.tx_type,
                under_dispute: false,
            },
        );

        // Update account balance
        self.account_manager.update(record.client, |account| {
            account.available = account
                .available
                .checked_add(amount)
                .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", record.client))?;
            account.total = account
                .total
                .checked_add(amount)
                .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", record.client))?;
            Ok(())
        })
    }

    /// Process a withdrawal transaction
    ///
    /// This method processes a withdrawal by:
    /// 1. Validating sufficient available funds
    /// 2. Storing the transaction for potential future disputes
    /// 3. Updating the account balance with checked arithmetic
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record containing withdrawal details
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the withdrawal was processed successfully
    /// * `Err(PaymentError::MissingAmount)` - If the amount field is missing
    /// * `Err(PaymentError::InsufficientFunds)` - If available funds are insufficient
    /// * `Err(PaymentError::ArithmeticUnderflow)` - If the withdrawal would cause underflow
    pub fn process_withdrawal(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        // Extract amount or return error if missing
        let amount = record
            .amount
            .ok_or_else(|| PaymentError::missing_amount("withdrawal", record.tx, record.client))?;

        // Check for duplicate transaction ID
        if self.transaction_store.get(record.tx).is_some() {
            return Err(PaymentError::duplicate_transaction(
                record.tx,
                record.client,
            ));
        }

        // Capture values before the closure to avoid any potential issues
        let client = record.client;
        let tx = record.tx;
        let tx_type = record.tx_type;

        // Update account balance with checked arithmetic and insufficient funds check
        let update_result = self.account_manager.update(client, |account| {
            // Check for insufficient funds before processing
            if account.available < amount {
                return Err(PaymentError::insufficient_funds(
                    client,
                    account.available,
                    amount,
                ));
            }

            account.available = account
                .available
                .checked_sub(amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("withdrawal", client))?;

            account.total = account
                .total
                .checked_sub(amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("withdrawal", client))?;

            Ok(())
        });

        // Only store transaction if update succeeded
        update_result?;

        // Store transaction for potential disputes (only after successful withdrawal)
        self.transaction_store.store(
            tx,
            StoredTransaction {
                client,
                amount,
                tx_type,
                under_dispute: false,
            },
        );

        Ok(())
    }

    /// Process a dispute transaction
    ///
    /// This method processes a dispute by:
    /// 1. Validating the referenced transaction exists
    /// 2. Validating the client ID matches
    /// 3. Validating the transaction is not already disputed
    /// 4. Marking the transaction as disputed
    /// 5. Moving funds from available to held
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record containing dispute details
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the dispute was processed successfully
    /// * `Err(PaymentError::TransactionNotFound)` - If the referenced transaction doesn't exist
    /// * `Err(PaymentError::ClientMismatch)` - If the client ID doesn't match
    /// * `Err(PaymentError::TransactionAlreadyDisputed)` - If the transaction is already disputed
    /// * `Err(PaymentError::ArithmeticUnderflow)` - If moving funds would cause underflow
    /// * `Err(PaymentError::ArithmeticOverflow)` - If moving funds would cause overflow
    pub fn process_dispute(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        // Get the referenced transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "dispute"))?;

        // Verify client ID matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "dispute",
            ));
        }

        // Mark transaction as disputed (this will fail if already disputed)
        self.transaction_store.update(record.tx, |tx| {
            if tx.under_dispute {
                return Err(PaymentError::transaction_already_disputed(
                    record.tx, tx.client,
                ));
            }
            tx.under_dispute = true;
            Ok(())
        })?;

        // Move funds from available to held
        self.account_manager.update(record.client, |account| {
            account.available = account
                .available
                .checked_sub(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("dispute", record.client))?;
            account.held = account
                .held
                .checked_add(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_overflow("dispute", record.client))?;
            Ok(())
        })
    }

    /// Process a resolve transaction
    ///
    /// This method processes a resolve by:
    /// 1. Validating the referenced transaction exists
    /// 2. Validating the client ID matches
    /// 3. Validating the transaction is currently disputed
    /// 4. Marking the transaction as not disputed
    /// 5. Moving funds from held back to available
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record containing resolve details
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the resolve was processed successfully
    /// * `Err(PaymentError::TransactionNotFound)` - If the referenced transaction doesn't exist
    /// * `Err(PaymentError::ClientMismatch)` - If the client ID doesn't match
    /// * `Err(PaymentError::TransactionNotDisputed)` - If the transaction is not disputed
    /// * `Err(PaymentError::ArithmeticUnderflow)` - If moving funds would cause underflow
    /// * `Err(PaymentError::ArithmeticOverflow)` - If moving funds would cause overflow
    pub fn process_resolve(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        // Get the referenced transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "resolve"))?;

        // Verify client ID matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "resolve",
            ));
        }

        // Verify transaction is disputed
        if !stored_tx.under_dispute {
            return Err(PaymentError::transaction_not_disputed(
                record.tx,
                stored_tx.client,
                "resolve",
            ));
        }

        // Mark transaction as not disputed
        self.transaction_store.update(record.tx, |tx| {
            tx.under_dispute = false;
            Ok(())
        })?;

        // Move funds from held back to available
        self.account_manager.update(record.client, |account| {
            account.held = account
                .held
                .checked_sub(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("resolve", record.client))?;
            account.available = account
                .available
                .checked_add(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_overflow("resolve", record.client))?;
            Ok(())
        })
    }

    /// Process a chargeback transaction
    ///
    /// This method processes a chargeback by:
    /// 1. Validating the referenced transaction exists
    /// 2. Validating the client ID matches
    /// 3. Validating the transaction is currently disputed
    /// 4. Removing held funds and decreasing total
    /// 5. Locking the account
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record containing chargeback details
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the chargeback was processed successfully
    /// * `Err(PaymentError::TransactionNotFound)` - If the referenced transaction doesn't exist
    /// * `Err(PaymentError::ClientMismatch)` - If the client ID doesn't match
    /// * `Err(PaymentError::TransactionNotDisputed)` - If the transaction is not disputed
    /// * `Err(PaymentError::ArithmeticUnderflow)` - If removing funds would cause underflow
    pub fn process_chargeback(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        // Get the referenced transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "chargeback"))?;

        // Verify client ID matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "chargeback",
            ));
        }

        // Verify transaction is disputed
        if !stored_tx.under_dispute {
            return Err(PaymentError::transaction_not_disputed(
                record.tx,
                stored_tx.client,
                "chargeback",
            ));
        }

        // Remove held funds, decrease total, and lock account (atomic operation)
        self.account_manager.update(record.client, |account| {
            account.held = account
                .held
                .checked_sub(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("chargeback", record.client))?;
            account.total = account
                .total
                .checked_sub(stored_tx.amount)
                .ok_or_else(|| PaymentError::arithmetic_underflow("chargeback", record.client))?;
            account.locked = true;
            Ok(())
        })
    }

    /// Process a transaction record by routing to the appropriate handler
    ///
    /// This is the main entry point for processing transactions. It checks if the
    /// account is locked and routes the transaction to the appropriate handler based
    /// on the transaction type.
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record to process
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the transaction was processed successfully
    /// * `Err(PaymentError::AccountLocked)` - If the account is locked
    /// * `Err(...)` - Other errors from specific transaction handlers
    pub fn process_transaction(
        &self,
        record: crate::types::TransactionRecord,
    ) -> Result<(), crate::types::PaymentError> {
        use crate::types::{PaymentError, TransactionType};

        // Check if account is locked (except for dispute-related operations on locked accounts)
        // Disputes, resolves, and chargebacks can be processed on locked accounts
        match record.tx_type {
            TransactionType::Deposit | TransactionType::Withdrawal => {
                if self.account_manager.is_locked(record.client) {
                    return Err(PaymentError::account_locked(record.client));
                }
            }
            TransactionType::Dispute | TransactionType::Resolve | TransactionType::Chargeback => {
                // These can be processed on locked accounts
            }
        }

        // Route to appropriate handler
        match record.tx_type {
            TransactionType::Deposit => self.process_deposit(record),
            TransactionType::Withdrawal => self.process_withdrawal(record),
            TransactionType::Dispute => self.process_dispute(record),
            TransactionType::Resolve => self.process_resolve(record),
            TransactionType::Chargeback => self.process_chargeback(record),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TransactionRecord, TransactionType};
    use rust_decimal::Decimal;

    #[test]
    fn test_new_creates_engine() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());

        let _engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Verify the engine was created (basic smoke test)
        assert!(Arc::strong_count(&account_manager) >= 2); // Original + engine
        assert!(Arc::strong_count(&transaction_store) >= 2); // Original + engine
    }

    #[test]
    fn test_engine_is_cloneable() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());

        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Clone the engine
        let _engine_clone = engine.clone();

        // Verify both engines share the same underlying components
        assert!(Arc::strong_count(&account_manager) >= 3); // Original + engine + clone
        assert!(Arc::strong_count(&transaction_store) >= 3); // Original + engine + clone
    }

    #[test]
    fn test_engine_can_be_shared_across_threads() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());

        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Spawn threads that clone the engine
        let mut handles = vec![];
        for _ in 0..5 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                // Just verify we can access the cloned engine in another thread
                let _engine = engine_clone;
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Test passes if no panics occurred
    }

    #[test]
    fn test_process_deposit_successful() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let record = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };

        let result = engine.process_deposit(record);
        assert!(result.is_ok());

        // Verify account balance updated
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(10000, 4));
        assert!(!account.locked);

        // Verify transaction stored
        let stored_tx = transaction_store.get(1);
        assert!(stored_tx.is_some());
        let stored_tx = stored_tx.unwrap();
        assert_eq!(stored_tx.client, 1);
        assert_eq!(stored_tx.amount, Decimal::new(10000, 4));
        assert_eq!(stored_tx.tx_type, TransactionType::Deposit);
        assert!(!stored_tx.under_dispute);
    }

    #[test]
    fn test_process_deposit_creates_account_if_not_exists() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let record = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 42,
            tx: 1,
            amount: Some(Decimal::new(5000, 4)),
        };

        let result = engine.process_deposit(record);
        assert!(result.is_ok());

        // Verify account was created
        let account = account_manager.get_or_create(42);
        assert_eq!(account.client, 42);
        assert_eq!(account.available, Decimal::new(5000, 4));
        assert_eq!(account.total, Decimal::new(5000, 4));
    }

    #[test]
    fn test_process_deposit_missing_amount() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let record = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: None, // Missing amount
        };

        let result = engine.process_deposit(record);
        assert!(result.is_err());

        match result {
            Err(crate::types::PaymentError::MissingAmount {
                tx_type,
                tx,
                client,
            }) => {
                assert_eq!(tx_type, "deposit");
                assert_eq!(tx, 1);
                assert_eq!(client, 1);
            }
            _ => panic!("Expected MissingAmount error"),
        }

        // Verify no account was created
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);

        // Verify transaction was not stored
        assert!(transaction_store.get(1).is_none());
    }

    #[test]
    fn test_process_deposit_multiple_deposits_same_account() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // First deposit
        let record1 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(record1).unwrap();

        // Second deposit
        let record2 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(5000, 4)),
        };
        engine.process_deposit(record2).unwrap();

        // Verify cumulative balance
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(15000, 4));
        assert_eq!(account.total, Decimal::new(15000, 4));

        // Verify both transactions stored
        assert!(transaction_store.get(1).is_some());
        assert!(transaction_store.get(2).is_some());
    }

    #[test]
    fn test_process_deposit_different_accounts() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit to account 1
        let record1 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(record1).unwrap();

        // Deposit to account 2
        let record2 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 2,
            tx: 2,
            amount: Some(Decimal::new(20000, 4)),
        };
        engine.process_deposit(record2).unwrap();

        // Verify both accounts have correct balances
        let account1 = account_manager.get_or_create(1);
        assert_eq!(account1.available, Decimal::new(10000, 4));

        let account2 = account_manager.get_or_create(2);
        assert_eq!(account2.available, Decimal::new(20000, 4));
    }

    #[test]
    fn test_process_deposit_arithmetic_overflow() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Set account to near maximum value
        account_manager
            .update(1, |account| {
                account.available = Decimal::MAX;
                account.total = Decimal::MAX;
                Ok(())
            })
            .unwrap();

        // Try to deposit more (should overflow)
        let record = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(1, 0)),
        };

        let result = engine.process_deposit(record);
        assert!(result.is_err());

        match result {
            Err(crate::types::PaymentError::ArithmeticOverflow { operation, client }) => {
                assert_eq!(operation, "deposit");
                assert_eq!(client, 1);
            }
            _ => panic!("Expected ArithmeticOverflow error"),
        }

        // Verify account state unchanged
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::MAX);
        assert_eq!(account.total, Decimal::MAX);
    }

    #[test]
    fn test_process_deposit_concurrent_different_accounts() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let mut handles = vec![];

        // Spawn 10 threads, each depositing to a different account
        for i in 0u16..10 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                let record = TransactionRecord {
                    tx_type: TransactionType::Deposit,
                    client: i,
                    tx: i as u32,
                    amount: Some(Decimal::new((i as i64 + 1) * 1000, 4)),
                };
                engine_clone.process_deposit(record).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all accounts have correct balances
        for i in 0u16..10 {
            let account = account_manager.get_or_create(i);
            let expected = Decimal::new((i as i64 + 1) * 1000, 4);
            assert_eq!(account.available, expected);
            assert_eq!(account.total, expected);
        }
    }

    #[test]
    fn test_process_deposit_concurrent_same_account() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let mut handles = vec![];

        // Spawn 100 threads, all depositing to the same account
        for i in 0u32..100 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                let record = TransactionRecord {
                    tx_type: TransactionType::Deposit,
                    client: 1,
                    tx: i,
                    amount: Some(Decimal::new(100, 4)),
                };
                engine_clone.process_deposit(record).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify the account has the correct total (100 deposits * 0.0100 = 1.0000)
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));

        // Verify all transactions were stored
        for i in 0u32..100 {
            assert!(transaction_store.get(i).is_some());
        }
    }

    #[test]
    fn test_process_withdrawal_successful() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // First deposit funds
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        // Then withdraw
        let withdrawal = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(5000, 4)),
        };

        let result = engine.process_withdrawal(withdrawal);
        assert!(result.is_ok());

        // Verify account balance updated
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(5000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(5000, 4));
        assert!(!account.locked);

        // Verify transaction stored
        let stored_tx = transaction_store.get(2);
        assert!(stored_tx.is_some());
        let stored_tx = stored_tx.unwrap();
        assert_eq!(stored_tx.client, 1);
        assert_eq!(stored_tx.amount, Decimal::new(5000, 4));
        assert_eq!(stored_tx.tx_type, TransactionType::Withdrawal);
        assert!(!stored_tx.under_dispute);
    }

    #[test]
    fn test_process_withdrawal_insufficient_funds() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit small amount
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(5000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        // Try to withdraw more than available
        let withdrawal = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(10000, 4)),
        };

        let result = engine.process_withdrawal(withdrawal);
        assert!(result.is_err());

        match result {
            Err(crate::types::PaymentError::InsufficientFunds {
                client,
                available,
                requested,
            }) => {
                assert_eq!(client, 1);
                assert_eq!(available, Decimal::new(5000, 4));
                assert_eq!(requested, Decimal::new(10000, 4));
            }
            _ => panic!("Expected InsufficientFunds error"),
        }

        // Verify account balance unchanged
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(5000, 4));
        assert_eq!(account.total, Decimal::new(5000, 4));

        // Verify transaction was NOT stored (failed withdrawal)
        assert!(transaction_store.get(2).is_none());
    }

    #[test]
    fn test_process_withdrawal_missing_amount() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        let withdrawal = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: None, // Missing amount
        };

        let result = engine.process_withdrawal(withdrawal);
        assert!(result.is_err());

        match result {
            Err(crate::types::PaymentError::MissingAmount {
                tx_type,
                tx,
                client,
            }) => {
                assert_eq!(tx_type, "withdrawal");
                assert_eq!(tx, 1);
                assert_eq!(client, 1);
            }
            _ => panic!("Expected MissingAmount error"),
        }

        // Verify no account changes
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);

        // Verify transaction was not stored
        assert!(transaction_store.get(1).is_none());
    }

    #[test]
    fn test_process_withdrawal_from_empty_account() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Try to withdraw from account with no funds
        let withdrawal = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(5000, 4)),
        };

        let result = engine.process_withdrawal(withdrawal);
        assert!(result.is_err());

        match result {
            Err(crate::types::PaymentError::InsufficientFunds {
                client,
                available,
                requested,
            }) => {
                assert_eq!(client, 1);
                assert_eq!(available, Decimal::ZERO);
                assert_eq!(requested, Decimal::new(5000, 4));
            }
            _ => panic!("Expected InsufficientFunds error"),
        }
    }

    #[test]
    fn test_process_withdrawal_multiple_withdrawals() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit funds
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        // First withdrawal
        let withdrawal1 = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(3000, 4)),
        };
        engine.process_withdrawal(withdrawal1).unwrap();

        // Second withdrawal
        let withdrawal2 = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 3,
            amount: Some(Decimal::new(2000, 4)),
        };
        engine.process_withdrawal(withdrawal2).unwrap();

        // Verify cumulative balance
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(5000, 4));
        assert_eq!(account.total, Decimal::new(5000, 4));

        // Verify both transactions stored
        assert!(transaction_store.get(2).is_some());
        assert!(transaction_store.get(3).is_some());
    }

    #[test]
    fn test_process_withdrawal_different_accounts() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit to both accounts
        let deposit1 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(deposit1).unwrap();

        let deposit2 = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 2,
            tx: 2,
            amount: Some(Decimal::new(20000, 4)),
        };
        engine.process_deposit(deposit2).unwrap();

        // Withdraw from both accounts
        let withdrawal1 = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 3,
            amount: Some(Decimal::new(5000, 4)),
        };
        engine.process_withdrawal(withdrawal1).unwrap();

        let withdrawal2 = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 2,
            tx: 4,
            amount: Some(Decimal::new(8000, 4)),
        };
        engine.process_withdrawal(withdrawal2).unwrap();

        // Verify both accounts have correct balances
        let account1 = account_manager.get_or_create(1);
        assert_eq!(account1.available, Decimal::new(5000, 4));

        let account2 = account_manager.get_or_create(2);
        assert_eq!(account2.available, Decimal::new(12000, 4));
    }

    #[test]
    fn test_process_withdrawal_arithmetic_underflow() {
        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Verify that normal operations don't cause underflow
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        let withdrawal = TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(10000, 4)),
        };

        let result = engine.process_withdrawal(withdrawal);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_withdrawal_concurrent_different_accounts() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit to 10 different accounts
        for i in 0u16..10 {
            let deposit = TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: i,
                tx: i as u32,
                amount: Some(Decimal::new((i as i64 + 1) * 10000, 4)),
            };
            engine.process_deposit(deposit).unwrap();
        }

        let mut handles = vec![];

        // Spawn 10 threads, each withdrawing from a different account
        for i in 0u16..10 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                let withdrawal = TransactionRecord {
                    tx_type: TransactionType::Withdrawal,
                    client: i,
                    tx: (i as u32) + 100,
                    amount: Some(Decimal::new((i as i64 + 1) * 5000, 4)),
                };
                engine_clone.process_withdrawal(withdrawal).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all accounts have correct balances (half withdrawn)
        for i in 0u16..10 {
            let account = account_manager.get_or_create(i);
            let expected = Decimal::new((i as i64 + 1) * 5000, 4);
            assert_eq!(account.available, expected);
            assert_eq!(account.total, expected);
        }
    }

    #[test]
    fn test_process_withdrawal_concurrent_same_account() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit funds
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(Decimal::new(50000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        let mut handles = vec![];

        // Spawn 50 threads, all withdrawing from the same account
        for i in 1u32..=50 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                let withdrawal = TransactionRecord {
                    tx_type: TransactionType::Withdrawal,
                    client: 1,
                    tx: i,
                    amount: Some(Decimal::new(1000, 4)),
                };
                engine_clone.process_withdrawal(withdrawal)
            });
            handles.push(handle);
        }

        // Wait for all threads to complete and collect results
        let mut successful = 0;
        let mut failed = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => successful += 1,
                Err(_) => failed += 1,
            }
        }

        // All 50 withdrawals should succeed (5.0000 - 50 * 0.1000 = 0)
        assert_eq!(successful, 50);
        assert_eq!(failed, 0);

        // Verify the account has zero balance
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);

        // Verify all successful transactions were stored
        let stored_count = (1u32..=50)
            .filter(|&i| transaction_store.get(i).is_some())
            .count();
        assert_eq!(stored_count, 50);
    }

    #[test]
    fn test_process_withdrawal_concurrent_same_account_overdraft_prevention() {
        use std::thread;

        let account_manager = Arc::new(AsyncAccountManager::new());
        let transaction_store = Arc::new(AsyncTransactionStore::new());
        let engine = AsyncTransactionEngine::new(
            Arc::clone(&account_manager),
            Arc::clone(&transaction_store),
        );

        // Deposit a small amount
        let deposit = TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(Decimal::new(10000, 4)),
        };
        engine.process_deposit(deposit).unwrap();

        let mut handles = vec![];

        // Spawn 20 threads, all trying to withdraw 0.1000 (total would be 2.0000)
        for i in 1u32..=20 {
            let engine_clone = engine.clone();
            let handle = thread::spawn(move || {
                let withdrawal = TransactionRecord {
                    tx_type: TransactionType::Withdrawal,
                    client: 1,
                    tx: i,
                    amount: Some(Decimal::new(1000, 4)), // 0.1000 each
                };
                engine_clone.process_withdrawal(withdrawal)
            });
            handles.push(handle);
        }

        // Wait for all threads to complete and collect results
        let mut successful = 0;
        let mut failed = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => successful += 1,
                Err(crate::types::PaymentError::InsufficientFunds { .. }) => failed += 1,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        // Only 10 withdrawals should succeed (1.0000 / 0.1000 = 10)
        assert_eq!(successful, 10);
        assert_eq!(failed, 10);

        // Verify the account has zero balance (all available funds withdrawn)
        let account = account_manager.get_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);

        // Verify no overdraft occurred
        assert!(account.available >= Decimal::ZERO);
    }
}
