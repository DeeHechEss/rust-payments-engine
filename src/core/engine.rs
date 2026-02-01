//! Transaction processing engine
//!
//! This module provides the TransactionEngine that orchestrates transaction processing
//! by coordinating between the AccountManager and TransactionStore components.
//!
//! The engine enforces business rules such as:
//! - Account lock checks before processing transactions
//! - Transaction validation (amounts present, client matching, etc.)
//! - Proper dispute lifecycle management (dispute → resolve/chargeback)

use crate::core::account_manager::AccountManager;
use crate::core::transaction_store::TransactionStore;
use crate::types::{Account, PaymentError, StoredTransaction, TransactionRecord, TransactionType};

/// Transaction processing engine
///
/// Orchestrates transaction processing by coordinating between AccountManager
/// and TransactionStore. Enforces business rules and maintains system invariants.
pub struct TransactionEngine {
    account_manager: AccountManager,
    transaction_store: TransactionStore,
}

impl TransactionEngine {
    /// Create a new TransactionEngine
    ///
    /// Initializes an empty engine with no accounts or stored transactions.
    ///
    /// # Returns
    ///
    /// A new TransactionEngine ready to process transactions
    pub fn new() -> Self {
        TransactionEngine {
            account_manager: AccountManager::new(),
            transaction_store: TransactionStore::new(),
        }
    }

    /// Process a single transaction record
    ///
    /// Routes the transaction to the appropriate handler based on transaction type.
    /// Checks if the account is locked before processing (except for the initial
    /// lock check which happens during chargeback processing).
    ///
    /// # Arguments
    ///
    /// * `record` - The transaction record to process
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the transaction was processed successfully
    /// * `Err(PaymentError)` if the transaction failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The account is locked
    /// - The transaction validation fails
    /// - The account operation fails (insufficient funds, arithmetic overflow, etc.)
    pub fn process(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
        // Check if account is locked (except for chargebacks which lock the account)
        // Note: We check before processing to prevent any operations on locked accounts
        if self.account_manager.is_locked(record.client) {
            return Err(PaymentError::account_locked(record.client));
        }

        match record.tx_type {
            TransactionType::Deposit => self.process_deposit(record),
            TransactionType::Withdrawal => self.process_withdrawal(record),
            TransactionType::Dispute => self.process_dispute(record),
            TransactionType::Resolve => self.process_resolve(record),
            TransactionType::Chargeback => self.process_chargeback(record),
        }
    }

    /// Process a deposit transaction
    ///
    /// Validates the amount is present, checks for duplicate transaction IDs,
    /// updates the account balance, and stores the transaction for potential
    /// future disputes.
    ///
    /// # Arguments
    ///
    /// * `record` - The deposit transaction record
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the deposit was processed successfully
    /// * `Err(PaymentError)` if the deposit failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount field is missing
    /// - The transaction ID is a duplicate (already exists)
    /// - The account operation fails (arithmetic overflow)
    fn process_deposit(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
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

        // Update account
        self.account_manager.deposit(record.client, amount)?;

        // Store transaction for potential disputes
        self.transaction_store.store(
            record.tx,
            StoredTransaction {
                client: record.client,
                amount,
                tx_type: TransactionType::Deposit,
                under_dispute: false,
            },
        );

        Ok(())
    }

    /// Process a withdrawal transaction
    ///
    /// Validates the amount is present, checks for duplicate transaction IDs,
    /// checks for sufficient funds, updates the account balance, and stores
    /// the transaction for potential future disputes.
    ///
    /// # Arguments
    ///
    /// * `record` - The withdrawal transaction record
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the withdrawal was processed successfully
    /// * `Err(PaymentError)` if the withdrawal failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount field is missing
    /// - The transaction ID is a duplicate (already exists)
    /// - Insufficient available funds
    /// - The account operation fails (arithmetic underflow)
    fn process_withdrawal(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
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

        // Update account (will fail if insufficient funds)
        self.account_manager.withdraw(record.client, amount)?;

        // Store transaction for potential disputes
        self.transaction_store.store(
            record.tx,
            StoredTransaction {
                client: record.client,
                amount,
                tx_type: TransactionType::Withdrawal,
                under_dispute: false,
            },
        );

        Ok(())
    }

    /// Process a dispute transaction
    ///
    /// Looks up the original transaction, validates the client matches,
    /// verifies the transaction is not already disputed, holds the funds,
    /// and marks the transaction as disputed.
    ///
    /// # Arguments
    ///
    /// * `record` - The dispute transaction record
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the dispute was processed successfully
    /// * `Err(PaymentError)` if the dispute failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction ID is not found
    /// - The client ID doesn't match the original transaction
    /// - The transaction is already under dispute
    /// - Insufficient available funds to hold
    fn process_dispute(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
        // Look up the original transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "dispute"))?;

        // Verify client matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "dispute",
            ));
        }

        // Verify not already disputed
        if stored_tx.under_dispute {
            return Err(PaymentError::transaction_already_disputed(
                record.tx,
                record.client,
            ));
        }

        // Hold the funds
        self.account_manager
            .hold_funds(record.client, stored_tx.amount)?;

        // Mark as disputed
        self.transaction_store.mark_disputed(record.tx)?;

        Ok(())
    }

    /// Process a resolve transaction
    ///
    /// Looks up the original transaction, validates the client matches,
    /// verifies the transaction is under dispute, releases the held funds,
    /// and marks the transaction as resolved.
    ///
    /// # Arguments
    ///
    /// * `record` - The resolve transaction record
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the resolve was processed successfully
    /// * `Err(PaymentError)` if the resolve failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction ID is not found
    /// - The client ID doesn't match the original transaction
    /// - The transaction is not under dispute
    /// - Insufficient held funds to release
    fn process_resolve(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
        // Look up the original transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "resolve"))?;

        // Verify client matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "resolve",
            ));
        }

        // Verify it's under dispute
        if !stored_tx.under_dispute {
            return Err(PaymentError::transaction_not_disputed(
                record.tx,
                record.client,
                "resolve",
            ));
        }

        // Release the funds
        self.account_manager
            .release_funds(record.client, stored_tx.amount)?;

        // Mark as resolved
        self.transaction_store.mark_resolved(record.tx)?;

        Ok(())
    }

    /// Process a chargeback transaction
    ///
    /// Looks up the original transaction, validates the client matches,
    /// verifies the transaction is under dispute, removes the held funds,
    /// and locks the account.
    ///
    /// # Arguments
    ///
    /// * `record` - The chargeback transaction record
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the chargeback was processed successfully
    /// * `Err(PaymentError)` if the chargeback failed
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction ID is not found
    /// - The client ID doesn't match the original transaction
    /// - The transaction is not under dispute
    /// - Insufficient held funds for chargeback
    fn process_chargeback(&mut self, record: TransactionRecord) -> Result<(), PaymentError> {
        // Look up the original transaction
        let stored_tx = self
            .transaction_store
            .get(record.tx)
            .ok_or_else(|| PaymentError::transaction_not_found(record.tx, "chargeback"))?;

        // Verify client matches
        if stored_tx.client != record.client {
            return Err(PaymentError::client_mismatch(
                record.tx,
                stored_tx.client,
                record.client,
                "chargeback",
            ));
        }

        // Verify it's under dispute
        if !stored_tx.under_dispute {
            return Err(PaymentError::transaction_not_disputed(
                record.tx,
                record.client,
                "chargeback",
            ));
        }

        // Execute chargeback (removes held funds and locks account)
        self.account_manager
            .chargeback(record.client, stored_tx.amount)?;

        Ok(())
    }

    /// Get final account states for output
    ///
    /// Returns a sorted list of all accounts that have been created
    /// during transaction processing.
    ///
    /// # Returns
    ///
    /// A vector of account references sorted by client ID
    pub fn get_accounts(&self) -> Vec<&Account> {
        self.account_manager.get_all_accounts()
    }
}

impl Default for TransactionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_process_deposit_creates_account() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(Decimal::new(10000, 4)), // 1.0000
        });

        assert!(result.is_ok());

        let accounts = engine.get_accounts();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].client, 1);
        assert_eq!(accounts[0].available, Decimal::new(10000, 4));
        assert_eq!(accounts[0].total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_process_deposit_without_amount_fails() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::MissingAmount { .. }
        ));
    }

    #[test]
    fn test_process_withdrawal_with_sufficient_funds() {
        let mut engine = TransactionEngine::new();

        // Deposit 2.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(20000, 4)),
            })
            .unwrap();

        // Withdraw 1.0
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(10000, 4)),
        });

        assert!(result.is_ok());

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::new(10000, 4));
        assert_eq!(accounts[0].total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_process_withdrawal_with_insufficient_funds() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Try to withdraw 2.0
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(20000, 4)),
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientFunds { .. }
        ));

        // Balance should be unchanged
        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::new(10000, 4));
    }

    #[test]
    fn test_process_withdrawal_without_amount_fails() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::MissingAmount { .. }
        ));
    }

    #[test]
    fn test_process_dispute_holds_funds() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute the deposit
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_ok());

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::ZERO);
        assert_eq!(accounts[0].held, Decimal::new(10000, 4));
        assert_eq!(accounts[0].total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_process_dispute_on_nonexistent_transaction() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Dispute,
            client: 1,
            tx: 999,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotFound { .. }
        ));
    }

    #[test]
    fn test_process_dispute_with_client_mismatch() {
        let mut engine = TransactionEngine::new();

        // Deposit for client 1
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Try to dispute as client 2
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Dispute,
            client: 2,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::ClientMismatch { .. }
        ));
    }

    #[test]
    fn test_process_dispute_already_disputed() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute once
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Try to dispute again
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionAlreadyDisputed { .. }
        ));
    }

    #[test]
    fn test_process_resolve_releases_funds() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Resolve
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Resolve,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_ok());

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::new(10000, 4));
        assert_eq!(accounts[0].held, Decimal::ZERO);
        assert_eq!(accounts[0].total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_process_resolve_on_nonexistent_transaction() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Resolve,
            client: 1,
            tx: 999,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotFound { .. }
        ));
    }

    #[test]
    fn test_process_resolve_with_client_mismatch() {
        let mut engine = TransactionEngine::new();

        // Deposit for client 1
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Try to resolve as client 2
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Resolve,
            client: 2,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::ClientMismatch { .. }
        ));
    }

    #[test]
    fn test_process_resolve_on_non_disputed_transaction() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Try to resolve without disputing first
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Resolve,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotDisputed { .. }
        ));
    }

    #[test]
    fn test_process_chargeback_locks_account() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Chargeback
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Chargeback,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_ok());

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::ZERO);
        assert_eq!(accounts[0].held, Decimal::ZERO);
        assert_eq!(accounts[0].total, Decimal::ZERO);
        assert!(accounts[0].locked);
    }

    #[test]
    fn test_process_chargeback_on_nonexistent_transaction() {
        let mut engine = TransactionEngine::new();

        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Chargeback,
            client: 1,
            tx: 999,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotFound { .. }
        ));
    }

    #[test]
    fn test_process_chargeback_with_client_mismatch() {
        let mut engine = TransactionEngine::new();

        // Deposit for client 1
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Try to chargeback as client 2
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Chargeback,
            client: 2,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::ClientMismatch { .. }
        ));
    }

    #[test]
    fn test_process_chargeback_on_non_disputed_transaction() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Try to chargeback without disputing first
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Chargeback,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::TransactionNotDisputed { .. }
        ));
    }

    #[test]
    fn test_locked_account_rejects_deposit() {
        let mut engine = TransactionEngine::new();

        // Setup: deposit, dispute, chargeback to lock account
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Chargeback,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Try to deposit - should fail
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Deposit,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(5000, 4)),
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::AccountLocked { client: 1 }
        ));
    }

    #[test]
    fn test_locked_account_rejects_withdrawal() {
        let mut engine = TransactionEngine::new();

        // Setup: deposit, dispute, chargeback to lock account
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Chargeback,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Try to withdraw - should fail
        let result = engine.process(TransactionRecord {
            tx_type: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(Decimal::new(5000, 4)),
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::AccountLocked { client: 1 }
        ));
    }

    #[test]
    fn test_multiple_clients_independent_accounts() {
        let mut engine = TransactionEngine::new();

        // Client 1: deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Client 2: deposit 2.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 2,
                tx: 2,
                amount: Some(Decimal::new(20000, 4)),
            })
            .unwrap();

        let accounts = engine.get_accounts();
        assert_eq!(accounts.len(), 2);

        // Find accounts by client ID
        let account1 = accounts.iter().find(|a| a.client == 1).unwrap();
        let account2 = accounts.iter().find(|a| a.client == 2).unwrap();

        assert_eq!(account1.available, Decimal::new(10000, 4));
        assert_eq!(account2.available, Decimal::new(20000, 4));
    }

    #[test]
    fn test_full_dispute_resolution_cycle() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Resolve
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Resolve,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::new(10000, 4));
        assert_eq!(accounts[0].held, Decimal::ZERO);
        assert_eq!(accounts[0].total, Decimal::new(10000, 4));
        assert!(!accounts[0].locked);
    }

    #[test]
    fn test_full_chargeback_cycle() {
        let mut engine = TransactionEngine::new();

        // Deposit 1.0
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Deposit,
                client: 1,
                tx: 1,
                amount: Some(Decimal::new(10000, 4)),
            })
            .unwrap();

        // Dispute
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Dispute,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        // Chargeback
        engine
            .process(TransactionRecord {
                tx_type: TransactionType::Chargeback,
                client: 1,
                tx: 1,
                amount: None,
            })
            .unwrap();

        let accounts = engine.get_accounts();
        assert_eq!(accounts[0].available, Decimal::ZERO);
        assert_eq!(accounts[0].held, Decimal::ZERO);
        assert_eq!(accounts[0].total, Decimal::ZERO);
        assert!(accounts[0].locked);
    }
}
