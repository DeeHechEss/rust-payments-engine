//! Types module
//!
//! Contains core data structures used throughout the application.
//! This module organizes types into logical submodules:
//! - `account`: Account-related types
//! - `transaction`: Transaction-related types and identifiers
//! - `error`: Error types for the payments engine

pub mod account;
pub mod error;
pub mod transaction;

pub use account::Account;
pub use error::PaymentError;
pub use transaction::{
    ClientId, StoredTransaction, TransactionId, TransactionRecord, TransactionType,
};
