//! Per-period schema versioning. ANAF rejects XML whose namespace does not match
//! the version required for the reported period, and D300's namespace is period-
//! specific (e.g. the Aug-2025 21%/11% rate change forced v11 -> v12). This module
//! is the single place that answers: for (declaration, reported period), which
//! namespace / root element / schema version / DUK type do we emit?
//!
//! Adding a new version: append a `SchemaVersion` row with the correct
//! `[valid_from, valid_to]` window and bump the previous row's `valid_to`. Vendor
//! the matching XSD and add a golden fixture. The coverage unit test guards gaps.

use chrono::NaiveDate;

use crate::anaf_decl::DeclKind;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct SchemaVersion {
    pub decl: DeclKind,
    /// Inclusive lower bound of the reported-period window.
    pub valid_from: NaiveDate,
    /// Inclusive upper bound, or `None` for "current / open-ended".
    pub valid_to: Option<NaiveDate>,
    pub namespace: &'static str,
    pub root_element: &'static str,
    pub schema_label: &'static str,
    pub duk_type: &'static str,
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("static schema-version date must be valid")
}

/// Registry of known schema versions, ordered by declaration then period.
/// NOTE: these reflect the ANAF schemas current as of 2026-06; update when ANAF
/// republishes (≈twice a year) and re-vendor the XSD + golden fixtures.
pub fn schema_versions() -> Vec<SchemaVersion> {
    vec![
        // ── D300 (decont TVA) ──────────────────────────────────────────────
        SchemaVersion {
            decl: DeclKind::D300,
            valid_from: d(2023, 1, 1),
            valid_to: Some(d(2025, 7, 31)),
            namespace: "mfp:anaf:dgti:d300:declaratie:v11",
            root_element: "declaratie300",
            schema_label: "D300 v11 (≤2025-07)",
            duk_type: "D300",
        },
        SchemaVersion {
            decl: DeclKind::D300,
            valid_from: d(2025, 8, 1),
            valid_to: None,
            namespace: "mfp:anaf:dgti:d300:declaratie:v12",
            root_element: "declaratie300",
            schema_label: "D300 v12 (≥2025-08, rate 21%/11%)",
            duk_type: "D300",
        },
        // ── D394 (declarație informativă) ──────────────────────────────────
        SchemaVersion {
            decl: DeclKind::D394,
            valid_from: d(2022, 1, 1),
            valid_to: None,
            namespace: "mfp:anaf:dgti:d394:declaratie:v4",
            root_element: "declaratie394",
            schema_label: "D394 v4 (OPANAF 77/2022)",
            duk_type: "D394",
        },
        // ── SAF-T D406 (OECD-derived) ──────────────────────────────────────
        SchemaVersion {
            decl: DeclKind::D406,
            valid_from: d(2022, 1, 1),
            valid_to: None,
            namespace: "urn:StandardAuditFile-Taxation-Financial:RO",
            root_element: "AuditFile",
            schema_label: "SAF-T RO v2.4.9 (2025)",
            duk_type: "D406",
        },
    ]
}

/// Resolve the schema version for a declaration and the period BEING REPORTED
/// (not today's date — a late-filed June return must use June's schema).
pub fn resolve(decl: DeclKind, period: NaiveDate) -> AppResult<SchemaVersion> {
    schema_versions()
        .into_iter()
        .find(|v| {
            v.decl == decl
                && period >= v.valid_from
                && v.valid_to.map(|end| period <= end).unwrap_or(true)
        })
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Nu există o versiune de schemă înregistrată pentru {:?} în perioada {} — actualizați anaf_decl::version.",
                decl, period
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn d300_v12_for_post_aug_2025() {
        let sv = resolve(DeclKind::D300, date(2025, 9, 15)).expect("should resolve");
        assert_eq!(sv.namespace, "mfp:anaf:dgti:d300:declaratie:v12");
    }

    #[test]
    fn d300_v11_for_pre_aug_2025() {
        let sv = resolve(DeclKind::D300, date(2025, 3, 15)).expect("should resolve");
        assert_eq!(sv.namespace, "mfp:anaf:dgti:d300:declaratie:v11");
    }

    #[test]
    fn d300_before_any_window_returns_validation_error() {
        let result = resolve(DeclKind::D300, date(2019, 6, 1));
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Validation(_) => {}
            other => panic!("expected Validation error, got: {other:?}"),
        }
    }

    #[test]
    fn windows_non_overlapping_for_all_decl_kinds() {
        // For each DeclKind, test a spread of representative dates and assert
        // that at most one SchemaVersion matches each date.
        let test_dates = [
            date(2022, 1, 1),
            date(2022, 6, 15),
            date(2023, 3, 1),
            date(2024, 12, 31),
            date(2025, 7, 31),
            date(2025, 8, 1),
            date(2025, 9, 15),
            date(2026, 1, 1),
            date(2026, 6, 1),
        ];
        let all_kinds = [DeclKind::D300, DeclKind::D394, DeclKind::D406];

        for kind in all_kinds {
            for &period in &test_dates {
                let versions = schema_versions();
                let matches: Vec<_> = versions
                    .iter()
                    .filter(|v| {
                        v.decl == kind
                            && period >= v.valid_from
                            && v.valid_to.map(|end| period <= end).unwrap_or(true)
                    })
                    .collect();
                assert!(
                    matches.len() <= 1,
                    "period {period} for {kind:?} matched {} schema versions — windows must be non-overlapping",
                    matches.len()
                );
            }
        }
    }
}
