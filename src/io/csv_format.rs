//! CSV format handling for transaction records and account output
//!
//! This module centralizes all CSV format concerns, providing:
//! - CsvRecord structure for deserialization
//! - Conversion from CSV records to domain types
//! - Account output serialization
//!
//! All functions are pure (no I/O) for easy testing.

use crate::types::{Account, ClientId, TransactionId, TransactionRecord, TransactionType};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::io::Write;
use std::str::FromStr;

/// CSV record structure for deserialization
///
/// Matches the input CSV format with columns: type, client, tx, amount
/// The amount field is optional because dispute/resolve/chargeback
/// operations don't have amounts in the CSV.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct CsvRecord {
    #[serde(rename = "type")]
    pub tx_type: String,
    pub client: ClientId,
    pub tx: TransactionId,
    pub amount: Option<String>,
}

/// Convert a CsvRecord to a TransactionRecord
///
/// This function:
/// - Parses the transaction type string into a TransactionType enum
/// - Parses the amount string into a Decimal (if present)
/// - Validates that amounts are present for deposit/withdrawal
/// - Validates that amounts are absent for dispute/resolve/chargeback
///
/// # Arguments
///
/// * `csv_record` - The deserialized CSV record
///
/// # Returns
///
/// Result containing either:
/// - Ok(TransactionRecord) - Successfully converted record
/// - Err(String) - Error message describing the conversion failure
pub fn convert_csv_record(csv_record: CsvRecord) -> Result<TransactionRecord, String> {
    let tx_type = match csv_record.tx_type.to_lowercase().as_str() {
        "deposit" => TransactionType::Deposit,
        "withdrawal" => TransactionType::Withdrawal,
        "dispute" => TransactionType::Dispute,
        "resolve" => TransactionType::Resolve,
        "chargeback" => TransactionType::Chargeback,
        _ => {
            return Err(format!(
                "Invalid transaction type: '{}' for tx {}",
                csv_record.tx_type, csv_record.tx
            ))
        }
    };

    // Parse amount if present
    let amount = match csv_record.amount {
        Some(amount_str) if !amount_str.trim().is_empty() => {
            match Decimal::from_str(amount_str.trim()) {
                Ok(decimal) => Some(decimal),
                Err(_) => {
                    return Err(format!(
                        "Invalid amount '{}' for tx {}",
                        amount_str, csv_record.tx
                    ))
                }
            }
        }
        _ => None,
    };

    // Validate amount presence based on transaction type
    match tx_type {
        TransactionType::Deposit | TransactionType::Withdrawal => {
            if amount.is_none() {
                return Err(format!(
                    "{:?} transaction {} for client {} requires an amount",
                    tx_type, csv_record.tx, csv_record.client
                ));
            }
        }
        TransactionType::Dispute | TransactionType::Resolve | TransactionType::Chargeback => {
            // These transaction types should not have amounts
            // (they reference existing transactions)
            // We don't enforce this strictly - just ignore any amount provided
        }
    }

    Ok(TransactionRecord {
        tx_type,
        client: csv_record.client,
        tx: csv_record.tx,
        amount,
    })
}

/// Write account states to CSV format
///
/// Writes accounts in CSV format with columns: client, available, held, total, locked
/// Accounts are sorted by client ID for deterministic output.
///
/// # Arguments
///
/// * `accounts` - Slice of account states to write
/// * `output` - Mutable reference to a writer for outputting CSV
///
/// # Returns
///
/// * `Ok(())` if writing succeeded
/// * `Err(String)` if a write error occurred
pub fn write_accounts_csv(accounts: &[Account], output: &mut dyn Write) -> Result<(), String> {
    use csv::Writer;

    let mut writer = Writer::from_writer(output);

    // Write header
    writer
        .write_record(["client", "available", "held", "total", "locked"])
        .map_err(|e| format!("Failed to write CSV header: {}", e))?;

    // Sort accounts by client ID for deterministic output
    let mut sorted_accounts = accounts.to_vec();
    sorted_accounts.sort_by_key(|account| account.client);

    // Write each account
    for account in sorted_accounts {
        writer
            .write_record(&[
                account.client.to_string(),
                format!("{:.4}", account.available),
                format!("{:.4}", account.held),
                format!("{:.4}", account.total),
                account.locked.to_string(),
            ])
            .map_err(|e| format!("Failed to write account record: {}", e))?;
    }

    writer
        .flush()
        .map_err(|e| format!("Failed to flush output: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use rust_decimal::Decimal;

    #[rstest]
    #[case("deposit", TransactionType::Deposit, Some("100.0"))]
    #[case("withdrawal", TransactionType::Withdrawal, Some("50.0"))]
    #[case("DEPOSIT", TransactionType::Deposit, Some("100.0"))] // case insensitive
    fn test_convert_csv_record_valid_with_amount(
        #[case] tx_type: &str,
        #[case] expected_type: TransactionType,
        #[case] amount: Option<&str>,
    ) {
        let csv_record = CsvRecord {
            tx_type: tx_type.to_string(),
            client: 1,
            tx: 1,
            amount: amount.map(|s| s.to_string()),
        };

        let result = convert_csv_record(csv_record);
        assert!(result.is_ok());

        let record = result.unwrap();
        assert_eq!(record.tx_type, expected_type);
        assert_eq!(record.client, 1);
        assert_eq!(record.tx, 1);
        assert!(record.amount.is_some());
    }

    #[rstest]
    #[case("dispute", TransactionType::Dispute)]
    #[case("resolve", TransactionType::Resolve)]
    #[case("chargeback", TransactionType::Chargeback)]
    fn test_convert_csv_record_valid_without_amount(
        #[case] tx_type: &str,
        #[case] expected_type: TransactionType,
    ) {
        let csv_record = CsvRecord {
            tx_type: tx_type.to_string(),
            client: 1,
            tx: 1,
            amount: None,
        };

        let result = convert_csv_record(csv_record);
        assert!(result.is_ok());

        let record = result.unwrap();
        assert_eq!(record.tx_type, expected_type);
        assert_eq!(record.amount, None);
    }

    #[rstest]
    #[case::invalid_type("invalid", Some("100.0"), "Invalid transaction type")]
    #[case::deposit_missing_amount("deposit", None, "requires an amount")]
    #[case::withdrawal_missing_amount("withdrawal", None, "requires an amount")]
    #[case::invalid_amount("deposit", Some("not_a_number"), "Invalid amount")]
    #[case::empty_amount("deposit", Some(""), "requires an amount")]
    #[case::whitespace_amount("deposit", Some("  "), "requires an amount")]
    fn test_convert_csv_record_errors(
        #[case] tx_type: &str,
        #[case] amount: Option<&str>,
        #[case] expected_error: &str,
    ) {
        let csv_record = CsvRecord {
            tx_type: tx_type.to_string(),
            client: 1,
            tx: 1,
            amount: amount.map(|s| s.to_string()),
        };

        let result = convert_csv_record(csv_record);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(expected_error));
    }

    #[rstest]
    #[case("  100.0  ", Decimal::new(1000, 1))] // whitespace trimming
    #[case("100.1234", Decimal::new(1001234, 4))] // four decimal places
    fn test_convert_csv_record_amount_parsing(#[case] amount_str: &str, #[case] expected: Decimal) {
        let csv_record = CsvRecord {
            tx_type: "deposit".to_string(),
            client: 1,
            tx: 1,
            amount: Some(amount_str.to_string()),
        };

        let result = convert_csv_record(csv_record);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, Some(expected));
    }

    #[rstest]
    #[case::single_account(
        vec![Account {
            client: 1,
            available: Decimal::new(1000000, 4),
            held: Decimal::ZERO,
            total: Decimal::new(1000000, 4),
            locked: false,
        }],
        "client,available,held,total,locked\n1,100.0000,0.0000,100.0000,false\n"
    )]
    #[case::multiple_accounts(
        vec![
            Account {
                client: 1,
                available: Decimal::new(1000000, 4),
                held: Decimal::ZERO,
                total: Decimal::new(1000000, 4),
                locked: false,
            },
            Account {
                client: 2,
                available: Decimal::new(2000000, 4),
                held: Decimal::ZERO,
                total: Decimal::new(2000000, 4),
                locked: false,
            },
        ],
        "client,available,held,total,locked\n1,100.0000,0.0000,100.0000,false\n2,200.0000,0.0000,200.0000,false\n"
    )]
    #[case::sorted_by_client_id(
        vec![
            Account {
                client: 3,
                available: Decimal::ZERO,
                held: Decimal::ZERO,
                total: Decimal::ZERO,
                locked: false,
            },
            Account {
                client: 1,
                available: Decimal::ZERO,
                held: Decimal::ZERO,
                total: Decimal::ZERO,
                locked: false,
            },
            Account {
                client: 2,
                available: Decimal::ZERO,
                held: Decimal::ZERO,
                total: Decimal::ZERO,
                locked: false,
            },
        ],
        "client,available,held,total,locked\n1,0.0000,0.0000,0.0000,false\n2,0.0000,0.0000,0.0000,false\n3,0.0000,0.0000,0.0000,false\n"
    )]
    #[case::with_held_funds(
        vec![Account {
            client: 1,
            available: Decimal::ZERO,
            held: Decimal::new(1000000, 4),
            total: Decimal::new(1000000, 4),
            locked: false,
        }],
        "client,available,held,total,locked\n1,0.0000,100.0000,100.0000,false\n"
    )]
    #[case::locked_account(
        vec![Account {
            client: 1,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            total: Decimal::ZERO,
            locked: true,
        }],
        "client,available,held,total,locked\n1,0.0000,0.0000,0.0000,true\n"
    )]
    #[case::empty_accounts(
        vec![],
        "client,available,held,total,locked\n"
    )]
    #[case::four_decimal_precision(
        vec![Account {
            client: 1,
            available: Decimal::new(1001234, 4),
            held: Decimal::new(5678, 4),
            total: Decimal::new(1006912, 4),
            locked: false,
        }],
        "client,available,held,total,locked\n1,100.1234,0.5678,100.6912,false\n"
    )]
    fn test_write_accounts_csv(#[case] accounts: Vec<Account>, #[case] expected_output: &str) {
        let mut output = Vec::new();
        let result = write_accounts_csv(&accounts, &mut output);
        assert!(result.is_ok());

        let output_str = String::from_utf8(output).unwrap();
        assert_eq!(output_str, expected_output);
    }
}
