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
            // DUK _dateVersionTable: v11 covers periods 2025-08…2025-12
            valid_to: Some(d(2025, 12, 31)),
            namespace: "mfp:anaf:dgti:d300:declaratie:v11",
            root_element: "declaratie300",
            schema_label: "D300 v11 (≤2025-12)",
            duk_type: "D300",
        },
        SchemaVersion {
            decl: DeclKind::D300,
            // DUK _dateVersionTable: v12 starts 2026-01-01
            valid_from: d(2026, 1, 1),
            valid_to: None,
            namespace: "mfp:anaf:dgti:d300:declaratie:v12",
            root_element: "declaratie300",
            schema_label: "D300 v12 (≥2026-01)",
            duk_type: "D300",
        },
        // ── D394 (declarație informativă) ──────────────────────────────────
        SchemaVersion {
            decl: DeclKind::D394,
            valid_from: d(2022, 1, 1),
            valid_to: None,
            // Verified against the official XSD (sample_d394.xml targetNamespace):
            // the current schema is v5, not v4.
            namespace: "mfp:anaf:dgti:d394:declaratie:v5",
            root_element: "declaratie394",
            schema_label: "D394 v5 (informatii/rezumat1/rezumat2/op1/op2/op11)",
            duk_type: "D394",
        },
        // ── SAF-T D406 ─────────────────────────────────────────────────────
        SchemaVersion {
            decl: DeclKind::D406,
            valid_from: d(2022, 1, 1),
            valid_to: None,
            // Production DUK namespace is d406 (no trailing 't').
            // The vendored XSD uses d406t as targetNamespace — use the
            // _prod copy (Ro_SAFT_Schema_v249_prod.xsd) for xmllint tests.
            namespace: "mfp:anaf:dgti:d406:declaratie:v1",
            root_element: "AuditFile",
            schema_label: "SAF-T RO v2.4.9 (d406:v1)",
            duk_type: "D406",
        },
        // ── D112 (declarația 112) ──────────────────────────────────────────
        // The D112 EMITTER (d112_xml.rs) hardcodes the namespace …declaratie:v7,
        // CONFIRMED by running the official D112Validator.jar (build 209/Apr-2026):
        // the in-force model REQUIRES :v7 and REJECTS the older :v6 (d112_10102024.xsd).
        // This registry entry is metadata only — the D112 path does NOT call resolve()
        // (the emitter owns its namespace), but we keep it accurate (:v7). The July-2026
        // model (Ordin 605/95/928/2.314/2026) reuses :v7 (nomenclator/rule-level changes).
        SchemaVersion {
            decl: DeclKind::D112,
            valid_from: d(2026, 1, 1),
            valid_to: Some(d(2026, 6, 30)),
            namespace: "mfp:anaf:dgti:declaratie_unica:declaratie:v7",
            root_element: "declaratieUnica",
            schema_label: "D112 v7 (≤2026-06)",
            duk_type: "D112",
        },
        SchemaVersion {
            decl: DeclKind::D112,
            valid_from: d(2026, 7, 1),
            valid_to: None,
            namespace: "mfp:anaf:dgti:declaratie_unica:declaratie:v7",
            root_element: "declaratieUnica",
            schema_label: "D112 v7 (≥2026-07, model Ordin 605/2026)",
            duk_type: "D112",
        },
        // ── D205 (informativă anuală, pe beneficiar) ───────────────────────
        // OPANAF 179/2022 mod. 102/2025. Perioada = ANUL de venit (luna_r=12);
        // resolve cu data de 31 dec a anului de venit. Schema v3.
        SchemaVersion {
            decl: DeclKind::D205,
            valid_from: d(2025, 1, 1),
            valid_to: None,
            namespace: "mfp:anaf:dgti:d205:declaratie:v3",
            root_element: "declaratie205",
            schema_label: "D205 v3 (≥2025, OPANAF 102/2025)",
            duk_type: "D205",
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
    fn d300_v12_for_2026_onward() {
        let sv = resolve(DeclKind::D300, date(2026, 1, 1)).expect("should resolve");
        assert_eq!(sv.namespace, "mfp:anaf:dgti:d300:declaratie:v12");
    }

    #[test]
    fn d300_v11_for_2025() {
        // v11 now covers all of 2025 (including Aug-Dec per DUK _dateVersionTable)
        let sv = resolve(DeclKind::D300, date(2025, 9, 15)).expect("should resolve");
        assert_eq!(sv.namespace, "mfp:anaf:dgti:d300:declaratie:v11");
        let sv2 = resolve(DeclKind::D300, date(2025, 12, 31)).expect("should resolve");
        assert_eq!(sv2.namespace, "mfp:anaf:dgti:d300:declaratie:v11");
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
        let all_kinds = [
            DeclKind::D300,
            DeclKind::D394,
            DeclKind::D406,
            DeclKind::D112,
            DeclKind::D205,
        ];

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
