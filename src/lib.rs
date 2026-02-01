//! Rust Payments Engine Library
//! # Overview
//!
//! This library provides a streaming CSV-based transaction processor implementing both sync and an async strategy
//!
//! # Architecture
//!
//! The system is organized into several key components:
//!
//! - [`types`] - Core data types (Account, Transaction, etc.)
//! - [`cli`] - CLI arguments parsing
//! - [`core`] - Business logic components:
//!   - [`core::engine`] - Transaction processing orchestration
//!   - [`core::account_manager`] - Account state management and balance operations
//!   - [`core::transaction_store`] - Transaction history for dispute resolution
//! - [`io`] - I/O handling with pluggable parsing strategies
//!
//! # Transaction Types
//!
//! The engine supports five transaction types:
//!
//! - **Deposit**: Credit funds to an account
//! - **Withdrawal**: Debit funds from an account (requires sufficient available balance)
//! - **Dispute**: Challenge a previous transaction, freezing associated funds
//! - **Resolve**: Release funds from a disputed transaction back to available
//! - **Chargeback**: Reverse a disputed transaction and lock the account
//!
//! # Account States
//!
//! Each account maintains:
//! - `available`: Funds available for withdrawal or trading
//! - `held`: Funds frozen due to disputes
//! - `total`: Sum of available and held funds
//! - `locked`: Whether the account is locked (due to chargeback)

// Module declarations
pub mod cli;
pub mod core;
pub mod io;
pub mod strategy;
pub mod types;

pub use core::{AccountManager, TransactionEngine, TransactionStore};
pub use io::write_accounts_csv;
pub use types::{
    Account, ClientId, PaymentError, StoredTransaction, TransactionId, TransactionRecord,
    TransactionType,
};
