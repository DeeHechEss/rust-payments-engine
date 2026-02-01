//! Core business logic module
//!
//! This module contains the core transaction processing components:
//! - `traits` - Trait abstractions for interchangeable implementations
//! - `engine` - Transaction processing orchestration
//! - `account_manager` - Account state management and balance operations
//! - `transaction_store` - Transaction storage for dispute resolution
//! - `async` - Asynchronous implementations (feature-gated)

pub mod account_manager;
pub mod r#async;
pub mod engine;
pub mod traits;
pub mod transaction_store;

pub use account_manager::AccountManager;
pub use engine::TransactionEngine;
pub use r#async::{AsyncAccountManager, AsyncTransactionEngine, AsyncTransactionStore};
pub use transaction_store::TransactionStore;
