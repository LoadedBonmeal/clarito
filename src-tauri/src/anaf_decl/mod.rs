//! Official ANAF declaration generators (D300, D394, SAF-T D406) + the
//! per-period schema-version layer and the DUKIntegrator validation harness.
//!
//! Every generator here emits schema-conformant XML via the `quick_xml::Writer`
//! pattern (see `ubl/generator.rs`), NOT the legacy hand-rolled string builders
//! in `commands/{declarations,d394,saft}.rs`.

pub mod bilant_xml;
pub mod cash_vat;
pub mod d100;
pub mod d100_xml;
pub mod d101;
pub mod d101_xml;
pub mod d112;
pub mod d112_xml;
pub mod d205_xml;
pub mod d207_xml;
pub mod d300;
pub mod d301_xml;
pub mod d390;
pub mod d394;
pub mod d700_xml;
pub mod d710_xml;
pub mod duk;
pub mod etransport;
pub mod etva;
pub mod form_versions;
pub mod preflight;
pub mod saft;
pub mod validation;
pub mod version;
pub mod xml;

/// The official declarations this module targets. `as_duk_type` returns the
/// token ANAF's validator CLI expects. NOTE: D300/D394/D406 share the bundled
/// DUKIntegrator kit; D112 and D205 are each validated by a SEPARATE validator
/// (D112Validator / D205Validator), so their harness wiring routes to a distinct
/// jar (see `validation.rs`).
///
/// D301 and D700 are DUKIntegrator OVERLAY validators: `lib/D301Validator.jar` /
/// `lib/D700Validator.jar` are dispatched via `java -jar DUKIntegrator.jar -v D301 …`
/// / `-v D700 …` (the standard `-v` overlay path in `run_java_validator`).
///
/// D710 este un validator OVERLAY (ca D301/D700): `lib/D710Validator.jar` este apelat
/// PRIN `DUKIntegrator.jar -v D710 <xml> <result>` (NU direct, NU standalone).
/// `run_duk` rutează D710 prin același `run_java_validator` ca D301/D700.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    D300,
    D394,
    D406,
    D112,
    /// D205 — declarația informativă anuală, pe beneficiar (impozit reținut la sursă; cap. dividende).
    D205,
    /// D301 — decont special TVA (OPANAF 592/2016). Overlay DUKIntegrator: lib/D301Validator.jar.
    D301,
    /// D700 — declarație înregistrare/mențiuni/radiere (OPANAF 15/2026). Overlay: lib/D700Validator.jar.
    D700,
    /// D710 — declarație rectificativă D100 (OPANAF 587/2016). Overlay: lib/D710Validator.jar via DUKIntegrator `-v D710`.
    D710,
    /// D100 — declarație privind obligațiile de plată la bugetul de stat (OPANAF 57/2026).
    /// Overlay DUKIntegrator: lib/D100Validator.jar via `java -jar DUKIntegrator.jar -v D100`.
    D100,
    /// D101 — declarație privind impozitul pe profit (OPANAF 206/2025).
    /// Overlay DUKIntegrator: lib/D101Validator.jar via `java -jar DUKIntegrator.jar -v D101`.
    D101,
}

impl DeclKind {
    pub fn as_duk_type(self) -> &'static str {
        match self {
            DeclKind::D300 => "D300",
            DeclKind::D394 => "D394",
            DeclKind::D406 => "D406",
            DeclKind::D112 => "D112",
            DeclKind::D205 => "D205",
            DeclKind::D301 => "D301",
            DeclKind::D700 => "D700",
            DeclKind::D710 => "D710",
            DeclKind::D100 => "D100",
            DeclKind::D101 => "D101",
        }
    }

    /// Returns `true` for declarations whose validator jar is invoked standalone
    /// (`java -jar <jar> <xml>`) rather than via the DUKIntegrator overlay
    /// (`java -jar DUKIntegrator.jar -v <TYPE> <xml> <result>`).
    ///
    /// NOTE: All current declarations (D301, D700, D710) use the DUKIntegrator OVERLAY path.
    /// D710 was previously mistakenly classified as standalone — confirmed to use `-v D710`.
    pub fn is_standalone_validator(self) -> bool {
        false // All declarations use DUKIntegrator overlay; no current standalone validators.
    }
}

// ── Shared whole-lei rounding ────────────────────────────────────────────────

/// Rotunjește o sumă Decimal la lei întregi (i64), COMERCIAL (half away from zero) — convenția
/// ANAF pentru toate declarațiile cu sume în lei întregi (bilanț, D300, D390...).
pub(crate) fn round_lei(d: rust_decimal::Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

// ── Shared XML escaping ──────────────────────────────────────────────────────

/// Escape pentru conținut XML (text + atribute): elimină caracterele de control (ILEGALE în
/// XML 1.0 — un nume cu \u{0b} ar invalida tot fișierul) și escapează & < > " '.
/// Folosit de TOATE generatoarele hand-rolled (d112, bilanț, SAF-T, d300, d394); quick-xml
/// escapează singur.
pub(crate) fn xml_esc(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&apos;".chars().collect(),
            other => vec![other],
        })
        .collect()
}

// ── CUI mod-11 checksum ──────────────────────────────────────────────────────

/// Validate a Romanian CUI (Cod Unic de Înregistrare) using the official
/// mod-11 algorithm extracted from the ANAF validator (dec.DECValidatorRoot).
///
/// Algorithm (from decompiled `checkCUI`):
/// 1. Strip leading "RO" prefix (case-insensitive) and whitespace.
/// 2. Must be 2–10 digits, not starting with "0".
/// 3. Left-pad to 10 digits with zeros.
/// 4. Weighted sum of positions 0–8 using weights [7,5,3,2,1,7,5,3,2].
/// 5. control = (sum × 10) % 11; if control == 10 → control = 0.
/// 6. Valid iff control == digit[9] (last digit).
///
/// Returns `true` if the CUI is structurally valid (passes checksum).
/// Returns `false` for non-numeric, out-of-range, or wrong-checksum values.
/// Returns `true` for an empty string (omitted CUI — caller decides policy).
pub fn valid_cui(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() {
        return true; // absent CUI — not our job to reject here
    }
    // Strip optional "RO" prefix
    let s = if s.len() >= 2 && s[..2].eq_ignore_ascii_case("ro") {
        s[2..].trim()
    } else {
        s
    };
    if s.is_empty() {
        return false;
    }
    let n = s.len();
    if !(2..=10).contains(&n) {
        return false;
    }
    if s.starts_with('0') {
        return false;
    }
    // All chars must be ASCII digits
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let digits: Vec<u8> = s.bytes().map(|b| b - b'0').collect();
    // Left-pad to 10
    let mut padded = [0u8; 10];
    let offset = 10 - n;
    for (i, &d) in digits.iter().enumerate() {
        padded[offset + i] = d;
    }
    let weights: [u32; 9] = [7, 5, 3, 2, 1, 7, 5, 3, 2];
    let sum: u32 = padded[..9]
        .iter()
        .zip(weights.iter())
        .map(|(&d, &w)| d as u32 * w)
        .sum();
    let mut ctrl = (sum * 10) % 11;
    if ctrl == 10 {
        ctrl = 0;
    }
    ctrl == padded[9] as u32
}

/// Compute the check digit for a CUI base (all digits except the last).
///
/// `base` must be a non-empty string of 1–9 digits not starting with '0'.
/// Returns the check digit (0–9), or 0 on invalid input.
pub fn cui_check_digit(base: &str) -> u8 {
    let s = base.trim();
    let n = s.len();
    if n == 0 || n > 9 || !s.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    let digits: Vec<u8> = s.bytes().map(|b| b - b'0').collect();
    let mut padded = [0u8; 9];
    let offset = 9 - n;
    for (i, &d) in digits.iter().enumerate() {
        padded[offset + i] = d;
    }
    // weights for positions 0..8 = positions of the 9-digit padded form
    let weights: [u32; 9] = [7, 5, 3, 2, 1, 7, 5, 3, 2];
    let sum: u32 = padded
        .iter()
        .zip(weights.iter())
        .map(|(&d, &w)| d as u32 * w)
        .sum();
    let mut ctrl = (sum * 10) % 11;
    if ctrl == 10 {
        ctrl = 0;
    }
    ctrl as u8
}

/// Validate a Romanian CNP (Cod Numeric Personal): exactly 13 digits with the official mod-11
/// control digit (weights 279146358279; if the weighted-sum mod 11 == 10 the control digit is 1).
/// ANAF's D112 validator rejects malformed CNPs, so we guard before serializing. An empty string
/// returns false (a CNP is required for an insured person).
pub fn valid_cnp(raw: &str) -> bool {
    let s = raw.trim();
    if s.len() != 13 || !s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let d: Vec<u32> = s.bytes().map(|b| (b - b'0') as u32).collect();
    const W: [u32; 12] = [2, 7, 9, 1, 4, 6, 3, 5, 8, 2, 7, 9];
    let sum: u32 = (0..12).map(|i| d[i] * W[i]).sum();
    let mut ctrl = sum % 11;
    if ctrl == 10 {
        ctrl = 1;
    }
    ctrl == d[12]
}

#[cfg(test)]
mod decl_kind_tests {
    use super::*;

    #[test]
    fn d301_as_duk_type_round_trip() {
        assert_eq!(DeclKind::D301.as_duk_type(), "D301");
    }

    #[test]
    fn d700_as_duk_type_round_trip() {
        assert_eq!(DeclKind::D700.as_duk_type(), "D700");
    }

    #[test]
    fn d710_as_duk_type_round_trip() {
        assert_eq!(DeclKind::D710.as_duk_type(), "D710");
    }

    #[test]
    fn d100_as_duk_type_round_trip() {
        assert_eq!(DeclKind::D100.as_duk_type(), "D100");
    }

    #[test]
    fn d101_as_duk_type_round_trip() {
        assert_eq!(DeclKind::D101.as_duk_type(), "D101");
    }

    #[test]
    fn no_validators_are_standalone() {
        // D710 was previously misclassified as standalone; confirmed via DUK that it uses
        // the standard DUKIntegrator overlay path (`-v D710`). All validators return false.
        assert!(!DeclKind::D710.is_standalone_validator());
        assert!(!DeclKind::D301.is_standalone_validator());
        assert!(!DeclKind::D700.is_standalone_validator());
        assert!(!DeclKind::D300.is_standalone_validator());
        assert!(!DeclKind::D205.is_standalone_validator());
        assert!(!DeclKind::D112.is_standalone_validator());
        assert!(!DeclKind::D100.is_standalone_validator());
        assert!(!DeclKind::D101.is_standalone_validator());
    }

    #[test]
    fn existing_decl_kinds_unchanged() {
        assert_eq!(DeclKind::D300.as_duk_type(), "D300");
        assert_eq!(DeclKind::D394.as_duk_type(), "D394");
        assert_eq!(DeclKind::D406.as_duk_type(), "D406");
        assert_eq!(DeclKind::D112.as_duk_type(), "D112");
        assert_eq!(DeclKind::D205.as_duk_type(), "D205");
    }
}

#[cfg(test)]
mod cui_tests {
    use super::*;

    #[test]
    fn cnp_validation() {
        // base 196010141001 → control digit 9 (weighted sum 163, 163 % 11 = 9).
        assert!(valid_cnp("1960101410019")); // valid control digit
        assert!(!valid_cnp("1960101410017")); // wrong control digit (7 ≠ 9)
        assert!(!valid_cnp("196010141001")); // 12 digits (too short)
        assert!(!valid_cnp("19601014100199")); // 14 digits (too long)
        assert!(!valid_cnp("196010141001X")); // non-digit
        assert!(!valid_cnp("")); // empty
    }

    #[test]
    fn known_valid_cuis() {
        // Computed from algorithm: base + check digit
        assert!(valid_cui("12345674"), "12345674 should be valid");
        assert!(valid_cui("98765438"), "98765438 should be valid");
        assert!(valid_cui("87654329"), "87654329 should be valid");
        assert!(valid_cui("76543210"), "76543210 should be valid");
        assert!(valid_cui("22222229"), "22222229 should be valid");
        assert!(valid_cui("11111110"), "11111110 should be valid");
    }

    #[test]
    fn ro_prefix_stripped() {
        assert!(valid_cui("RO12345674"), "RO-prefixed should be valid");
        assert!(valid_cui("ro12345674"), "lowercase ro should be valid");
    }

    #[test]
    fn invalid_cuis() {
        assert!(!valid_cui("12345678"), "12345678 bad check digit");
        assert!(!valid_cui("98765432"), "98765432 bad check digit");
        assert!(!valid_cui("0"), "leading zero");
        assert!(!valid_cui("1"), "too short");
        assert!(!valid_cui("ABCDEF"), "non-numeric");
    }

    #[test]
    fn check_digit_computation() {
        assert_eq!(cui_check_digit("1234567"), 4);
        assert_eq!(cui_check_digit("9876543"), 8);
        assert_eq!(cui_check_digit("8765432"), 9);
        assert_eq!(cui_check_digit("7654321"), 0);
        assert_eq!(cui_check_digit("2222222"), 9);
        assert_eq!(cui_check_digit("1111111"), 0);
    }
}
