//! Asynchronous implementations of core components
//!
//! This module provides thread-safe, concurrent implementations of the core
//! transaction processing components using DashMap for locking.
//!
//! # Architecture
//!
//! The async implementations use the same interfaces as the synchronous versions
//! but with concurrent data structures:
//!
//! - **AsyncAccountManager**: Thread-safe account state management using DashMap
//! - **AsyncTransactionStore**: Thread-safe transaction history using DashMap
//! - **AsyncTransactionEngine**: Orchestrates async transaction processing
//!
//! # Thread Safety
//!
//! All components are designed for safe concurrent access:
//! - Operations on different accounts/transactions proceed in parallel
//! - Operations on the same account/transaction are properly synchronized
//! - No global locks - fine-grained locking per entity

pub mod account_manager;
pub mod batch_processor;
pub mod engine;
pub mod transaction_store;

pub use account_manager::AsyncAccountManager;
pub use batch_processor::BatchProcessor;
pub use engine::AsyncTransactionEngine;
pub use transaction_store::AsyncTransactionStore;
