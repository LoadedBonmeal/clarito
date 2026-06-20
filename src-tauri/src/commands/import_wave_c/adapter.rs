//! ImportAdapter trait — the contract every source adapter must implement.
//!
//! All adapters are parse-only in W1/W2/W3; the commit engine (W4) reads
//! the returned `StagedData` and persists it to the import_staging_* tables.

use crate::error::AppResult;

use super::{DetectedColumn, ImportInput, ParseCtx, SourceKind, StagedData};

/// Every import source (WinMentor, SmartBill, SAGA, …) implements this trait.
///
/// Contracts:
/// * `parse()` MUST NEVER touch the database. It only reads the input and
///   returns in-memory `StagedData`. DB writes happen in the W4 commit engine.
/// * `parse()` must be infallible for individual records: per-record errors
///   go into `StagedData.warnings` (or a `resolution = ERROR` row), not `Err`.
///   Only structural failures (unreadable file, wrong encoding, empty input)
///   should propagate as `Err`.
/// * `detect_columns()` is optional; it is only called when the UI needs to
///   show a column-confirmation dialog before parse (DEFENSIVE adapters).
pub trait ImportAdapter: Send + Sync {
    /// The source kind this adapter handles.
    fn source(&self) -> SourceKind;

    /// Parse the input into staged (uncommitted) rows. DB-free.
    fn parse(&self, input: &ImportInput, ctx: &ParseCtx) -> AppResult<StagedData>;

    /// Sniff the input and return the discovered column/key names with samples.
    /// Default: returns empty vec (adapters with a fixed key-value schema skip this).
    fn detect_columns(&self, _input: &ImportInput) -> AppResult<Vec<DetectedColumn>> {
        Ok(vec![])
    }
}
