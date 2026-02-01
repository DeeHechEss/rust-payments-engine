//! I/O module
//!
//! Handles CSV parsing and output.
//!
//! # Components
//!
//! - `csv_format` - CSV format handling (record conversion, output serialization)
//! - `sync_reader` - Synchronous CSV reader with iterator interface
//! - `async_reader` - Asynchronous CSV reader with batch reading interface

pub mod async_reader;
pub mod csv_format;
pub mod sync_reader;

pub use async_reader::AsyncReader;
pub use csv_format::{convert_csv_record, write_accounts_csv, CsvRecord};
pub use sync_reader::SyncReader;
