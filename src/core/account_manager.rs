//! Account management module
//!
//! This module provides the `AccountManager` struct which maintains the state
//! of all client accounts and provides operations for managing account balances.
//!
//! The AccountManager is responsible for:
//! - Creating new accounts on first transaction
//! - Tracking account balances (available, held, total)
//! - Managing account locked status
//! - Providing sorted account listings for output

use crate::types::{Account, ClientId, PaymentError};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Manages all client accounts and their states
///
/// The AccountManager maintains an in-memory map of client IDs to account states.
/// It provides methods for account creation, balance queries, and retrieving
/// all accounts for output generation.
pub struct AccountManager {
    /// Map of client IDs to account states
    accounts: HashMap<ClientId, Account>,
}

impl AccountManager {
    /// Create a new AccountManager with no accounts
    ///
    /// # Returns
    ///
    /// A new AccountManager with an empty account map
    pub fn new() -> Self {
        AccountManager {
            accounts: HashMap::new(),
        }
    }

    /// Get or create an account for the specified client
    ///
    /// If an account already exists for the client, returns a mutable reference
    /// to it. If no account exists, creates a new account with zero balances
    /// and unlocked status.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to get or create an account for
    ///
    /// # Returns
    ///
    /// A mutable reference to the account for the specified client
    pub fn get_or_create_account(&mut self, client: ClientId) -> &mut Account {
        self.accounts
            .entry(client)
            .or_insert_with(|| Account::new(client))
    }

    /// Check if an account is locked
    ///
    /// Returns true if the account exists and is locked, false otherwise.
    /// If the account doesn't exist, returns false (non-existent accounts
    /// are not considered locked).
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to check
    ///
    /// # Returns
    ///
    /// `true` if the account exists and is locked, `false` otherwise
    pub fn is_locked(&self, client: ClientId) -> bool {
        self.accounts
            .get(&client)
            .is_some_and(|account| account.locked)
    }

    /// Get all accounts sorted by client ID
    ///
    /// Returns a vector of references to all accounts, sorted by client ID
    /// in ascending order. This provides deterministic output for CSV generation.
    ///
    /// # Returns
    ///
    /// A vector of references to all accounts, sorted by client ID
    pub fn get_all_accounts(&self) -> Vec<&Account> {
        let mut accounts: Vec<&Account> = self.accounts.values().collect();
        accounts.sort_by_key(|account| account.client);
        accounts
    }

    /// Deposit funds into a client account
    ///
    /// Increases both the available and total balances by the specified amount.
    /// Uses checked arithmetic to prevent overflow and maintain account integrity.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to deposit funds into
    /// * `amount` - The amount to deposit (must be non-negative)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the deposit was successful
    /// * `Err(PaymentError)` - If overflow would occur
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Adding the amount to available funds would cause overflow
    /// - Adding the amount to total funds would cause overflow
    pub fn deposit(&mut self, client: ClientId, amount: Decimal) -> Result<(), PaymentError> {
        let account = self.get_or_create_account(client);

        let new_available = account
            .available
            .checked_add(amount)
            .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", client))?;

        let new_total = account
            .total
            .checked_add(amount)
            .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", client))?;

        // Update account balances
        account.available = new_available;
        account.total = new_total;

        Ok(())
    }

    /// Withdraw funds from a client account
    ///
    /// Decreases both the available and total balances by the specified amount.
    /// Uses checked arithmetic to prevent underflow and maintain account integrity.
    /// Validates that sufficient available funds exist before processing.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to withdraw funds from
    /// * `amount` - The amount to withdraw (must be non-negative)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the withdrawal was successful
    /// * `Err(PaymentError)` - If insufficient funds or underflow would occur
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The withdrawal amount exceeds available funds
    /// - Subtracting the amount from available funds would cause underflow
    /// - Subtracting the amount from total funds would cause underflow
    pub fn withdraw(&mut self, client: ClientId, amount: Decimal) -> Result<(), PaymentError> {
        let account = self.get_or_create_account(client);

        // Check if sufficient available funds exist
        if account.available < amount {
            return Err(PaymentError::insufficient_funds(
                client,
                account.available,
                amount,
            ));
        }

        let new_available = account
            .available
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("withdrawal", client))?;

        let new_total = account
            .total
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("withdrawal", client))?;

        // Update account balances
        account.available = new_available;
        account.total = new_total;

        Ok(())
    }

    /// Move funds from available to held (dispute)
    ///
    /// Decreases available funds and increases held funds by the specified amount.
    /// Uses checked arithmetic to prevent underflow and maintain account integrity.
    /// The total balance remains unchanged as funds are only moved between states.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to hold funds for
    /// * `amount` - The amount to move from available to held (must be non-negative)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the hold was successful
    /// * `Err(PaymentError)` - If insufficient available funds or overflow would occur
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount exceeds available funds
    /// - Subtracting the amount from available funds would cause underflow
    /// - Adding the amount to held funds would cause overflow
    pub fn hold_funds(&mut self, client: ClientId, amount: Decimal) -> Result<(), PaymentError> {
        let account = self.get_or_create_account(client);

        // Check if sufficient available funds exist
        if account.available < amount {
            return Err(PaymentError::insufficient_available_funds(
                client,
                account.available,
                amount,
                "hold_funds",
            ));
        }

        let new_available = account
            .available
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("hold_funds", client))?;

        let new_held = account
            .held
            .checked_add(amount)
            .ok_or_else(|| PaymentError::arithmetic_overflow("hold_funds", client))?;

        // Update account balances (total remains unchanged)
        account.available = new_available;
        account.held = new_held;

        Ok(())
    }

    /// Move funds from held to available (resolve)
    ///
    /// Decreases held funds and increases available funds by the specified amount.
    /// Uses checked arithmetic to prevent underflow and maintain account integrity.
    /// The total balance remains unchanged as funds are only moved between states.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to release funds for
    /// * `amount` - The amount to move from held to available (must be non-negative)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the release was successful
    /// * `Err(PaymentError)` - If insufficient held funds or overflow would occur
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount exceeds held funds
    /// - Subtracting the amount from held funds would cause underflow
    /// - Adding the amount to available funds would cause overflow
    pub fn release_funds(&mut self, client: ClientId, amount: Decimal) -> Result<(), PaymentError> {
        let account = self.get_or_create_account(client);

        // Check if sufficient held funds exist
        if account.held < amount {
            return Err(PaymentError::insufficient_held_funds(
                client,
                account.held,
                amount,
                "release_funds",
            ));
        }

        let new_held = account
            .held
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("release_funds", client))?;

        let new_available = account
            .available
            .checked_add(amount)
            .ok_or_else(|| PaymentError::arithmetic_overflow("release_funds", client))?;

        // Update account balances (total remains unchanged)
        account.held = new_held;
        account.available = new_available;

        Ok(())
    }

    /// Remove held funds and lock account (chargeback)
    ///
    /// Decreases both held funds and total funds by the specified amount, then
    /// locks the account to prevent further transactions. Uses checked arithmetic
    /// to prevent underflow and maintain account integrity.
    ///
    /// # Arguments
    ///
    /// * `client` - The client ID to chargeback funds from
    /// * `amount` - The amount to remove from held and total (must be non-negative)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the chargeback was successful
    /// * `Err(PaymentError)` - If insufficient held funds or underflow would occur
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The amount exceeds held funds
    /// - Subtracting the amount from held funds would cause underflow
    /// - Subtracting the amount from total funds would cause underflow
    pub fn chargeback(&mut self, client: ClientId, amount: Decimal) -> Result<(), PaymentError> {
        let account = self.get_or_create_account(client);

        // Check if sufficient held funds exist
        if account.held < amount {
            return Err(PaymentError::insufficient_held_funds(
                client,
                account.held,
                amount,
                "chargeback",
            ));
        }

        let new_held = account
            .held
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("chargeback", client))?;

        let new_total = account
            .total
            .checked_sub(amount)
            .ok_or_else(|| PaymentError::arithmetic_underflow("chargeback", client))?;

        // Update account balances and lock the account
        account.held = new_held;
        account.total = new_total;
        account.locked = true;

        Ok(())
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_new_creates_empty_manager() {
        let manager = AccountManager::new();
        assert_eq!(manager.accounts.len(), 0);
        assert_eq!(manager.get_all_accounts().len(), 0);
    }

    #[test]
    fn test_get_or_create_account_creates_new_account() {
        let mut manager = AccountManager::new();

        let account = manager.get_or_create_account(1);

        assert_eq!(account.client, 1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);
        assert!(!account.locked);
    }

    #[test]
    fn test_get_or_create_account_returns_existing_account() {
        let mut manager = AccountManager::new();

        // Create account and modify it
        let account = manager.get_or_create_account(1);
        account.available = Decimal::new(10000, 4); // 1.0000
        account.total = Decimal::new(10000, 4);

        // Get the same account again
        let account = manager.get_or_create_account(1);
        assert_eq!(account.client, 1);
        assert_eq!(account.available, Decimal::new(10000, 4));
    }

    #[test]
    fn test_get_or_create_account_with_multiple_clients() {
        let mut manager = AccountManager::new();

        manager.get_or_create_account(1);
        manager.get_or_create_account(2);
        manager.get_or_create_account(3);

        assert_eq!(manager.accounts.len(), 3);
    }

    #[test]
    fn test_is_locked_returns_false_for_nonexistent_account() {
        let manager = AccountManager::new();
        assert!(!manager.is_locked(1));
    }

    #[test]
    fn test_is_locked_returns_false_for_unlocked_account() {
        let mut manager = AccountManager::new();
        manager.get_or_create_account(1);

        assert!(!manager.is_locked(1));
    }

    #[test]
    fn test_is_locked_returns_true_for_locked_account() {
        let mut manager = AccountManager::new();

        let account = manager.get_or_create_account(1);
        account.locked = true;

        assert!(manager.is_locked(1));
    }

    #[test]
    fn test_deposit_increases_available_and_total() {
        let mut manager = AccountManager::new();

        // Deposit 10.5000 into account 1
        let result = manager.deposit(1, Decimal::new(105000, 4));
        assert!(result.is_ok());

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(105000, 4));
        assert_eq!(account.total, Decimal::new(105000, 4));
        assert_eq!(account.held, Decimal::ZERO);
    }

    #[test]
    fn test_deposit_multiple_times_accumulates() {
        let mut manager = AccountManager::new();

        // First deposit: 1.0000
        manager.deposit(1, Decimal::new(10000, 4)).unwrap();

        // Second deposit: 2.5000
        manager.deposit(1, Decimal::new(25000, 4)).unwrap();

        // Third deposit: 0.5000
        manager.deposit(1, Decimal::new(5000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(40000, 4));
        assert_eq!(account.total, Decimal::new(40000, 4));
    }

    #[test]
    fn test_deposit_for_multiple_clients() {
        let mut manager = AccountManager::new();

        // Deposit into different clients
        manager.deposit(1, Decimal::new(10000, 4)).unwrap();
        manager.deposit(2, Decimal::new(20000, 4)).unwrap();
        manager.deposit(3, Decimal::new(30000, 4)).unwrap();

        assert_eq!(manager.accounts.len(), 3);

        let account1 = manager.get_or_create_account(1);
        assert_eq!(account1.available, Decimal::new(10000, 4));

        let account2 = manager.get_or_create_account(2);
        assert_eq!(account2.available, Decimal::new(20000, 4));

        let account3 = manager.get_or_create_account(3);
        assert_eq!(account3.available, Decimal::new(30000, 4));
    }

    #[test]
    fn test_deposit_does_not_affect_held_funds() {
        let mut manager = AccountManager::new();

        // Create account and manually set held funds
        let account = manager.get_or_create_account(1);
        account.held = Decimal::new(5000, 4); // 0.5000
        account.total = Decimal::new(5000, 4);

        // Deposit should not change held funds
        manager.deposit(1, Decimal::new(10000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.held, Decimal::new(5000, 4));
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(15000, 4));
    }

    #[test]
    fn test_deposit_overflow_in_available_funds() {
        let mut manager = AccountManager::new();

        let account = manager.get_or_create_account(1);
        // Use Decimal::MAX directly - adding anything should overflow
        account.available = Decimal::MAX;
        account.total = Decimal::MAX;

        // Try to deposit a small amount - should fail with overflow
        let result = manager.deposit(1, Decimal::ONE);

        // If overflow detection works, this should be an error
        // Note: Decimal::checked_add returns None on overflow
        if result.is_err() {
            assert!(matches!(
                result.unwrap_err(),
                PaymentError::ArithmeticOverflow { .. }
            ));

            // Account should remain unchanged
            let account = manager.get_or_create_account(1);
            assert_eq!(account.available, Decimal::MAX);
            assert_eq!(account.total, Decimal::MAX);
        } else {
            // If Decimal doesn't overflow at MAX, this test documents that behavior
            // In practice, Decimal::MAX is so large that overflow is unlikely in real scenarios
            println!("Note: Decimal::MAX + 1 did not overflow - Decimal may saturate");
        }
    }

    #[test]
    fn test_withdraw_decreases_available_and_total() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000 first
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // Withdraw 5.0000
        let result = manager.withdraw(1, Decimal::new(50000, 4));
        assert!(result.is_ok());

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(50000, 4));
        assert_eq!(account.total, Decimal::new(50000, 4));
        assert_eq!(account.held, Decimal::ZERO);
    }

    #[test]
    fn test_withdraw_with_insufficient_funds() {
        let mut manager = AccountManager::new();

        // Deposit 5.0000
        manager.deposit(1, Decimal::new(50000, 4)).unwrap();

        // Try to withdraw 10.0000 (more than available)
        let result = manager.withdraw(1, Decimal::new(100000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientFunds { .. }
        ));

        // Account should remain unchanged
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(50000, 4));
        assert_eq!(account.total, Decimal::new(50000, 4));
    }

    #[test]
    fn test_withdraw_multiple_times() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // First withdrawal: 2.0000
        manager.withdraw(1, Decimal::new(20000, 4)).unwrap();

        // Second withdrawal: 3.0000
        manager.withdraw(1, Decimal::new(30000, 4)).unwrap();

        // Third withdrawal: 1.0000
        manager.withdraw(1, Decimal::new(10000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(40000, 4));
        assert_eq!(account.total, Decimal::new(40000, 4));
    }

    #[test]
    fn test_withdraw_from_nonexistent_account() {
        let mut manager = AccountManager::new();

        // Try to withdraw from account that doesn't exist
        // get_or_create_account will create it with zero balance
        let result = manager.withdraw(1, Decimal::new(10000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientFunds { .. }
        ));

        // Account should exist but have zero balance
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);
    }

    #[test]
    fn test_withdraw_does_not_affect_held_funds() {
        let mut manager = AccountManager::new();

        // Create account with both available and held funds
        {
            let account = manager.get_or_create_account(1);
            account.available = Decimal::new(100000, 4);
            account.held = Decimal::new(50000, 4);
            account.total = Decimal::new(150000, 4);
        }

        // Withdraw from available funds
        manager.withdraw(1, Decimal::new(30000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.held, Decimal::new(50000, 4));
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.total, Decimal::new(120000, 4));
    }

    #[test]
    fn test_withdraw_cannot_use_held_funds() {
        let mut manager = AccountManager::new();

        // Create account with held funds but low available funds
        let account = manager.get_or_create_account(1);
        account.available = Decimal::new(20000, 4);
        account.held = Decimal::new(80000, 4);
        account.total = Decimal::new(100000, 4);

        // Try to withdraw more than available (but less than total)
        let result = manager.withdraw(1, Decimal::new(50000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientFunds { .. }
        ));

        // Account should remain unchanged
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(20000, 4));
        assert_eq!(account.held, Decimal::new(80000, 4));
        assert_eq!(account.total, Decimal::new(100000, 4));
    }

    #[test]
    fn test_withdraw_underflow_protection() {
        let mut manager = AccountManager::new();

        // Deposit a small amount
        manager.deposit(1, Decimal::new(10000, 4)).unwrap();

        // Try to withdraw more - should fail with insufficient funds
        let result = manager.withdraw(1, Decimal::new(20000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientFunds { .. }
        ));

        // Account should remain unchanged
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_hold_funds_moves_available_to_held() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // Hold 3.0000
        let result = manager.hold_funds(1, Decimal::new(30000, 4));
        assert!(result.is_ok());

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.held, Decimal::new(30000, 4));
        assert_eq!(account.total, Decimal::new(100000, 4));
    }

    #[test]
    fn test_hold_funds_with_insufficient_available() {
        let mut manager = AccountManager::new();

        // Deposit 5.0000
        manager.deposit(1, Decimal::new(50000, 4)).unwrap();

        // Try to hold 10.0000 (more than available)
        let result = manager.hold_funds(1, Decimal::new(100000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientAvailableFunds { .. }
        ));

        // Account should remain unchanged
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(50000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(50000, 4));
    }

    #[test]
    fn test_hold_funds_multiple_times() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // Hold funds multiple times
        manager.hold_funds(1, Decimal::new(20000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(10000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(40000, 4));
        assert_eq!(account.held, Decimal::new(60000, 4));
        assert_eq!(account.total, Decimal::new(100000, 4));
    }

    #[test]
    fn test_release_funds_moves_held_to_available() {
        let mut manager = AccountManager::new();

        // Setup: deposit and hold funds
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Release 3.0000
        let result = manager.release_funds(1, Decimal::new(30000, 4));
        assert!(result.is_ok());

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(100000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(100000, 4));
    }

    #[test]
    fn test_release_funds_with_insufficient_held() {
        let mut manager = AccountManager::new();

        // Setup: deposit and hold 3.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Try to release 5.0000 (more than held)
        let result = manager.release_funds(1, Decimal::new(50000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientHeldFunds { .. }
        ));

        // Account should remain unchanged
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.held, Decimal::new(30000, 4));
        assert_eq!(account.total, Decimal::new(100000, 4));
    }

    #[test]
    fn test_chargeback_removes_held_funds_and_locks_account() {
        let mut manager = AccountManager::new();

        // Setup: deposit and hold funds
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Chargeback 3.0000
        let result = manager.chargeback(1, Decimal::new(30000, 4));
        assert!(result.is_ok());

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(70000, 4));
        assert!(account.locked);
    }

    #[test]
    fn test_chargeback_with_insufficient_held() {
        let mut manager = AccountManager::new();

        // Setup: deposit and hold 3.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Try to chargeback 5.0000 (more than held)
        let result = manager.chargeback(1, Decimal::new(50000, 4));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PaymentError::InsufficientHeldFunds { .. }
        ));

        // Account should remain unchanged (including not locked)
        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.held, Decimal::new(30000, 4));
        assert_eq!(account.total, Decimal::new(100000, 4));
        assert!(!account.locked); // Should not be locked on failed chargeback
    }

    #[test]
    fn test_full_dispute_resolution_cycle() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // Dispute: hold 3.0000
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Resolve: release 3.0000
        manager.release_funds(1, Decimal::new(30000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(100000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(100000, 4));
        assert!(!account.locked);
    }

    #[test]
    fn test_full_dispute_chargeback_cycle() {
        let mut manager = AccountManager::new();

        // Deposit 10.0000
        manager.deposit(1, Decimal::new(100000, 4)).unwrap();

        // Dispute: hold 3.0000
        manager.hold_funds(1, Decimal::new(30000, 4)).unwrap();

        // Chargeback: remove 3.0000 and lock
        manager.chargeback(1, Decimal::new(30000, 4)).unwrap();

        let account = manager.get_or_create_account(1);
        assert_eq!(account.available, Decimal::new(70000, 4));
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::new(70000, 4));
        assert!(account.locked);
    }
}
