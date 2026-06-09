//! Official ANAF declaration generators (D300, D394, SAF-T D406) + the
//! per-period schema-version layer and the DUKIntegrator validation harness.
//!
//! Every generator here emits schema-conformant XML via the `quick_xml::Writer`
//! pattern (see `ubl/generator.rs`), NOT the legacy hand-rolled string builders
//! in `commands/{declarations,d394,saft}.rs`.

pub mod cash_vat;
pub mod d101;
pub mod d300;
pub mod d390;
pub mod d394;
pub mod duk;
pub mod etransport;
pub mod etva;
pub mod form_versions;
pub mod preflight;
pub mod saft;
pub mod validation;
pub mod version;
pub mod xml;

/// The three official declarations this module targets. `as_duk_type` returns
/// the token DUKIntegrator's `-v` CLI expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    D300,
    D394,
    D406,
}

impl DeclKind {
    pub fn as_duk_type(self) -> &'static str {
        match self {
            DeclKind::D300 => "D300",
            DeclKind::D394 => "D394",
            DeclKind::D406 => "D406",
        }
    }
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

#[cfg(test)]
mod cui_tests {
    use super::*;

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
