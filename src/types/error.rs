//! Error types for the Rust Payments Engine
//!
//! This module defines all error types that can occur during transaction processing.
//! Errors are designed to be descriptive and user-friendly for CLI output.
//!
//! # Error Categories
//!
//! - **File I/O Errors**: File not found, permission denied, etc.
//! - **CSV Parsing Errors**: Malformed CSV, invalid data types, etc.
//! - **Transaction Errors**: Insufficient funds, account locked, invalid references, etc.
//! - **Arithmetic Errors**: Overflow, underflow in balance calculations

use rust_decimal::Decimal;
use thiserror::Error;

/// Main error type for the payments engine
///
/// This enum represents all possible errors that can occur during
/// transaction processing. Each variant includes relevant context
/// to help diagnose and resolve the issue.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PaymentError {
    /// File not found at the specified path
    ///
    /// This is a fatal error that prevents processing from starting.
    #[error("File not found: {path}")]
    FileNotFound {
        /// The path that was not found
        path: String,
    },

    /// I/O error occurred while reading or writing files
    ///
    /// This is typically a fatal error (file permissions, disk full, etc.).
    #[error("I/O error: {message}")]
    IoError {
        /// Description of the I/O error
        message: String,
    },

    /// CSV parsing error occurred
    ///
    /// This is a recoverable error - the malformed record is skipped
    /// and processing continues with the next record.
    #[error("CSV parse error{}: {message}", line.map(|l| format!(" at line {}", l)).unwrap_or_default())]
    ParseError {
        /// Line number where the error occurred (if available)
        line: Option<u64>,
        /// Description of the parsing error
        message: String,
    },

    /// Invalid transaction type encountered
    ///
    /// This is a recoverable error - the invalid transaction is skipped
    /// and processing continues.
    #[error("Invalid transaction type '{tx_type}'{}", tx.map(|t| format!(" for transaction {}", t)).unwrap_or_default())]
    InvalidTransactionType {
        /// The invalid transaction type string
        tx_type: String,
        /// Transaction ID (if available)
        tx: Option<u32>,
    },

    /// Amount field is missing for a transaction that requires it
    ///
    /// Deposits and withdrawals require an amount field.
    /// This is a recoverable error.
    #[error("{tx_type} transaction {tx} for client {client} requires an amount")]
    MissingAmount {
        /// Transaction type that requires an amount
        tx_type: String,
        /// Transaction ID
        tx: u32,
        /// Client ID
        client: u16,
    },

    /// Invalid amount value (negative or malformed)
    ///
    /// This is a recoverable error - the transaction is skipped.
    #[error("Invalid amount '{amount}' for transaction {tx}")]
    InvalidAmount {
        /// The invalid amount string
        amount: String,
        /// Transaction ID
        tx: u32,
    },

    /// Insufficient funds for withdrawal
    ///
    /// This is a recoverable error - the withdrawal is rejected
    /// and the account state remains unchanged.
    #[error(
        "Insufficient funds for client {client}: available {available}, requested {requested}"
    )]
    InsufficientFunds {
        /// Client ID
        client: u16,
        /// Available balance
        available: Decimal,
        /// Requested withdrawal amount
        requested: Decimal,
    },

    /// Account is locked and cannot process transactions
    ///
    /// This is a recoverable error - the transaction is rejected.
    #[error("Account {client} is locked")]
    AccountLocked {
        /// Client ID of the locked account
        client: u16,
    },

    /// Arithmetic overflow would occur
    ///
    /// This is a recoverable error - the transaction is rejected
    /// to maintain account integrity.
    #[error("Arithmetic overflow in {operation} for client {client}")]
    ArithmeticOverflow {
        /// Operation that would overflow
        operation: String,
        /// Client ID
        client: u16,
    },

    /// Arithmetic underflow would occur
    ///
    /// This is a recoverable error - the transaction is rejected
    /// to maintain account integrity.
    #[error("Arithmetic underflow in {operation} for client {client}")]
    ArithmeticUnderflow {
        /// Operation that would underflow
        operation: String,
        /// Client ID
        client: u16,
    },

    /// Transaction not found for dispute operation
    ///
    /// This is a recoverable error - the dispute/resolve/chargeback
    /// is ignored and processing continues.
    #[error("Transaction {tx} not found for {operation}")]
    TransactionNotFound {
        /// Transaction ID that was not found
        tx: u32,
        /// Operation that failed
        operation: String,
    },

    /// Transaction is already under dispute
    ///
    /// This is a recoverable error - the duplicate dispute is ignored.
    #[error("Transaction {tx} for client {client} is already under dispute")]
    TransactionAlreadyDisputed {
        /// Transaction ID
        tx: u32,
        /// Client ID
        client: u16,
    },

    /// Transaction is not under dispute
    ///
    /// This is a recoverable error - the resolve/chargeback is ignored.
    #[error("Transaction {tx} for client {client} is not under dispute ({operation})")]
    TransactionNotDisputed {
        /// Transaction ID
        tx: u32,
        /// Client ID
        client: u16,
        /// Operation that failed
        operation: String,
    },

    /// Client mismatch in dispute operation
    ///
    /// The client ID in the dispute/resolve/chargeback doesn't match
    /// the client ID of the original transaction.
    /// This is a recoverable error - the operation is rejected.
    #[error("Client mismatch for {operation} on transaction {tx}: expected client {expected_client}, got client {actual_client}")]
    ClientMismatch {
        /// Transaction ID
        tx: u32,
        /// Expected client ID (from original transaction)
        expected_client: u16,
        /// Actual client ID (from dispute operation)
        actual_client: u16,
        /// Operation that failed
        operation: String,
    },

    /// Insufficient held funds for operation
    ///
    /// This is a recoverable error - the operation is rejected.
    #[error("Insufficient held funds for {operation} on client {client}: held {held}, requested {requested}")]
    InsufficientHeldFunds {
        /// Client ID
        client: u16,
        /// Held balance
        held: Decimal,
        /// Requested amount
        requested: Decimal,
        /// Operation that failed
        operation: String,
    },

    /// Insufficient available funds for operation
    ///
    /// This is a recoverable error - the operation is rejected.
    #[error("Insufficient available funds for {operation} on client {client}: available {available}, requested {requested}")]
    InsufficientAvailableFunds {
        /// Client ID
        client: u16,
        /// Available balance
        available: Decimal,
        /// Requested amount
        requested: Decimal,
        /// Operation that failed
        operation: String,
    },

    /// Duplicate transaction ID encountered
    ///
    /// Transaction IDs must be unique. This is a recoverable error -
    /// the duplicate transaction is ignored.
    #[error("Duplicate transaction ID {tx} for client {client}")]
    DuplicateTransaction {
        /// Transaction ID that is duplicated
        tx: u32,
        /// Client ID
        client: u16,
    },
}

// Conversion from io::Error to PaymentError
impl From<std::io::Error> for PaymentError {
    fn from(error: std::io::Error) -> Self {
        PaymentError::IoError {
            message: error.to_string(),
        }
    }
}

// Conversion from csv::Error to PaymentError
impl From<csv::Error> for PaymentError {
    fn from(error: csv::Error) -> Self {
        // Extract line number if available
        let line = error.position().map(|pos| pos.line());

        PaymentError::ParseError {
            line,
            message: error.to_string(),
        }
    }
}

// Helper functions for creating common errors

impl PaymentError {
    /// Create an InsufficientFunds error
    pub fn insufficient_funds(client: u16, available: Decimal, requested: Decimal) -> Self {
        PaymentError::InsufficientFunds {
            client,
            available,
            requested,
        }
    }

    /// Create an AccountLocked error
    pub fn account_locked(client: u16) -> Self {
        PaymentError::AccountLocked { client }
    }

    /// Create a TransactionNotFound error
    pub fn transaction_not_found(tx: u32, operation: &str) -> Self {
        PaymentError::TransactionNotFound {
            tx,
            operation: operation.to_string(),
        }
    }

    /// Create a ClientMismatch error
    pub fn client_mismatch(
        tx: u32,
        expected_client: u16,
        actual_client: u16,
        operation: &str,
    ) -> Self {
        PaymentError::ClientMismatch {
            tx,
            expected_client,
            actual_client,
            operation: operation.to_string(),
        }
    }

    /// Create a TransactionAlreadyDisputed error
    pub fn transaction_already_disputed(tx: u32, client: u16) -> Self {
        PaymentError::TransactionAlreadyDisputed { tx, client }
    }

    /// Create a TransactionNotDisputed error
    pub fn transaction_not_disputed(tx: u32, client: u16, operation: &str) -> Self {
        PaymentError::TransactionNotDisputed {
            tx,
            client,
            operation: operation.to_string(),
        }
    }

    /// Create an ArithmeticOverflow error
    pub fn arithmetic_overflow(operation: &str, client: u16) -> Self {
        PaymentError::ArithmeticOverflow {
            operation: operation.to_string(),
            client,
        }
    }

    /// Create an ArithmeticUnderflow error
    pub fn arithmetic_underflow(operation: &str, client: u16) -> Self {
        PaymentError::ArithmeticUnderflow {
            operation: operation.to_string(),
            client,
        }
    }

    /// Create a MissingAmount error
    pub fn missing_amount(tx_type: &str, tx: u32, client: u16) -> Self {
        PaymentError::MissingAmount {
            tx_type: tx_type.to_string(),
            tx,
            client,
        }
    }

    /// Create an InvalidAmount error
    pub fn invalid_amount(amount: &str, tx: u32) -> Self {
        PaymentError::InvalidAmount {
            amount: amount.to_string(),
            tx,
        }
    }

    /// Create an InvalidTransactionType error
    pub fn invalid_transaction_type(tx_type: &str, tx: Option<u32>) -> Self {
        PaymentError::InvalidTransactionType {
            tx_type: tx_type.to_string(),
            tx,
        }
    }

    /// Create an InsufficientHeldFunds error
    pub fn insufficient_held_funds(
        client: u16,
        held: Decimal,
        requested: Decimal,
        operation: &str,
    ) -> Self {
        PaymentError::InsufficientHeldFunds {
            client,
            held,
            requested,
            operation: operation.to_string(),
        }
    }

    /// Create an InsufficientAvailableFunds error
    pub fn insufficient_available_funds(
        client: u16,
        available: Decimal,
        requested: Decimal,
        operation: &str,
    ) -> Self {
        PaymentError::InsufficientAvailableFunds {
            client,
            available,
            requested,
            operation: operation.to_string(),
        }
    }

    /// Create a DuplicateTransaction error
    pub fn duplicate_transaction(tx: u32, client: u16) -> Self {
        PaymentError::DuplicateTransaction { tx, client }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use rust_decimal::Decimal;

    #[rstest]
    #[case::file_not_found(
        PaymentError::FileNotFound { path: "test.csv".to_string() },
        "File not found: test.csv"
    )]
    #[case::io_error(
        PaymentError::IoError { message: "Permission denied".to_string() },
        "I/O error: Permission denied"
    )]
    #[case::parse_error_with_line(
        PaymentError::ParseError { line: Some(42), message: "Invalid field".to_string() },
        "CSV parse error at line 42: Invalid field"
    )]
    #[case::parse_error_without_line(
        PaymentError::ParseError { line: None, message: "Invalid field".to_string() },
        "CSV parse error: Invalid field"
    )]
    #[case::invalid_transaction_type(
        PaymentError::InvalidTransactionType { tx_type: "invalid".to_string(), tx: Some(123) },
        "Invalid transaction type 'invalid' for transaction 123"
    )]
    #[case::missing_amount(
        PaymentError::MissingAmount { tx_type: "deposit".to_string(), tx: 123, client: 1 },
        "deposit transaction 123 for client 1 requires an amount"
    )]
    #[case::insufficient_funds(
        PaymentError::InsufficientFunds { client: 1, available: Decimal::new(5000, 4), requested: Decimal::new(10000, 4) },
        "Insufficient funds for client 1: available 0.5000, requested 1.0000"
    )]
    #[case::account_locked(
        PaymentError::AccountLocked { client: 42 },
        "Account 42 is locked"
    )]
    #[case::arithmetic_overflow(
        PaymentError::ArithmeticOverflow { operation: "deposit".to_string(), client: 1 },
        "Arithmetic overflow in deposit for client 1"
    )]
    #[case::transaction_not_found(
        PaymentError::TransactionNotFound { tx: 999, operation: "dispute".to_string() },
        "Transaction 999 not found for dispute"
    )]
    #[case::client_mismatch(
        PaymentError::ClientMismatch { tx: 123, expected_client: 1, actual_client: 2, operation: "dispute".to_string() },
        "Client mismatch for dispute on transaction 123: expected client 1, got client 2"
    )]
    fn test_error_display(#[case] error: PaymentError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::insufficient_funds(
        PaymentError::insufficient_funds(1, Decimal::new(5000, 4), Decimal::new(10000, 4)),
        PaymentError::InsufficientFunds { client: 1, available: Decimal::new(5000, 4), requested: Decimal::new(10000, 4) }
    )]
    #[case::account_locked(
        PaymentError::account_locked(42),
        PaymentError::AccountLocked { client: 42 }
    )]
    #[case::transaction_not_found(
        PaymentError::transaction_not_found(999, "dispute"),
        PaymentError::TransactionNotFound { tx: 999, operation: "dispute".to_string() }
    )]
    #[case::client_mismatch(
        PaymentError::client_mismatch(123, 1, 2, "dispute"),
        PaymentError::ClientMismatch { tx: 123, expected_client: 1, actual_client: 2, operation: "dispute".to_string() }
    )]
    fn test_helper_functions(#[case] result: PaymentError, #[case] expected: PaymentError) {
        assert_eq!(result, expected);
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let error: PaymentError = io_error.into();
        assert!(matches!(error, PaymentError::IoError { .. }));
        assert_eq!(error.to_string(), "I/O error: Permission denied");
    }
}
