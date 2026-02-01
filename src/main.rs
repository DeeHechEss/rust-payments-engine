//! Rust Payments Engine CLI
//!
//! Command-line interface for processing financial transactions from CSV files.
//!
//! # Usage
//!
//! ```bash
//! cargo run -- transactions.csv > accounts.csv
//! cargo run -- --strategy sync transactions.csv > accounts.csv
//! cargo run -- --strategy async transactions.csv > accounts.csv
//! cargo run -- --strategy async --batch-size 2000 --max-concurrent 8 transactions.csv > accounts.csv
//! ```
//!
//! The program reads transaction records from the input CSV file, processes them
//! through the payments engine using the selected processing strategy, and outputs
//! the final account states to stdout.
//!
//! # Processing Strategies
//!
//! - **sync**: Synchronous CSV parsing with single-threaded processing (default)
//! - **async**: Asynchronous batch processing with multi-threaded parallelism
//!
//! # Exit Codes
//!
//! - 0: Success
//! - 1: Error (missing arguments, file not found, file not readable, etc.)

use rust_payments_engine::cli;
use rust_payments_engine::strategy;
use std::process;

fn main() {
    // Parse command-line arguments using clap
    let args = cli::parse_args();

    // Create the appropriate processing strategy based on CLI arguments
    let strategy = {
        let config = if matches!(args.strategy, cli::StrategyType::Async) {
            Some(args.to_batch_config())
        } else {
            None
        };
        strategy::create_strategy(args.strategy, config)
    };

    // Process transactions using the selected strategy
    // Output goes to stdout
    let mut output = std::io::stdout();
    if let Err(e) = strategy.process(&args.input_file, &mut output) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
