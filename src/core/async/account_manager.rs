//! Thread-safe account management for async batch processing
//!
//! This module provides the `AsyncAccountManager` struct, which manages account states
//! using concurrent data structures to enable safe multi-threaded access.
//!
//! # Design
//!
//! The `AsyncAccountManager` uses `DashMap` (a concurrent HashMap) to provide thread-safe
//! account storage with fine-grained locking. This allows multiple threads to safely
//! access different accounts concurrently while maintaining consistency for operations
//! on the same account.
//!
//! # Thread Safety
//!
//! All operations are thread-safe and prevent data races through DashMap's internal
//! synchronization. The Rust type system ensures that shared references cannot be
//! used to mutate state, and mutable operations are properly synchronized.

use crate::types::{Account, ClientId, PaymentError};
use dashmap::DashMap;

/// Thread-safe account state manager for async batch processing
///
/// `AsyncAccountManager` provides concurrent access to account states using
/// `DashMap` for fine-grained locking. Multiple threads can safely access
/// different accounts simultaneously, while operations on the same account
/// are automatically serialized.
///
/// # Thread Safety
///
/// All methods are safe to call from multiple threads concurrently. The internal
/// `DashMap` ensures that:
/// - Concurrent reads to different accounts don't block each other
/// - Concurrent writes to different accounts don't block each other
/// - Operations on the same account are properly synchronized
///
/// # Performance
///
/// For multi-threaded workloads with many different clients, `AsyncAccountManager`
/// provides excellent scalability. However, for single-threaded workloads or workloads
/// with a single client, the synchronous `AccountManager` is more efficient.
#[derive(Debug)]
pub struct AsyncAccountManager {
    /// Concurrent HashMap storing account states by client ID
    ///
    /// DashMap provides fine-grained locking through internal sharding,
    /// allowing concurrent access to different accounts without global locks.
    accounts: DashMap<ClientId, Account>,
}

impl AsyncAccountManager {
    /// Create a new empty AsyncAccountManager
    ///
    /// # Returns
    ///
    /// A new `AsyncAccountManager` with no accounts. Accounts will be created
    /// on-demand as transactions are processed.
    pub fn new() -> Self {
        Self {
            accounts: DashMap::new(),
        }
    }

    /// Get an existing account or create a new one if it doesn't exist
    ///
    /// This method is thread-safe and can be called concurrently from multiple threads.
    /// If the account doesn't exist, it will be created with zero balances and unlocked status.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID to retrieve or create an account for
    ///
    /// # Returns
    ///
    /// A clone of the account. Note that this is a snapshot at the time of the call;
    /// concurrent modifications by other threads won't be reflected in the returned value.
    ///
    /// # Thread Safety
    ///
    /// Multiple threads can safely call this method concurrently. If multiple threads
    /// attempt to create the same account simultaneously, only one will succeed in
    /// creating it, and all threads will receive the same account.
    pub fn get_or_create(&self, client_id: ClientId) -> Account {
        self.accounts
            .entry(client_id)
            .or_insert_with(|| Account::new(client_id))
            .clone()
    }

    /// Update an account using a closure
    ///
    /// This method provides atomic access to an account for modification. The closure
    /// receives a mutable reference to the account and can modify it. The account is
    /// locked during the closure execution, ensuring no other thread can modify it
    /// concurrently.
    ///
    /// If the account doesn't exist, it will be created before the closure is called.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID of the account to update
    /// * `f` - A closure that receives a mutable reference to the account and returns
    ///   a Result indicating success or failure
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the closure executed successfully
    /// * `Err(PaymentError)` if the closure returned an error
    ///
    /// # Thread Safety
    ///
    /// The closure is executed while holding a lock on the account entry. This ensures
    /// that modifications are atomic and no other thread can observe a partially-updated
    /// account state.
    pub fn update<F>(&self, client_id: ClientId, f: F) -> Result<(), PaymentError>
    where
        F: FnOnce(&mut Account) -> Result<(), PaymentError>,
    {
        let mut entry = self
            .accounts
            .entry(client_id)
            .or_insert_with(|| Account::new(client_id));
        f(entry.value_mut())
    }

    /// Check if an account is locked
    ///
    /// This is a read-only operation that checks the locked status of an account.
    /// If the account doesn't exist, it is considered unlocked (returns false).
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID to check
    ///
    /// # Returns
    ///
    /// * `true` if the account exists and is locked
    /// * `false` if the account doesn't exist or is not locked
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and can be called concurrently. However, the
    /// returned value is a snapshot at the time of the call; the account's locked
    /// status may change immediately after this method returns.
    pub fn is_locked(&self, client_id: ClientId) -> bool {
        self.accounts
            .get(&client_id)
            .map(|acc| acc.locked)
            .unwrap_or(false)
    }

    /// Get all accounts for final output
    ///
    /// This method returns a vector containing clones of all accounts currently
    /// managed by this AsyncAccountManager. The accounts are returned in an
    /// arbitrary order (determined by the internal hash map).
    ///
    /// # Returns
    ///
    /// A vector of all accounts. The vector will be empty if no accounts have
    /// been created.
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and can be called concurrently. However, the
    /// returned vector is a snapshot at the time of the call; accounts may be
    /// created or modified by other threads after this method returns.
    ///
    pub fn get_all_accounts(&self) -> Vec<Account> {
        self.accounts
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Default for AsyncAccountManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_get_or_create_creates_new_account() {
        let manager = AsyncAccountManager::new();

        let account = manager.get_or_create(1);

        assert_eq!(account.client, 1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total, Decimal::ZERO);
        assert!(!account.locked);
    }

    #[test]
    fn test_get_or_create_returns_existing_account() {
        let manager = AsyncAccountManager::new();

        // Create account with some balance
        manager
            .update(1, |account| {
                account.available = Decimal::new(10000, 4);
                account.total = Decimal::new(10000, 4);
                Ok(())
            })
            .unwrap();

        // Get the account
        let account = manager.get_or_create(1);

        assert_eq!(account.client, 1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_update_creates_account_if_not_exists() {
        let manager = AsyncAccountManager::new();

        let result = manager.update(1, |account| {
            account.available = Decimal::new(5000, 4);
            account.total = Decimal::new(5000, 4);
            Ok(())
        });

        assert!(result.is_ok());

        let account = manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(5000, 4));
        assert_eq!(account.total, Decimal::new(5000, 4));
    }

    #[test]
    fn test_update_modifies_existing_account() {
        let manager = AsyncAccountManager::new();

        // Create account
        manager.get_or_create(1);

        // Update it
        let result = manager.update(1, |account| {
            account.available = Decimal::new(10000, 4);
            account.total = Decimal::new(10000, 4);
            Ok(())
        });

        assert!(result.is_ok());

        let account = manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
    }

    #[test]
    fn test_update_returns_error_from_closure() {
        let manager = AsyncAccountManager::new();

        let result = manager.update(1, |_account| Err(PaymentError::account_locked(1)));

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), PaymentError::account_locked(1));
    }

    #[test]
    fn test_is_locked_returns_false_for_nonexistent_account() {
        let manager = AsyncAccountManager::new();

        assert!(!manager.is_locked(1));
    }

    #[test]
    fn test_is_locked_returns_false_for_unlocked_account() {
        let manager = AsyncAccountManager::new();

        manager.get_or_create(1);

        assert!(!manager.is_locked(1));
    }

    #[test]
    fn test_is_locked_returns_true_for_locked_account() {
        let manager = AsyncAccountManager::new();

        manager
            .update(1, |account| {
                account.locked = true;
                Ok(())
            })
            .unwrap();

        assert!(manager.is_locked(1));
    }

    #[test]
    fn test_get_all_accounts_returns_all_accounts() {
        let manager = AsyncAccountManager::new();

        // Create multiple accounts
        manager.get_or_create(1);
        manager.get_or_create(2);
        manager.get_or_create(3);

        let accounts = manager.get_all_accounts();

        assert_eq!(accounts.len(), 3);

        // Verify all client IDs are present
        let client_ids: Vec<u16> = accounts.iter().map(|a| a.client).collect();
        assert!(client_ids.contains(&1));
        assert!(client_ids.contains(&2));
        assert!(client_ids.contains(&3));
    }

    #[test]
    fn test_multiple_updates_on_same_account() {
        let manager = AsyncAccountManager::new();

        // First update
        manager
            .update(1, |account| {
                account.available = Decimal::new(10000, 4);
                account.total = Decimal::new(10000, 4);
                Ok(())
            })
            .unwrap();

        // Second update
        manager
            .update(1, |account| {
                account.available = account
                    .available
                    .checked_add(Decimal::new(5000, 4))
                    .unwrap();
                account.total = account.total.checked_add(Decimal::new(5000, 4)).unwrap();
                Ok(())
            })
            .unwrap();

        let account = manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(15000, 4));
        assert_eq!(account.total, Decimal::new(15000, 4));
    }

    // Concurrent access tests
    // These tests verify that AsyncAccountManager is thread-safe and can handle
    // concurrent operations from multiple threads without data races or inconsistencies.
    #[test]
    fn test_concurrent_get_or_create_different_accounts() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Spawn 10 threads, each creating a different account
        for i in 0..10 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let account = manager_clone.get_or_create(i);
                assert_eq!(account.client, i);
                assert_eq!(account.available, Decimal::ZERO);
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all accounts were created
        assert_eq!(manager.accounts.len(), 10);
    }

    #[test]
    fn test_concurrent_get_or_create_same_account() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Spawn 10 threads, all trying to create the same account
        for _ in 0..10 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let account = manager_clone.get_or_create(1);
                assert_eq!(account.client, 1);
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify only one account was created
        assert_eq!(manager.accounts.len(), 1);
    }

    #[test]
    fn test_concurrent_updates_different_accounts() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Spawn 10 threads, each updating a different account
        for i in 0u16..10 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let amount = Decimal::new(((i + 1) * 1000) as i64, 4);
                manager_clone
                    .update(i, |account| {
                        account.available = amount;
                        account.total = amount;
                        Ok(())
                    })
                    .unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all accounts have correct balances
        for i in 0u16..10 {
            let account = manager.get_or_create(i);
            let expected = Decimal::new(((i + 1) * 1000) as i64, 4);
            assert_eq!(account.available, expected);
            assert_eq!(account.total, expected);
        }
    }

    #[test]
    fn test_concurrent_updates_same_account() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Spawn 100 threads, all incrementing the same account by 100
        for _ in 0..100 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                manager_clone
                    .update(1, |account| {
                        let amount = Decimal::new(100, 4);
                        account.available = account
                            .available
                            .checked_add(amount)
                            .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", 1))?;
                        account.total = account
                            .total
                            .checked_add(amount)
                            .ok_or_else(|| PaymentError::arithmetic_overflow("deposit", 1))?;
                        Ok(())
                    })
                    .unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify the account has the correct total (100 threads * 100 = 10000)
        let account = manager.get_or_create(1);
        assert_eq!(account.available, Decimal::new(10000, 4));
        assert_eq!(account.total, Decimal::new(10000, 4));
    }

    #[test]
    fn test_concurrent_mixed_operations() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Create initial accounts
        for i in 0..5 {
            manager.get_or_create(i);
        }

        // Spawn threads doing various operations
        // - Some threads read accounts
        // - Some threads update accounts
        // - Some threads check lock status
        for i in 0..20 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let client_id = (i % 5) as u16;

                match i % 3 {
                    0 => {
                        // Read operation
                        let account = manager_clone.get_or_create(client_id);
                        assert_eq!(account.client, client_id);
                    }
                    1 => {
                        // Update operation
                        manager_clone
                            .update(client_id, |account| {
                                let amount = Decimal::new(100, 4);
                                account.available = account.available.checked_add(amount).unwrap();
                                account.total = account.total.checked_add(amount).unwrap();
                                Ok(())
                            })
                            .unwrap();
                    }
                    2 => {
                        // Check lock status
                        let _is_locked = manager_clone.is_locked(client_id);
                    }
                    _ => unreachable!(),
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all accounts still exist and maintain invariants
        let accounts = manager.get_all_accounts();
        assert_eq!(accounts.len(), 5);

        for account in accounts {
            // Verify account invariant: total = available + held
            assert_eq!(account.total, account.available + account.held);
        }
    }

    #[test]
    fn test_concurrent_lock_operations() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());
        let mut handles = vec![];

        // Spawn threads that lock and unlock accounts
        for i in 0..10 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let client_id = (i % 3) as u16;

                // Lock the account
                manager_clone
                    .update(client_id, |account| {
                        account.locked = true;
                        Ok(())
                    })
                    .unwrap();

                // Check it's locked
                assert!(manager_clone.is_locked(client_id));

                // Unlock the account
                manager_clone
                    .update(client_id, |account| {
                        account.locked = false;
                        Ok(())
                    })
                    .unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // All accounts should be unlocked at the end
        for i in 0..3 {
            assert!(!manager.is_locked(i));
        }
    }

    #[test]
    fn test_concurrent_get_all_accounts() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(AsyncAccountManager::new());

        // Create some initial accounts
        for i in 0..5 {
            manager.get_or_create(i);
        }

        let mut handles = vec![];

        // Spawn threads that read all accounts while others modify them
        for i in 0..10 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                if i % 2 == 0 {
                    // Read all accounts
                    let accounts = manager_clone.get_all_accounts();
                    assert!(accounts.len() >= 5);
                } else {
                    // Update an account
                    manager_clone
                        .update((i % 5) as u16, |account| {
                            account.available =
                                account.available.checked_add(Decimal::new(100, 4)).unwrap();
                            account.total =
                                account.total.checked_add(Decimal::new(100, 4)).unwrap();
                            Ok(())
                        })
                        .unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify final state
        let accounts = manager.get_all_accounts();
        assert_eq!(accounts.len(), 5);
    }
}
