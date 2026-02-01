//! Benchmark suite for comparing processing strategies
//!
//! This benchmark compares the performance of synchronous and asynchronous
//! processing strategies using the divan benchmarking framework.
//!
//! # Running Benchmarks
//!
//! ```bash
//! # Run all benchmarks
//! cargo bench
//! ```
//!
//! # Benchmark Fixtures
//!
//! Three representative CSV files are used:
//! - `benchmark_small.csv` - Small dataset (100 transactions)
//! - `benchmark_medium.csv` - Medium dataset (1,000 transactions)
//! - `benchmark_large.csv` - Large dataset (1,000,000 transactions)
//!
//! Each fixture includes a mix of:
//! - Deposits and withdrawals
//! - Multiple clients
//! - Dispute resolution flows

use rust_payments_engine::cli::StrategyType;
use rust_payments_engine::strategy::create_strategy;
use rust_payments_engine::strategy::BatchConfig;
use std::path::Path;

fn main() {
    divan::main();
}

/// Benchmark synchronous processing strategy with small dataset (100 transactions)
#[divan::bench]
fn sync_strategy_small() {
    let strategy = create_strategy(StrategyType::Sync, None);
    let path = Path::new("benches/fixtures/benchmark_small.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}

/// Benchmark asynchronous processing strategy with small dataset (100 transactions)
#[divan::bench]
fn async_strategy_small() {
    let strategy = create_strategy(StrategyType::Async, Some(BatchConfig::default()));
    let path = Path::new("benches/fixtures/benchmark_small.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}

/// Benchmark synchronous processing strategy with medium dataset (1,000 transactions)
#[divan::bench]
fn sync_strategy_medium() {
    let strategy = create_strategy(StrategyType::Sync, None);
    let path = Path::new("benches/fixtures/benchmark_medium.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}

/// Benchmark asynchronous processing strategy with medium dataset (1,000 transactions)
#[divan::bench]
fn async_strategy_medium() {
    let strategy = create_strategy(StrategyType::Async, Some(BatchConfig::default()));
    let path = Path::new("benches/fixtures/benchmark_medium.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}

/// Benchmark synchronous processing strategy with large dataset (1,000,000 transactions)
#[divan::bench]
fn sync_strategy_large() {
    let strategy = create_strategy(StrategyType::Sync, None);
    let path = Path::new("benches/fixtures/benchmark_large.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}

/// Benchmark asynchronous processing strategy with large dataset (1,000,000 transactions)
#[divan::bench]
fn async_strategy_large() {
    let strategy = create_strategy(StrategyType::Async, Some(BatchConfig::default()));
    let path = Path::new("benches/fixtures/benchmark_large.csv");
    let mut output = Vec::new();

    strategy
        .process(path, &mut output)
        .expect("Processing failed");
}
