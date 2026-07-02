//! MT940 SWIFT bank statement parser.
//!
//! Tags parsed:
//!   :20:       — statement reference
//!   :25:       — account IBAN/number (used for statement-level IBAN)
//!   :60F:/:60M: — opening balance (D/C + 6-digit date + currency + amount,comma)
//!   :61:       — transaction: value date, optional booking date, D/C, amount
//!   :86:       — transaction details / counterparty info (follows :61:)
//!   :62F:/:62M: — closing balance (same format as :60F:)
//!
//! Amount format in :60F:/:61:/:62F:: comma as decimal separator, no sign —
//! the D/C indicator carries the sign.
//!
//! CUI extraction from :86:: "CUI:NNNN" / "CIF:NNNN" / "CUI NNNN".
//! IBAN extraction from :86:: "RO" + 2 digits + 4 uppercase letters + 16 alphanum.

use rust_decimal::Decimal;
use std::str::FromStr;

use crate::error::AppResult;

use super::parser::{
    check_integrity, decode_bytes, txn_hash, BankStatementParser, ParsedStatement, ParsedTxn,
};

pub struct Mt940Parser;

impl BankStatementParser for Mt940Parser {
    fn parse(&self, bytes: &[u8]) -> AppResult<ParsedStatement> {
        parse_mt940(bytes)
    }
}

// ─── Tag splitting ────────────────────────────────────────────────────────────

/// Split raw MT940 text into (tag_name, content) pairs.
/// Tags begin with ":XX:" where XX is 1–3 chars.
fn split_tags(text: &str) -> Vec<(String, String)> {
    let mut tags: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0usize;

    while i < len {
        // Look for ':' preceded by newline or start-of-text
        if chars[i] == ':' && (i == 0 || chars[i - 1] == '\n' || chars[i - 1] == '\r') {
            // Find closing ':'
            if let Some(rel) = (1usize..7).find(|&d| i + d < len && chars[i + d] == ':') {
                let tag_end = i + rel;
                let tag_name: String = chars[i + 1..tag_end].iter().collect();
                let content_start = tag_end + 1;
                // Content ends at the next tag start or EOF
                let content_end = find_next_tag_start(&chars, content_start);
                let content: String = chars[content_start..content_end].iter().collect();
                // Trim trailing CRLF but preserve internal content
                let content = content.trim_end_matches(['\r', '\n']).to_string();
                tags.push((tag_name, content));
                i = content_end;
                continue;
            }
        }
        i += 1;
    }
    tags
}

fn find_next_tag_start(chars: &[char], from: usize) -> usize {
    let len = chars.len();
    for i in from..len {
        if chars[i] == ':' && (i == 0 || chars[i - 1] == '\n' || chars[i - 1] == '\r') {
            // Verify it's a valid tag (closing ':' within next 6 chars)
            if (1usize..7).any(|d| i + d < len && chars[i + d] == ':') {
                return i;
            }
        }
    }
    len
}

// ─── Balance tag parsing ─────────────────────────────────────────────────────

/// Parse ":60F:" or ":62F:" content: "C260101RON10000,00"
/// → (is_credit, iso_date, currency_3, signed_decimal)
fn parse_balance_tag(content: &str) -> Option<(bool, String, String, Decimal)> {
    let s = content.trim();
    if s.len() < 10 {
        return None;
    }

    let is_credit = match s.chars().next()? {
        'C' => true,
        'D' => false,
        _ => return None,
    };

    let rest = &s[1..];
    if rest.len() < 6 {
        return None;
    }

    // 6-digit date: YYMMDD
    let yy = rest[..2].parse::<i32>().ok()?;
    let mm: u32 = rest[2..4].parse().ok()?;
    let dd: u32 = rest[4..6].parse().ok()?;
    let year = 2000 + yy;
    let iso_date = format!("{year:04}-{mm:02}-{dd:02}");

    let rest = &rest[6..];

    // 3-letter currency (or missing — fall back to empty → caller uses "RON")
    let (currency, amount_str) =
        if rest.len() >= 3 && rest[..3].chars().all(|c| c.is_ascii_alphabetic()) {
            (rest[..3].to_string(), &rest[3..])
        } else {
            (String::new(), rest)
        };

    let amount_str = amount_str.replace(',', ".");
    let amount = Decimal::from_str(amount_str.trim()).ok()?;
    let signed = if is_credit { amount } else { -amount };

    Some((is_credit, iso_date, currency, signed))
}

// ─── :61: parsing ────────────────────────────────────────────────────────────

/// Parse :61: content. Simplified format:
///   VDATE[BDATE]DC[FUND]AMOUNT[NREF...]
/// VDATE = YYMMDD, BDATE = optional MMDD, DC = C|D|RD|RC|CR|DR
/// AMOUNT uses comma decimal.
///
/// Returns: (value_date, booking_date, is_credit, amount)
fn parse_61(content: &str) -> Option<(String, String, bool, Decimal)> {
    let s = content.trim();
    if s.len() < 10 {
        return None;
    }

    let yy = s[..2].parse::<i32>().ok()?;
    let mm: u32 = s[2..4].parse().ok()?;
    let dd: u32 = s[4..6].parse().ok()?;
    let year = 2000 + yy;
    let value_date = format!("{year:04}-{mm:02}-{dd:02}");

    let mut pos = 6usize;

    // Optional 4-digit booking date MMDD
    let booking_date = if pos + 4 <= s.len()
        && s[pos..pos + 4].chars().all(|c| c.is_ascii_digit())
        && !s[pos..].starts_with(['C', 'D', 'R'])
    {
        let bm: u32 = s[pos..pos + 2].parse().ok()?;
        let bd: u32 = s[pos + 2..pos + 4].parse().ok()?;
        pos += 4;
        // FIX 8: the booking MMDD carries no year — inheriting the value date's
        // year verbatim puts a Dec-30 booking on a Jan-05 value date a YEAR ahead.
        // Standard SWIFT rule: the booking date is near the value date, so a
        // booking month >6 months AFTER the value month means the PRIOR year
        // (value Jan / booking Dec), and >6 months BEFORE means the NEXT year
        // (value Dec / booking Jan).
        let booking_year = if bm as i32 - mm as i32 > 6 {
            year - 1
        } else if mm as i32 - bm as i32 > 6 {
            year + 1
        } else {
            year
        };
        format!("{booking_year:04}-{bm:02}-{bd:02}")
    } else {
        value_date.clone()
    };

    // D/C indicator (2-char reversal first, then single)
    let is_credit = if s[pos..].starts_with("RD") || s[pos..].starts_with("DR") {
        pos += 2;
        false
    } else if s[pos..].starts_with("RC") || s[pos..].starts_with("CR") {
        pos += 2;
        true
    } else if s[pos..].starts_with('D') {
        pos += 1;
        false
    } else if s[pos..].starts_with('C') {
        pos += 1;
        true
    } else {
        return None;
    };

    // Optional 1-letter funds code (3rd letter of the currency — 'N' for RON).
    // FIX 1: the old `ch != b'N'` guard (meant to avoid eating an "NREF"/"NTRF"
    // reference prefix) also refused RON's own funds code 'N', so the 'N' fell
    // into the amount parse and EVERY RON transaction failed. Instead, skip the
    // letter exactly when the NEXT byte starts the amount (an ASCII digit or a
    // comma) — a reference prefix like "NTRF" is followed by another letter, so
    // it is never consumed, while "DN449,77" correctly skips the 'N'.
    if pos < s.len() {
        let ch = s.as_bytes()[pos];
        if ch.is_ascii_alphabetic()
            && s.as_bytes()
                .get(pos + 1)
                .is_some_and(|b| b.is_ascii_digit() || *b == b',')
        {
            pos += 1;
        }
    }

    // Amount: digits and commas
    let amt_start = pos;
    let s_bytes = s.as_bytes();
    while pos < s.len() && (s_bytes[pos].is_ascii_digit() || s_bytes[pos] == b',') {
        pos += 1;
    }
    if amt_start == pos {
        return None;
    }

    let amt_str = s[amt_start..pos].replace(',', ".");
    let amount = Decimal::from_str(&amt_str).ok()?;
    let signed = if is_credit { amount } else { -amount };

    Some((value_date, booking_date, is_credit, signed))
}

// ─── :86: helpers ────────────────────────────────────────────────────────────

fn extract_cui(text: &str) -> Option<String> {
    let upper = text.to_uppercase();
    for pat in &["CUI:", "CIF:", "CUI ", "CIF "] {
        if let Some(start) = upper.find(pat) {
            let rest = &text[start + pat.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 2 {
                return Some(digits);
            }
        }
    }
    None
}

/// Extract first RO IBAN (24 chars: RO + 2 digits + 4 letters + 16 alphanum).
fn extract_iban(text: &str) -> Option<String> {
    let upper = text.to_uppercase();
    let bytes = upper.as_bytes();
    let len = bytes.len();
    if len < 24 {
        return None;
    }

    for i in 0..=(len - 24) {
        if bytes[i] != b'R' || bytes[i + 1] != b'O' {
            continue;
        }
        if !bytes[i + 2].is_ascii_digit() || !bytes[i + 3].is_ascii_digit() {
            continue;
        }
        if !bytes[i + 4..i + 8].iter().all(|b| b.is_ascii_uppercase()) {
            continue;
        }
        if !bytes[i + 8..i + 24]
            .iter()
            .all(|b| b.is_ascii_alphanumeric())
        {
            continue;
        }
        // Check not surrounded by more alphanum (would be a longer sequence)
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after_ok = i + 24 >= len || !bytes[i + 24].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(upper[i..i + 24].to_string());
        }
    }
    None
}

/// Extract counterparty name from :86: content.
/// :86: subfields use /CODE/value format (e.g. /ORDP/CLIENT SRL).
/// Falls back to the first non-empty line.
fn extract_cpty_name(content: &str) -> Option<String> {
    // Try /NAME/ subfield first
    let upper = content.to_uppercase();
    if let Some(idx) = upper.find("/NAME/") {
        let rest = &content[idx + 6..];
        let val: String = rest.lines().next().unwrap_or("").trim().to_string();
        // Strip any following subfield /XXX/
        let val = if let Some(p) = val.find('/') {
            val[..p].trim().to_string()
        } else {
            val
        };
        if !val.is_empty() {
            return Some(val);
        }
    }
    // First non-empty line (stripped of leading /CODE/ if present)
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let name = if trimmed.starts_with('/') {
            // Pattern: /CODE/value — take the value part
            let parts: Vec<&str> = trimmed.splitn(3, '/').collect();
            if parts.len() >= 3 {
                parts[2].trim()
            } else {
                trimmed
            }
        } else {
            trimmed
        };
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

// ─── Main parser ─────────────────────────────────────────────────────────────

pub fn parse_mt940(bytes: &[u8]) -> AppResult<ParsedStatement> {
    let text = decode_bytes(bytes);
    let tags = split_tags(&text);

    let mut statement_ref = String::new();
    let mut statement_date = String::new();
    let mut opening_balance = Decimal::ZERO;
    let mut closing_balance = Decimal::ZERO;
    // FIX 5: track whether the FILE carried a currency (in :60F:/:62F:). When it
    // did not, transactions are emitted with an EMPTY currency so the import
    // command can resolve it from the linked bank account (instead of a silent
    // RON default that breaks currency-aware matching for foreign accounts).
    let mut currency: Option<String> = None;
    let mut txns: Vec<ParsedTxn> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Pending :61: waiting for its :86:
    struct Pending61 {
        value_date: String,
        booking_date: String,
        amount: Decimal,
    }
    let mut pending: Option<Pending61> = None;

    let flush_pending =
        |p: Pending61, reference: Option<String>, txns: &mut Vec<ParsedTxn>, currency: &str| {
            let cui = reference.as_deref().and_then(extract_cui);
            let iban = reference.as_deref().and_then(extract_iban);
            let cpty_name = reference.as_deref().and_then(extract_cpty_name);
            let hash = txn_hash(&p.booking_date, &p.amount, reference.as_deref());
            txns.push(ParsedTxn {
                booking_date: p.booking_date,
                value_date: Some(p.value_date),
                amount: p.amount,
                currency: currency.to_string(),
                counterparty_name: cpty_name,
                counterparty_iban: iban,
                counterparty_cui: cui,
                reference,
                txn_hash: hash,
            });
        };

    for (tag, content) in &tags {
        match tag.as_str() {
            "20" => {
                statement_ref = content.trim().to_string();
            }
            "25" => {
                // Account identifier — not used directly beyond IBAN extraction
            }
            "60F" | "60M" => {
                if let Some((_cr, date, cur, bal)) = parse_balance_tag(content) {
                    opening_balance = bal;
                    if statement_date.is_empty() {
                        statement_date = date;
                    }
                    if !cur.is_empty() {
                        currency = Some(cur);
                    }
                }
            }
            "61" => {
                // Flush pending (no following :86:)
                if let Some(p) = pending.take() {
                    flush_pending(p, None, &mut txns, currency.as_deref().unwrap_or(""));
                }
                match parse_61(content) {
                    Some((vd, bd, _cr, amt)) => {
                        pending = Some(Pending61 {
                            value_date: vd,
                            booking_date: bd,
                            amount: amt,
                        });
                    }
                    None => {
                        warnings.push(format!(
                            "Could not parse :61: tag: {}",
                            &content[..content.len().min(60)]
                        ));
                    }
                }
            }
            "86" => {
                if let Some(p) = pending.take() {
                    flush_pending(
                        p,
                        Some(content.trim().to_string()),
                        &mut txns,
                        currency.as_deref().unwrap_or(""),
                    );
                }
                // If no pending :61:, this is a statement-level :86: — ignore.
            }
            "62F" | "62M" => {
                if let Some((_cr, date, cur, bal)) = parse_balance_tag(content) {
                    closing_balance = bal;
                    if statement_date.is_empty() {
                        statement_date = date;
                    }
                    if !cur.is_empty() && currency.is_none() {
                        currency = Some(cur);
                    }
                }
            }
            _ => {} // :25:, :28C:, etc.
        }
    }

    // Flush any trailing :61: with no following :86:
    if let Some(p) = pending.take() {
        flush_pending(p, None, &mut txns, currency.as_deref().unwrap_or(""));
    }

    let integrity_ok = check_integrity(opening_balance, closing_balance, &txns);
    if integrity_ok == Some(false) {
        let sum: Decimal = txns.iter().map(|t| t.amount).sum();
        warnings.push(format!(
            "Integrity check: opening({opening_balance}) + sum({sum}) ≠ closing({closing_balance})"
        ));
    }

    Ok(ParsedStatement {
        statement_ref,
        statement_date,
        opening_balance,
        closing_balance,
        // Statement-level metadata keeps the RON default for display; the
        // per-txn currency stays "" when the file carried none (see FIX 5).
        currency: currency.unwrap_or_else(|| "RON".to_string()),
        txns,
        warnings,
        integrity_ok,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 2 transactions: credit 500 + debit 300; opening 10000, closing 10200.
    const FIXTURE: &str = ":20:STMT20260101\r\n\
:25:RO49AAAA1B31007593840000\r\n\
:28C:001/001\r\n\
:60F:C260101RON10000,00\r\n\
:61:2601010101C500,00NTRFREF001\r\n\
:86:Transfer de la FURNIZOR SRL/CUI:12345678/REF:F2026001\r\n\
:61:2601020102D300,00NTRFREF002\r\n\
:86:Plata catre DISTRIBUT SRL/CUI:87654321\r\n\
:62F:C260102RON10200,00\r\n";

    #[test]
    fn mt940_parses_two_txns() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 2, "should parse 2 transactions");
    }

    #[test]
    fn mt940_credit_amount_positive() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert!(
            stmt.txns[0].amount > Decimal::ZERO,
            "credit must be positive"
        );
        assert_eq!(stmt.txns[0].amount, Decimal::from_str("500").unwrap());
    }

    #[test]
    fn mt940_debit_amount_negative() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert!(
            stmt.txns[1].amount < Decimal::ZERO,
            "debit must be negative"
        );
        assert_eq!(stmt.txns[1].amount, Decimal::from_str("-300").unwrap());
    }

    #[test]
    fn mt940_extracts_cui() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.txns[0].counterparty_cui.as_deref(), Some("12345678"));
        assert_eq!(stmt.txns[1].counterparty_cui.as_deref(), Some("87654321"));
    }

    #[test]
    fn mt940_integrity_ok() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        // opening(10000) + 500 - 300 = 10200 == closing → ok
        assert_eq!(stmt.integrity_ok, Some(true));
    }

    #[test]
    fn mt940_integrity_mismatch_produces_warning() {
        // Opening=10000, credit+500 → expected closing=10500, but statement says 9999.
        let bad = ":20:REF\r\n\
:60F:C260101RON10000,00\r\n\
:61:2601010101C500,00NTRFREF1\r\n\
:86:CLIENT SRL\r\n\
:62F:C260101RON9999,00\r\n";
        let stmt = parse_mt940(bad.as_bytes()).unwrap();
        assert_eq!(stmt.integrity_ok, Some(false));
        assert!(
            !stmt.warnings.is_empty(),
            "mismatch should produce a warning"
        );
    }

    #[test]
    fn mt940_malformed_61_warning_continues() {
        // :61: with garbage content — should warn but not panic; parse continues
        let input = ":20:REF\r\n\
:60F:C260101RON1000,00\r\n\
:61:XXXXXXGARBAGE\r\n\
:61:2601010101C200,00NTRFREF2\r\n\
:86:VALID CLIENT\r\n\
:62F:C260101RON1200,00\r\n";
        let stmt = parse_mt940(input.as_bytes()).unwrap();
        assert!(
            !stmt.warnings.is_empty(),
            "malformed :61: should produce warning"
        );
        assert_eq!(
            stmt.txns.len(),
            1,
            "valid transaction should still be parsed"
        );
    }

    #[test]
    fn mt940_dedup_hash_differs_for_different_txns() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert_ne!(stmt.txns[0].txn_hash, stmt.txns[1].txn_hash);
    }

    // ── FIX 1: RON funds code 'N' must not break the amount parse ────────────

    #[test]
    fn mt940_ron_funds_code_n_parses_amount() {
        // "DN449,77NTRFNONREF": D = debit, N = RON funds code, 449,77 = amount,
        // NTRF... = reference. The old parser refused to skip 'N' (to protect
        // "NREF") so the amount parse started at 'N' and failed → the whole RON
        // statement imported 0 transactions.
        let (value_date, booking_date, is_credit, amount) =
            parse_61("2601050105DN449,77NTRFNONREF").expect("RON :61: line must parse");
        assert_eq!(value_date, "2026-01-05");
        assert_eq!(booking_date, "2026-01-05");
        assert!(!is_credit);
        assert_eq!(amount, Decimal::from_str("-449.77").unwrap());
        assert_eq!(amount.abs(), Decimal::from_str("449.77").unwrap());
    }

    #[test]
    fn mt940_ron_statement_with_funds_code_imports_txns() {
        let input = ":20:REF\r\n\
:60F:C260101RON1000,00\r\n\
:61:2601050105DN449,77NTRFNONREF\r\n\
:86:Plata furnizor CUI:12345678\r\n\
:62F:C260105RON550,23\r\n";
        let stmt = parse_mt940(input.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 1, "RON txn with funds code 'N' must parse");
        assert_eq!(stmt.txns[0].amount, Decimal::from_str("-449.77").unwrap());
        assert_eq!(stmt.integrity_ok, Some(true));
    }

    #[test]
    fn mt940_reference_prefix_ntrf_not_consumed_as_funds_code() {
        // No funds code: the amount directly follows D/C, and the reference
        // starts with "NTRF" — the letter-skip must not eat into the reference.
        let (_, _, _, amount) = parse_61("2601010101C500,00NTRFREF001").unwrap();
        assert_eq!(amount, Decimal::from_str("500").unwrap());
    }

    // ── FIX 8: booking-date year across the Dec/Jan boundary ─────────────────

    #[test]
    fn mt940_booking_date_december_on_january_value_date_uses_prior_year() {
        // Value date 2026-01-05, booking MMDD 1230 → booking is 2025-12-30,
        // NOT 2026-12-30 (a year in the future).
        let (value_date, booking_date, _, _) =
            parse_61("2601051230D100,00NTRF").expect("must parse");
        assert_eq!(value_date, "2026-01-05");
        assert_eq!(booking_date, "2025-12-30");
    }

    #[test]
    fn mt940_booking_date_january_on_december_value_date_uses_next_year() {
        // Mirror direction: value date 2026-12-30, booking MMDD 0102 → 2027-01-02.
        let (value_date, booking_date, _, _) =
            parse_61("2612300102C100,00NTRF").expect("must parse");
        assert_eq!(value_date, "2026-12-30");
        assert_eq!(booking_date, "2027-01-02");
    }

    #[test]
    fn mt940_booking_date_same_month_keeps_year() {
        let (_, booking_date, _, _) = parse_61("2601050106C100,00NTRF").unwrap();
        assert_eq!(booking_date, "2026-01-06");
    }

    // ── FIX 5: currency explicitness marker ──────────────────────────────────

    #[test]
    fn mt940_txn_currency_explicit_when_file_carries_it() {
        let stmt = parse_mt940(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.txns[0].currency, "RON", ":60F: carried RON");
    }

    #[test]
    fn mt940_txn_currency_empty_when_file_carries_none() {
        // :60F: without a 3-letter currency → txns marked with "" so the import
        // command can resolve the currency from the linked bank account.
        let input = ":20:REF\r\n\
:60F:C2601011000,00\r\n\
:61:2601050105DN449,77NTRFNONREF\r\n\
:86:Plata furnizor\r\n";
        let stmt = parse_mt940(input.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 1);
        assert_eq!(
            stmt.txns[0].currency, "",
            "no currency in the file → empty marker for account-level resolution"
        );
        assert_eq!(stmt.currency, "RON", "statement metadata keeps RON default");
    }
}
