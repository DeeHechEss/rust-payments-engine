//! Account-related types for the Rust Payments Engine
//!
//! This module defines the Account structure and related functionality
//! for managing client account state.

use super::transaction::ClientId;
use rust_decimal::Decimal;

/// Client account state
///
/// Represents the current state of a client's account, including
/// available funds, held funds (due to disputes), and locked status.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// The client ID (u16: 0-65,535)
    pub client: ClientId,

    /// Funds available for withdrawal or trading
    ///
    /// This is the amount that can be withdrawn or used for transactions.
    /// Calculated as: total - held
    pub available: Decimal,

    /// Funds frozen due to disputes
    ///
    /// When a transaction is disputed, the associated funds are moved from
    /// available to held. They remain held until the dispute is resolved
    /// or charged back.
    pub held: Decimal,

    /// Total funds (available + held)
    ///
    /// This represents the total balance in the account, including both
    /// available and held funds. It only changes during deposits, withdrawals,
    /// and chargebacks (not during disputes or resolves).
    pub total: Decimal,

    /// Whether the account is locked (due to chargeback)
    ///
    /// Once an account is locked, all subsequent transactions are rejected.
    pub locked: bool,
}

impl Account {
    /// Create a new account with zero balances and unlocked status
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID for this account
    ///
    /// # Returns
    ///
    /// A new Account with:
    /// - available = 0.0000
    /// - held = 0.0000
    /// - total = 0.0000
    /// - locked = false
    pub fn new(client: ClientId) -> Self {
        Account {
            client,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            total: Decimal::ZERO,
            locked: false,
        }
    }
}
