use crate::strategy::BatchConfig;
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Process payment transactions with dispute resolution
#[derive(Parser, Debug)]
#[command(name = "payments-engine")]
#[command(about = "Process payment transactions with dispute resolution", long_about = None)]
pub struct CliArgs {
    /// Input CSV file path containing transaction records
    #[arg(value_name = "INPUT", help = "Path to the input CSV file")]
    pub input_file: PathBuf,

    /// Parsing strategy to use for processing transactions
    #[arg(
        long = "strategy",
        value_name = "STRATEGY",
        default_value = "async",
        help = "Parsing strategy: 'sync' for synchronous or 'async' for asynchronous"
    )]
    pub strategy: StrategyType,

    /// Number of transactions per batch (async mode only)
    #[arg(
        long = "batch-size",
        value_name = "SIZE",
        help = "Number of transactions per batch (default: 1000, range: 100-10000)"
    )]
    pub batch_size: Option<usize>,

    /// Maximum number of concurrent batches (async mode only)
    #[arg(
        long = "max-concurrent",
        value_name = "COUNT",
        help = "Maximum number of batches processing concurrently (default: CPU cores)"
    )]
    pub max_concurrent_batches: Option<usize>,
}

/// Available parsing strategies for CSV processing
#[derive(Clone, Debug, ValueEnum)]
pub enum StrategyType {
    Sync,
    Async,
}

impl CliArgs {
    /// Create a BatchConfig from CLI arguments
    ///
    /// This method constructs a BatchConfig using the CLI arguments if provided,
    /// or falls back to default values. It also validates the configuration and
    /// prints warnings to stderr if any issues are detected.
    ///
    /// # Returns
    ///
    /// A `BatchConfig` with values from CLI arguments or defaults.
    pub fn to_batch_config(&self) -> BatchConfig {
        // Use provided values or defaults
        if self.batch_size.is_some() || self.max_concurrent_batches.is_some() {
            // At least one custom value provided, create custom config
            let default = BatchConfig::default();
            BatchConfig::new(
                self.batch_size.unwrap_or(default.batch_size),
                self.max_concurrent_batches
                    .unwrap_or(default.max_concurrent_batches),
            )
        } else {
            // No custom values, use all defaults
            BatchConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // Strategy parsing tests
    #[rstest]
    #[case::default_strategy(&["program", "input.csv"], StrategyType::Async)]
    #[case::explicit_sync(&["program", "--strategy", "sync", "input.csv"], StrategyType::Sync)]
    #[case::explicit_async(&["program", "--strategy", "async", "input.csv"], StrategyType::Async)]
    fn test_strategy_parsing(#[case] args: &[&str], #[case] expected: StrategyType) {
        let parsed = CliArgs::try_parse_from(args).unwrap();
        match (&parsed.strategy, &expected) {
            (StrategyType::Sync, StrategyType::Sync) => (),
            (StrategyType::Async, StrategyType::Async) => (),
            _ => panic!("Expected {:?}, got {:?}", expected, parsed.strategy),
        }
    }

    // Individual config option tests
    #[rstest]
    #[case::batch_size(&["program", "--batch-size", "2000", "input.csv"], Some(2000), None)]
    #[case::max_concurrent(&["program", "--max-concurrent", "8", "input.csv"], None, Some(8))]
    #[case::no_options(&["program", "input.csv"], None, None)]
    #[case::all_options(
        &["program", "--strategy", "async", "--batch-size", "2000", "--max-concurrent", "8", "input.csv"],
        Some(2000),
        Some(8)
    )]
    fn test_config_options(
        #[case] args: &[&str],
        #[case] batch_size: Option<usize>,
        #[case] max_concurrent: Option<usize>,
    ) {
        let parsed = CliArgs::try_parse_from(args).unwrap();
        assert_eq!(parsed.batch_size, batch_size);
        assert_eq!(parsed.max_concurrent_batches, max_concurrent);
    }

    // BatchConfig conversion tests with valid values
    #[rstest]
    #[case::all_defaults(&["program", "input.csv"], 1000, num_cpus::get())]
    #[case::custom_batch_size(&["program", "--batch-size", "2000", "input.csv"], 2000, num_cpus::get())]
    #[case::custom_max_concurrent(&["program", "--max-concurrent", "8", "input.csv"], 1000, 8)]
    #[case::all_custom(
        &["program", "--batch-size", "2000", "--max-concurrent", "8", "input.csv"],
        2000,
        8
    )]
    fn test_batch_config_conversion(
        #[case] args: &[&str],
        #[case] expected_batch_size: usize,
        #[case] expected_max_concurrent: usize,
    ) {
        let parsed = CliArgs::try_parse_from(args).unwrap();
        let config = parsed.to_batch_config();

        assert_eq!(config.batch_size, expected_batch_size);
        assert_eq!(config.max_concurrent_batches, expected_max_concurrent);
    }

    // BatchConfig edge cases - zero values should fall back to defaults
    #[rstest]
    #[case::zero_batch_size(&["program", "--batch-size", "0", "input.csv"], "batch_size", 1000)]
    #[case::zero_max_concurrent(&["program", "--max-concurrent", "0", "input.csv"], "max_concurrent", num_cpus::get())]
    fn test_batch_config_zero_values_fallback(
        #[case] args: &[&str],
        #[case] field: &str,
        #[case] expected_default: usize,
    ) {
        let parsed = CliArgs::try_parse_from(args).unwrap();
        let config = parsed.to_batch_config();

        match field {
            "batch_size" => assert_eq!(config.batch_size, expected_default),
            "max_concurrent" => assert_eq!(config.max_concurrent_batches, expected_default),
            _ => panic!("Unknown field: {}", field),
        }
    }

    // Error handling tests
    #[rstest]
    #[case::missing_input(&["program"])]
    #[case::invalid_strategy(&["program", "--strategy", "invalid", "input.csv"])]
    fn test_parsing_errors(#[case] args: &[&str]) {
        let result = CliArgs::try_parse_from(args);
        assert!(result.is_err());
    }
}
