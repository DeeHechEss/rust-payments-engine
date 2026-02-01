//! Processing strategy module for transaction processing
//!
//! This module defines the Strategy pattern for complete transaction processing pipelines,
//! encompassing both CSV parsing and transaction engine processing. This allows different
//! processing implementations (synchronous, asynchronous batch) to be selected at runtime.

use crate::cli::StrategyType;
use std::io::Write;
use std::path::Path;

pub mod r#async;
pub mod sync;

pub use self::r#async::{AsyncProcessingStrategy, BatchConfig};
pub use sync::SyncProcessingStrategy;

/// Processing strategy trait for complete transaction processing pipelines
///
/// This trait defines the interface for different transaction processing implementations.
/// Each strategy must be able to read transactions from a CSV file, process them through
/// the appropriate engine, and write the final account states to output.
pub trait ProcessingStrategy: Send + Sync {
    /// Process transactions from input file and write results to output
    ///
    /// This method reads transaction records from the specified CSV file, processes
    /// them through the appropriate transaction engine, and writes the final account
    /// states to the provided output writer.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input CSV file containing transaction records
    /// * `output` - Mutable reference to a writer for outputting account states
    ///
    /// # Returns
    ///
    /// * `Ok(())` if all processing completed successfully (or with recoverable errors)
    /// * `Err(String)` if a fatal error occurred (file not found, I/O error, etc.)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The input file cannot be opened (file not found, permission denied)
    /// - A fatal I/O error occurs during reading or writing
    /// - The CSV structure is fundamentally invalid
    /// - Output cannot be written
    ///
    /// Individual transaction processing errors should be logged to stderr but
    /// should not cause this method to return an error. Processing should continue
    /// with the next transaction.
    fn process(&self, input_path: &Path, output: &mut dyn Write) -> Result<(), String>;
}

/// Create a processing strategy based on the specified strategy type
///
/// This factory function implements the Strategy pattern by selecting and
/// instantiating the appropriate processing strategy implementation at runtime
/// based on the provided strategy type and optional configuration.
///
/// # Arguments
///
/// * `strategy_type` - The type of processing strategy to create (Sync or Async)
/// * `config` - Optional configuration for async batch processing (ignored for sync)
///
/// # Returns
///
/// A boxed trait object implementing the ProcessingStrategy trait
pub fn create_strategy(
    strategy_type: StrategyType,
    config: Option<crate::strategy::BatchConfig>,
) -> Box<dyn ProcessingStrategy> {
    match strategy_type {
        StrategyType::Sync => Box::new(SyncProcessingStrategy),
        StrategyType::Async => {
            let config = config.unwrap_or_default();
            Box::new(AsyncProcessingStrategy::new(config))
        }
    }
}
