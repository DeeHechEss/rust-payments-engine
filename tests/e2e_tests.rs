//! End-to-end integration tests
//!
//! These tests validate the complete transaction processing pipeline using
//! predefined CSV test fixtures. Each test:
//! 1. Reads input.csv from a fixture directory
//! 2. Processes all transactions through the engine
//! 3. Generates output CSV
//! 4. Compares actual output with expected.csv
//!
//! Test fixtures are located in tests/fixtures/ and cover:
//! - Happy path scenarios
//! - Dispute resolution flows
//! - Chargeback flows
//! - Error conditions (insufficient funds, invalid references, etc.)
//! - Edge cases (precision, boundaries, duplicates, etc.)
//!
//! Each test is run twice: once with the synchronous parser and once with the async parser.

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_payments_engine::cli::StrategyType;
    use rust_payments_engine::strategy::create_strategy;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    /// Run a test fixture by processing input.csv and comparing with expected.csv
    ///
    /// This helper function:
    /// 1. Reads input.csv from tests/fixtures/{fixture_name}/
    /// 2. Processes all transactions using the specified strategy
    /// 3. Generates output CSV to a temporary file
    /// 4. Reads expected.csv from the fixture directory
    /// 5. Compares actual output with expected output (normalized)
    ///
    /// # Arguments
    ///
    /// * `fixture_name` - Name of the fixture directory (e.g., "happy_path")
    /// * `strategy_type` - Parsing strategy to use (Sync or Async)
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Input or expected files cannot be read
    /// - Output doesn't match expected (after normalization)
    fn run_test_fixture(fixture_name: &str, strategy_type: StrategyType) {
        // Construct paths to fixture files
        let fixture_dir = format!("tests/fixtures/{}", fixture_name);
        let input_path = format!("{}/input.csv", fixture_dir);
        let expected_path = format!("{}/expected.csv", fixture_dir);

        // Verify fixture files exist
        assert!(
            Path::new(&input_path).exists(),
            "Input file not found: {}",
            input_path
        );
        assert!(
            Path::new(&expected_path).exists(),
            "Expected file not found: {}",
            expected_path
        );

        // Create processing strategy
        let strategy = create_strategy(strategy_type.clone(), None);

        // Create temporary output file
        let mut temp_output = NamedTempFile::new().expect("Failed to create temp file");

        // Process all transactions using the selected strategy
        strategy
            .process(Path::new(&input_path), &mut temp_output)
            .unwrap_or_else(|e| panic!("Failed to process transactions: {}", e));

        // Flush output
        temp_output.flush().expect("Failed to flush temp file");

        // Read actual output from temp file
        let actual_output = fs::read_to_string(temp_output.path())
            .unwrap_or_else(|e| panic!("Failed to read temp output file: {}", e));

        // Read expected output
        let expected_output = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("Failed to read expected file {}: {}", expected_path, e));

        assert_eq!(
            actual_output, expected_output,
            "\n\nOutput mismatch for fixture: {} (strategy: {:?})\n\nActual output:\n{}\n\nExpected output:\n{}\n",
            fixture_name, strategy_type, actual_output, expected_output
        );
    }

    /// End-to-end test for all fixtures with both parsing strategies
    #[rstest]
    #[case("happy_path")]
    #[case("dispute_resolution")]
    #[case("chargeback_flow")]
    #[case("insufficient_funds")]
    #[case("invalid_references")]
    #[case("non_disputed_references")]
    #[case("locked_account")]
    #[case("precision_testing")]
    #[case("boundary_values")]
    #[case("duplicate_transactions")]
    #[case("multiple_clients")]
    #[case("malformed_data")]
    fn test_fixtures(
        #[case] fixture: &str,
        #[values(StrategyType::Sync, StrategyType::Async)] strategy: StrategyType,
    ) {
        run_test_fixture(fixture, strategy);
    }
}
