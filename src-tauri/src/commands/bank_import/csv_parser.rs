//! Generic defensive CSV bank statement parser.
//!
//! Auto-detects delimiter (';' or ','), date format (DD.MM.YYYY, DD/MM/YYYY, YYYY-MM-DD),
//! amount column (single signed or separate debit/credit pair), and description/IBAN/name columns.
//! Tolerates common RO bank CSV layouts (BRD, BCR, ING, BT, Raiffeisen).
//!
//! Per-record errors produce warnings and skip the row — they do not abort parsing.
//! CSV files typically omit balance totals so `integrity_ok` is always None.
//!
//! Documented follow-up: configurable column-mapping UI for non-standard layouts.

use rust_decimal::Decimal;
use std::str::FromStr;

use crate::error::AppResult;

use super::parser::{decode_bytes, txn_hash, BankStatementParser, ParsedStatement, ParsedTxn};

pub struct CsvParser;

impl BankStatementParser for CsvParser {
    fn parse(&self, bytes: &[u8]) -> AppResult<ParsedStatement> {
        parse_csv(bytes)
    }
}

// ─── CSV utilities ────────────────────────────────────────────────────────────

fn detect_delimiter(header: &str) -> char {
    let semicolons = header.chars().filter(|&c| c == ';').count();
    let commas = header.chars().filter(|&c| c == ',').count();
    if semicolons > commas {
        ';'
    } else {
        ','
    }
}

fn split_csv_line(line: &str, delim: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;

    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c == delim && !in_quotes => {
                fields.push(cur.trim().to_string());
                cur = String::new();
            }
            c => cur.push(c),
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

// ─── Column detection ─────────────────────────────────────────────────────────

struct ColMap {
    date: usize,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    desc: Option<usize>,
    iban: Option<usize>,
    name: Option<usize>,
}

fn detect_columns(headers: &[String]) -> ColMap {
    let mut date = 0usize;
    let mut amount: Option<usize> = None;
    let mut debit: Option<usize> = None;
    let mut credit: Option<usize> = None;
    let mut desc: Option<usize> = None;
    let mut iban: Option<usize> = None;
    let mut name: Option<usize> = None;

    for (i, h) in headers.iter().enumerate() {
        let lower = h.to_lowercase();
        let lw = lower.trim();
        match lw {
            s if s.contains("dat") => {
                // "Data", "Data tranzactie", "Date", "Dată"
                date = i;
            }
            s if s == "suma"
                || s == "amount"
                || s == "sumă"
                || s.contains("valoare")
                || s == "debit/credit"
                || s == "suma tranzactie" =>
            {
                amount = Some(i);
            }
            s if s.contains("debit") && !s.contains("credit") => {
                debit = Some(i);
            }
            s if s.contains("credit") && !s.contains("debit") => {
                credit = Some(i);
            }
            s if s.contains("descrip")
                || s.contains("detalii")
                || s.contains("narat")
                || s.contains("referinta")
                || s.contains("referință")
                || s.contains("explain")
                || s.contains("info")
                || s == "note" =>
            {
                desc = Some(i);
            }
            s if s.contains("iban") => {
                iban = Some(i);
            }
            s if s.contains("beneficiar")
                || s.contains("platitor")
                || s.contains("contrag")
                || s.contains("partener")
                || s == "name"
                || s == "denumire" =>
            {
                name = Some(i);
            }
            _ => {}
        }
    }

    ColMap {
        date,
        amount,
        debit,
        credit,
        desc,
        iban,
        name,
    }
}

// ─── Date parsing ─────────────────────────────────────────────────────────────

fn parse_date(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() < 8 {
        return None;
    }

    // ISO: YYYY-MM-DD
    if s.len() >= 10 && &s[4..5] == "-" && &s[7..8] == "-" {
        let (y, m, d) = (&s[..4], &s[5..7], &s[8..10]);
        if let (Ok(_y), Ok(_m), Ok(_d)) = (y.parse::<i32>(), m.parse::<u32>(), d.parse::<u32>()) {
            return Some(format!("{y}-{m}-{d}"));
        }
    }

    // DD.MM.YYYY or DD/MM/YYYY or DD-MM-YYYY (ambiguous with ISO)
    let sep = if s.contains('.') {
        '.'
    } else if s.contains('/') {
        '/'
    } else if s.len() >= 10 && &s[2..3] == "-" {
        '-'
    } else {
        return None;
    };

    let parts: Vec<&str> = s.splitn(3, sep).collect();
    if parts.len() < 3 {
        return None;
    }

    // Detect DD.MM.YYYY vs YYYY.MM.DD
    let (day_part, month_part, year_part) = if parts[0].len() == 4 {
        (parts[2], parts[1], parts[0])
    } else {
        (parts[0], parts[1], parts[2])
    };

    let day: u32 = day_part.parse().ok()?;
    let month: u32 = month_part.parse().ok()?;
    let year_raw: i32 = year_part.trim().parse().ok()?;
    let year = if year_raw < 100 {
        2000 + year_raw
    } else {
        year_raw
    };

    Some(format!("{year:04}-{month:02}-{day:02}"))
}

// ─── Amount parsing ───────────────────────────────────────────────────────────

/// Parse a RO-formatted amount string.
/// RO convention: period as thousands separator, comma as decimal.
/// Example: "1.234,56" → 1234.56; "1234,56" → 1234.56; "-800.00" → -800.00
fn parse_amount(s: &str) -> Option<Decimal> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Remove non-breaking spaces, thin spaces, etc.
    let clean: String = s
        .chars()
        .filter(|c| !matches!(*c, '\u{00A0}' | '\u{202F}' | '\u{2009}'))
        .collect();
    let clean = clean.trim();

    let has_comma = clean.contains(',');
    let has_dot = clean.contains('.');

    let normalised = if has_comma && has_dot {
        // "1.234,56" — dot is thousands, comma is decimal
        clean.replace('.', "").replace(',', ".")
    } else if has_comma {
        // "1234,56" — comma is decimal
        clean.replace(',', ".")
    } else {
        // Already dot-decimal or integer
        clean.to_string()
    };

    Decimal::from_str(&normalised).ok()
}

// ─── Main parser ─────────────────────────────────────────────────────────────

pub fn parse_csv(bytes: &[u8]) -> AppResult<ParsedStatement> {
    let text = decode_bytes(bytes);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());

    let header_line = match lines.next() {
        Some(h) => h,
        None => {
            return Ok(ParsedStatement {
                statement_ref: String::new(),
                statement_date: String::new(),
                opening_balance: Decimal::ZERO,
                closing_balance: Decimal::ZERO,
                currency: "RON".to_string(),
                txns: vec![],
                warnings: vec!["Empty CSV file.".into()],
                integrity_ok: None,
            });
        }
    };

    let delim = detect_delimiter(header_line);
    let headers = split_csv_line(header_line, delim);
    let col = detect_columns(&headers);

    let mut txns: Vec<ParsedTxn> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut statement_date = String::new();
    let currency = "RON".to_string(); // CSV rarely specifies currency

    for (lineno, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields = split_csv_line(line, delim);
        if fields.len() <= col.date {
            continue;
        }

        // Date
        let date_raw = &fields[col.date];
        let date = match parse_date(date_raw) {
            Some(d) => d,
            None => {
                if !date_raw.trim().is_empty() {
                    warnings.push(format!(
                        "Row {}: unparseable date '{date_raw}' — skipped",
                        lineno + 2
                    ));
                }
                continue;
            }
        };
        if statement_date.is_empty() {
            statement_date = date.clone();
        }

        // Amount
        let amount: Decimal = if let Some(ai) = col.amount {
            let raw = fields.get(ai).map(|s| s.as_str()).unwrap_or("");
            match parse_amount(raw) {
                Some(a) => a,
                None => {
                    warnings.push(format!(
                        "Row {}: unparseable amount '{raw}' — skipped",
                        lineno + 2
                    ));
                    continue;
                }
            }
        } else {
            // Separate debit / credit columns
            let cr_raw = col
                .credit
                .and_then(|i| fields.get(i))
                .map(|s| s.as_str())
                .unwrap_or("");
            let db_raw = col
                .debit
                .and_then(|i| fields.get(i))
                .map(|s| s.as_str())
                .unwrap_or("");
            let cr = parse_amount(cr_raw).unwrap_or(Decimal::ZERO);
            let db = parse_amount(db_raw).unwrap_or(Decimal::ZERO);
            if cr.is_zero() && db.is_zero() {
                warnings.push(format!(
                    "Row {}: both debit and credit are zero/empty — skipped",
                    lineno + 2
                ));
                continue;
            }
            cr - db // credit = positive, debit = negative
        };

        let reference = col
            .desc
            .and_then(|i| fields.get(i))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let cpty_name = col
            .name
            .and_then(|i| fields.get(i))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let cpty_iban = col
            .iban
            .and_then(|i| fields.get(i))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        let hash = txn_hash(&date, &amount, reference.as_deref());
        txns.push(ParsedTxn {
            booking_date: date,
            value_date: None,
            amount,
            currency: currency.clone(),
            counterparty_name: cpty_name,
            counterparty_iban: cpty_iban,
            counterparty_cui: None,
            reference,
            txn_hash: hash,
        });
    }

    Ok(ParsedStatement {
        statement_ref: String::new(),
        statement_date,
        opening_balance: Decimal::ZERO,
        closing_balance: Decimal::ZERO,
        currency,
        txns,
        warnings,
        integrity_ok: None, // CSV does not carry balances in the standard layouts
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Signed-amount CSV (common BRD / BCR layout)
    const CSV_SIGNED: &str = "Data;Descriere;Suma;IBAN Partener;Nume Partener\r\n\
2026-01-05;Incasare factura F2026/001;1500.00;RO49AAAA1B31007593840000;CLIENT ALFA SRL\r\n\
2026-01-10;Plata furnizor;-800.00;;FURNIZOR BETA SRL\r\n\
2026-01-15;Comision bancar;-15.50;;\r\n";

    // Separate Debit / Credit columns (ING Romania layout)
    const CSV_DEBIT_CREDIT: &str = "Data;Detalii;Debit;Credit\r\n\
15.01.2026;Incasare factura;;2000,00\r\n\
20.01.2026;Plata furnizor;500,00;\r\n";

    // RO thousands-separator format: "1.500,00"
    const CSV_RO_FORMAT: &str = "Data;Suma\r\n\
2026-01-01;1.500,00\r\n\
2026-01-02;-300,50\r\n";

    #[test]
    fn csv_signed_amount_parses_three_rows() {
        let stmt = parse_csv(CSV_SIGNED.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 3);
        assert_eq!(stmt.txns[0].amount, Decimal::from_str("1500").unwrap());
        assert_eq!(stmt.txns[1].amount, Decimal::from_str("-800").unwrap());
        assert_eq!(stmt.txns[2].amount, Decimal::from_str("-15.5").unwrap());
    }

    #[test]
    fn csv_debit_credit_columns() {
        let stmt = parse_csv(CSV_DEBIT_CREDIT.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 2);
        assert!(
            stmt.txns[0].amount > Decimal::ZERO,
            "credit row should be positive"
        );
        assert!(
            stmt.txns[1].amount < Decimal::ZERO,
            "debit row should be negative"
        );
    }

    #[test]
    fn csv_ro_thousands_separator() {
        let stmt = parse_csv(CSV_RO_FORMAT.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 2);
        assert_eq!(stmt.txns[0].amount, Decimal::from_str("1500").unwrap());
        assert_eq!(stmt.txns[1].amount, Decimal::from_str("-300.5").unwrap());
    }

    #[test]
    fn csv_bad_date_produces_warning_continues() {
        let bad = "Data;Suma\r\nNOT-A-DATE;100\r\n2026-01-01;50\r\n";
        let stmt = parse_csv(bad.as_bytes()).unwrap();
        assert!(!stmt.warnings.is_empty(), "bad date should produce warning");
        assert_eq!(stmt.txns.len(), 1, "valid row should still be parsed");
    }

    #[test]
    fn csv_empty_input_no_panic() {
        let stmt = parse_csv(b"").unwrap();
        assert!(stmt.txns.is_empty());
    }

    #[test]
    fn csv_counterparty_fields_extracted() {
        let stmt = parse_csv(CSV_SIGNED.as_bytes()).unwrap();
        assert_eq!(
            stmt.txns[0].counterparty_name.as_deref(),
            Some("CLIENT ALFA SRL")
        );
        assert_eq!(
            stmt.txns[0].counterparty_iban.as_deref(),
            Some("RO49AAAA1B31007593840000")
        );
    }
}
