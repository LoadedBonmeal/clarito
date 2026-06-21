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

/// Map Romanian month names (case-insensitive) to month numbers 1..=12.
/// Used for ING HomeBanking exports which write dates as `dd MMMM yyyy`
/// (e.g. `04 mai 2026`, `01 octombrie 2020`).
fn ro_month(name: &str) -> Option<u32> {
    match name.to_lowercase().as_str() {
        "ianuarie" => Some(1),
        "februarie" => Some(2),
        "martie" => Some(3),
        "aprilie" => Some(4),
        "mai" => Some(5),
        "iunie" => Some(6),
        "iulie" => Some(7),
        "august" => Some(8),
        "septembrie" => Some(9),
        "octombrie" => Some(10),
        "noiembrie" => Some(11),
        "decembrie" => Some(12),
        _ => None,
    }
}

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

    // ING HomeBanking: "dd MMMM yyyy" with Romanian month names
    // e.g. "04 mai 2026", "01 octombrie 2020"
    {
        let parts: Vec<&str> = s.splitn(3, ' ').collect();
        if parts.len() == 3 {
            if let (Some(month_num), Ok(day), Ok(year)) = (
                ro_month(parts[1]),
                parts[0].parse::<u32>(),
                parts[2].trim().parse::<i32>(),
            ) {
                if (1..=31).contains(&day) && year >= 1900 {
                    return Some(format!("{year:04}-{month_num:02}-{day:02}"));
                }
            }
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

/// Parse a bank-statement amount string.
///
/// Uses the **rightmost-separator rule**: whichever of `,` or `.` appears last
/// is the decimal mark; the other is treated as a thousands separator and stripped.
/// This correctly handles both:
///   - European/ING format: `"1.234,56"` → 1234.56 (comma last → decimal)
///   - BT24/BCR/George format: `"1,234.56"` → 1234.56 (dot last → decimal)
///   - Single separator: `"1234,56"` or `"1234.56"` → treated as decimal
///   - Integer / already normalised: `"1500"`, `"-800.00"` → pass-through
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
        // Rightmost separator is the decimal mark.
        if clean.rfind(',') > clean.rfind('.') {
            // comma is the decimal mark: strip dots (thousands), comma → '.'
            clean.replace('.', "").replace(',', ".")
        } else {
            // dot is the decimal mark: strip commas (thousands)
            clean.replace(',', "")
        }
    } else if has_comma {
        // Single separator: comma is decimal
        clean.replace(',', ".")
    } else {
        // Already dot-decimal or integer
        clean.to_string()
    };

    Decimal::from_str(&normalised).ok()
}

// ─── Preamble detection ───────────────────────────────────────────────────────

/// Returns true if a CSV row looks like a transaction/data header row.
/// Heuristic: the row must contain at least one date-ish token (e.g. "data",
/// "date", "dată", "datum") AND at least one amount-ish token
/// ("debit", "credit", "suma", "sumă", "amount", "valoare").
/// This lets us skip the metadata preamble rows that RO banks (BT, ING, BCR)
/// insert above the real header (18 rows for BT24, ~4 for ING, ~5 for BCR/George).
fn looks_like_header(row: &[String]) -> bool {
    let joined = row.join(" ").to_lowercase();
    let has_date = joined.contains("dat") || joined.contains("date");
    let has_amount = joined.contains("debit")
        || joined.contains("credit")
        || joined.contains("suma")
        || joined.contains("sumă")
        || joined.contains("amount")
        || joined.contains("valoare")
        || joined.contains("sumă");
    has_date && has_amount
}

// ─── Main parser ─────────────────────────────────────────────────────────────

pub fn parse_csv(bytes: &[u8]) -> AppResult<ParsedStatement> {
    let text = decode_bytes(bytes);
    let all_lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();

    if all_lines.is_empty() {
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

    // Scan for the real header row: the first row that contains both a
    // date-ish and an amount-ish column name. Real RO bank CSVs have
    // metadata preamble rows above this (BT ~18, ING ~4, BCR/George ~5).
    // We detect the delimiter from the FIRST line (usually the most
    // comma/semicolon-rich) then scan forward.
    let first_delim = detect_delimiter(all_lines[0]);
    let (header_idx, delim) = {
        let mut found = None;
        for (i, line) in all_lines.iter().enumerate() {
            let d = detect_delimiter(line);
            let cells = split_csv_line(line, d);
            if looks_like_header(&cells) {
                found = Some((i, d));
                break;
            }
        }
        found.unwrap_or((0, first_delim))
    };

    let mut warnings: Vec<String> = Vec::new();
    if header_idx > 0 {
        warnings.push(format!(
            "CSV: {header_idx} rând(uri) de preambul ignorate înainte de antetul coloanelor."
        ));
    }

    let header_line = all_lines[header_idx];
    let headers = split_csv_line(header_line, delim);
    let col = detect_columns(&headers);
    let data_lines = &all_lines[header_idx + 1..];

    let mut txns: Vec<ParsedTxn> = Vec::new();
    let mut statement_date = String::new();
    let currency = "RON".to_string(); // CSV rarely specifies currency

    for (lineno, line) in data_lines.iter().enumerate() {
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

    // ── Real-format fixture tests (FIX 1a, 1b, 1c) ───────────────────────────

    /// BT24-style CSV: ~18 metadata preamble rows, then real header
    /// `Data,...,Descriere,Debit,Credit`, dd/MM/yyyy dates, `1,234.56` amounts
    /// (dot=decimal, comma=thousands — BT/BCR convention, opposite of ING).
    #[test]
    fn csv_bt_preamble_and_dot_decimal_amounts() {
        // 18 preamble rows followed by the real header and two data rows.
        let mut csv = String::new();
        csv.push_str("Extras de cont BT24\r\n");
        csv.push_str("Titular cont: SC TEST SRL\r\n");
        csv.push_str("CIF: RO12345678\r\n");
        for i in 0..15 {
            csv.push_str(&format!("Metadate rand {}\r\n", i + 4));
        }
        // Row 19 = real header (Data + Debit/Credit confirms header heuristic)
        csv.push_str("Data,Numar document,Descriere,Debit,Credit\r\n");
        // BT amounts use dot-decimal + comma-thousands: "1,234.56"
        csv.push_str("15/06/2026,DOC001,Incasare client,,\"1,234.56\"\r\n");
        csv.push_str("16/06/2026,DOC002,Plata furnizor,\"800.00\",\r\n");

        let stmt = parse_csv(csv.as_bytes()).unwrap();

        // Preamble warning emitted
        assert!(
            stmt.warnings.iter().any(|w| w.contains("preambul")),
            "should warn about skipped preamble rows"
        );

        assert_eq!(stmt.txns.len(), 2, "should parse both data rows");

        // Dates: dd/MM/yyyy → ISO
        assert_eq!(stmt.txns[0].booking_date, "2026-06-15");
        assert_eq!(stmt.txns[1].booking_date, "2026-06-16");

        // FIX 1c: BT dot-decimal amounts — "1,234.56" must be 1234.56 not 1.23456
        assert_eq!(
            stmt.txns[0].amount,
            Decimal::from_str("1234.56").unwrap(),
            "BT credit 1,234.56 must parse as 1234.56"
        );
        assert_eq!(
            stmt.txns[1].amount,
            Decimal::from_str("-800.00").unwrap(),
            "BT debit 800.00 must be negative"
        );
    }

    /// ING HomeBanking-style CSV: metadata preamble (Titular cont / CNP / Adresa),
    /// then real header with separate Debit/Credit columns, Romanian month-name
    /// dates (`04 mai 2026`), and ING European amounts (`"2.000,00"`).
    #[test]
    fn csv_ing_preamble_ro_month_names_and_eu_amounts() {
        let csv = concat!(
            "Titular cont: Ion Popescu\r\n",
            "CNP: 1234567890123\r\n",
            "Adresa: Str. Florilor nr. 1, Bucuresti\r\n",
            "Cont IBAN: RO49INGB0000999900000001\r\n",
            // Real header row — contains "Data" and "Debit"/"Credit"
            "Data;Detalii tranzactie;Debit;Credit\r\n",
            // ING dates: "dd MMMM yyyy", amounts: European "2.000,00"
            "\"04 mai 2026\";Incasare factura F001;;\"2.000,00\"\r\n",
            "\"01 octombrie 2020\";Plata furnizor;\"500,00\";\r\n",
        );

        let stmt = parse_csv(csv.as_bytes()).unwrap();

        // Preamble skipped
        assert!(
            stmt.warnings.iter().any(|w| w.contains("preambul")),
            "should warn about skipped preamble rows"
        );

        assert_eq!(stmt.txns.len(), 2, "should parse both ING rows");

        // FIX 1b: Romanian month-name dates → ISO
        assert_eq!(
            stmt.txns[0].booking_date, "2026-05-04",
            "04 mai 2026 must become 2026-05-04"
        );
        assert_eq!(
            stmt.txns[1].booking_date, "2020-10-01",
            "01 octombrie 2020 must become 2020-10-01"
        );

        // FIX 1c: ING European amounts — "2.000,00" = 2000.00 (comma last → decimal)
        assert_eq!(
            stmt.txns[0].amount,
            Decimal::from_str("2000.00").unwrap(),
            "ING credit 2.000,00 must parse as 2000.00"
        );
        assert_eq!(
            stmt.txns[1].amount,
            Decimal::from_str("-500.00").unwrap(),
            "ING debit 500,00 must be negative"
        );
    }

    // ── parse_amount unit tests (FIX 1c) ─────────────────────────────────────

    #[test]
    fn parse_amount_bt_dot_decimal() {
        // BT/BCR: dot=decimal, comma=thousands → "1,234.56" must be 1234.56
        assert_eq!(
            parse_amount("1,234.56").unwrap(),
            Decimal::from_str("1234.56").unwrap()
        );
    }

    #[test]
    fn parse_amount_ing_eu_format() {
        // ING: comma=decimal, dot=thousands → "1.234,56" must be 1234.56
        assert_eq!(
            parse_amount("1.234,56").unwrap(),
            Decimal::from_str("1234.56").unwrap()
        );
    }

    // ── parse_date unit tests (FIX 1b) ───────────────────────────────────────

    #[test]
    fn parse_date_ro_month_names() {
        assert_eq!(parse_date("04 mai 2026").unwrap(), "2026-05-04");
        assert_eq!(parse_date("01 octombrie 2020").unwrap(), "2020-10-01");
        assert_eq!(parse_date("31 decembrie 2025").unwrap(), "2025-12-31");
        assert_eq!(parse_date("01 ianuarie 2024").unwrap(), "2024-01-01");
        // Case-insensitive
        assert_eq!(parse_date("15 Martie 2023").unwrap(), "2023-03-15");
    }
}
