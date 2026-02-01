// CLI module
// Command-line interface and argument parsing

mod args;

pub use args::{CliArgs, StrategyType};

use clap::Parser;

/// Parse command-line arguments using clap
///
/// This function parses the command-line arguments and returns a `CliArgs` struct
/// containing the parsed values. If parsing fails (e.g., invalid arguments, missing
/// required arguments, or --help flag), clap will automatically display an error
/// message or help text and exit the process.
///
/// # Returns
///
/// Returns a `CliArgs` struct with the parsed command-line arguments.
/// ```
pub fn parse_args() -> CliArgs {
    CliArgs::parse()
}
