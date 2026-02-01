//! Transaction-related types for the Rust Payments Engine
//!
//! This module defines transaction types, records, and stored transaction data
//! used throughout the system for processing payments and disputes.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Client identifier
///
/// Supports client IDs from 0 to 65,535
pub type ClientId = u16;

/// Transaction identifier
///
/// Supports transaction IDs from 0 to 4,294,967,295
pub type TransactionId = u32;

/// Transaction types supported by the payments engine
///
/// Each variant represents a different operation that can be performed
/// on client accounts. Deposits and withdrawals modify balances directly,
/// while disputes, resolves, and chargebacks manage the dispute lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    /// Credit funds to an account
    ///
    /// Increases both available and total balances by the transaction amount.
    /// Creates a new account if one doesn't exist for the client.
    Deposit,

    /// Debit funds from an account
    ///
    /// Decreases both available and total balances by the transaction amount.
    /// Requires sufficient available funds to succeed.
    Withdrawal,

    /// Challenge a previous transaction, freezing associated funds
    ///
    /// Moves funds from available to held, keeping total unchanged.
    /// References an existing deposit or withdrawal transaction.
    Dispute,

    /// Release funds from a disputed transaction back to available
    ///
    /// Moves funds from held back to available, keeping total unchanged.
    /// Can only be applied to transactions currently under dispute.
    Resolve,

    /// Reverse a disputed transaction and lock the account
    ///
    /// Removes held funds, decreases total, and locks the account.
    /// Can only be applied to transactions currently under dispute.
    Chargeback,
}

/// Input transaction record from CSV
///
/// Represents a single transaction as read from the input CSV file.
/// The amount field is optional because dispute, resolve, and chargeback
/// operations reference existing transactions and don't specify amounts.
#[derive(Debug, Clone)]
pub struct TransactionRecord {
    /// The type of transaction (deposit, withdrawal, dispute, resolve, or chargeback)
    pub tx_type: TransactionType,

    /// The client ID this transaction applies to (u16: 0-65,535)
    pub client: ClientId,

    /// Unique transaction identifier (u32: 0-4,294,967,295)
    pub tx: TransactionId,

    /// Transaction amount with 4 decimal places precision
    ///
    /// Required for deposit and withdrawal transactions.
    /// Should be None for dispute, resolve, and chargeback operations.
    pub amount: Option<Decimal>,
}

/// Stored transaction for dispute resolution
///
/// Only deposits and withdrawals are stored, as these are the only
/// transaction types that can be disputed. This optimizes memory usage
/// by not storing dispute/resolve/chargeback operations.
#[derive(Debug, Clone)]
pub struct StoredTransaction {
    /// The client ID that owns this transaction
    pub client: ClientId,

    /// The transaction amount with 4 decimal places precision
    pub amount: Decimal,

    /// The transaction type (only Deposit or Withdrawal are stored)
    pub tx_type: TransactionType,

    /// Whether this transaction is currently disputed
    ///
    /// Set to true when a dispute is processed, false when resolved.
    /// Used to prevent duplicate disputes and validate resolve/chargeback operations.
    pub under_dispute: bool,
}
