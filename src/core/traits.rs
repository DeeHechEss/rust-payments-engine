//! Core traits for account management, transaction storage, and engine operations
//!
//! This module defines the trait abstractions that allow both synchronous and
//! asynchronous implementations to be used interchangeably.

use crate::types::{
    Account, ClientId, PaymentError, StoredTransaction, TransactionId, TransactionRecord,
};

/// Trait for managing account state
///
/// Provides operations for creating accounts, managing balances, and querying account status.
/// Implementations can be synchronous (using HashMap) or asynchronous (using DashMap).
pub trait AccountManager {
    /// Get or create an account for the specified client
    fn get_or_create(&mut self, client_id: ClientId) -> Account;

    /// Update an account using a closure
    fn update<F>(&mut self, client_id: ClientId, f: F) -> Result<(), PaymentError>
    where
        F: FnOnce(&mut Account) -> Result<(), PaymentError>;

    /// Check if an account is locked
    fn is_locked(&self, client_id: ClientId) -> bool;

    /// Get all accounts for final output
    fn get_all_accounts(&self) -> Vec<Account>;
}

/// Trait for storing and retrieving transactions
///
/// Provides operations for storing disputable transactions and managing dispute state.
/// Implementations can be synchronous (using HashMap) or asynchronous (using DashMap).
pub trait TransactionStore {
    /// Store a transaction
    fn store(&mut self, tx_id: TransactionId, transaction: StoredTransaction);

    /// Get a transaction by ID
    fn get(&self, tx_id: TransactionId) -> Option<StoredTransaction>;

    /// Update a transaction using a closure
    fn update<F>(&mut self, tx_id: TransactionId, f: F) -> Result<(), PaymentError>
    where
        F: FnOnce(&mut StoredTransaction) -> Result<(), PaymentError>;
}

/// Trait for processing transactions
///
/// Provides the main transaction processing interface that coordinates between
/// account management and transaction storage.
pub trait TransactionEngine {
    /// Process a single transaction record
    fn process(&mut self, record: TransactionRecord) -> Result<(), PaymentError>;

    /// Get all accounts for output
    fn get_accounts(&self) -> Vec<Account>;
}
