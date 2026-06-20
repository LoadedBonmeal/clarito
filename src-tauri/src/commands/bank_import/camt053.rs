//! CAMT.053 (ISO 20022 BankToCustomerStatement) XML parser.
//!
//! Uses quick-xml for streaming parsing over `<BkToCstmrStmt>` → `<Stmt>`.
//! Elements extracted:
//!   <Bal Tp=OPBD> → opening balance
//!   <Bal Tp=CLBD> → closing balance
//!   <Ntry> → each entry: Amt + CdtDbtInd + BookgDt + ValDt +
//!             NtryDtls/TxDtls/(RltdPties,RltdAgts,RmtInf)
//!
//! Namespace-agnostic: uses `local_name()` so any CAMT variant namespace works.

use quick_xml::events::Event;
use quick_xml::Reader;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::error::AppResult;

use super::parser::{
    check_integrity, decode_bytes, txn_hash, BankStatementParser, ParsedStatement, ParsedTxn,
};

pub struct Camt053Parser;

impl BankStatementParser for Camt053Parser {
    fn parse(&self, bytes: &[u8]) -> AppResult<ParsedStatement> {
        parse_camt053(bytes)
    }
}

// ─── Internal entry buffer ────────────────────────────────────────────────────

#[derive(Default)]
struct NtryBuf {
    amt: Decimal,
    ccy: String,
    credit: bool,
    booking_date: String,
    value_date: Option<String>,
    cpty_name: Option<String>,
    cpty_iban: Option<String>,
    reference: Option<String>,
}

// ─── Main parser ─────────────────────────────────────────────────────────────

pub fn parse_camt053(bytes: &[u8]) -> AppResult<ParsedStatement> {
    let text = decode_bytes(bytes);
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);

    let mut statement_ref = String::new();
    let mut statement_date = String::new();
    let mut opening_balance = Decimal::ZERO;
    let mut closing_balance = Decimal::ZERO;
    let mut currency = "RON".to_string();
    let mut txns: Vec<ParsedTxn> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Element path stack (local names only)
    let mut path: Vec<String> = Vec::new();

    // Balance tracking
    let mut cur_bal_type = String::new(); // OPBD | CLBD | ...
    let mut cur_bal_credit = true;

    // Entry tracking
    let mut in_entry = false;
    let mut entry = NtryBuf::default();

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => {
                warnings.push(format!("XML parse error: {e}"));
                break;
            }
            Ok(Event::Eof) => break,

            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if name == "Ntry" {
                    entry = NtryBuf {
                        credit: true,
                        ..Default::default()
                    };
                    in_entry = true;
                }

                // Capture Ccy attribute on <Amt>
                if name == "Amt" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"Ccy" {
                            let ccy = String::from_utf8_lossy(&attr.value).to_string();
                            if in_entry {
                                entry.ccy = ccy.clone();
                            }
                            if !ccy.is_empty() && (currency == "RON" || currency.is_empty()) {
                                currency = ccy;
                            }
                        }
                    }
                }

                path.push(name);
            }

            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                // <Amt Ccy="XXX"/> — capture currency even when self-closing
                if name == "Amt" {
                    for attr in e.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"Ccy" {
                            let ccy = String::from_utf8_lossy(&attr.value).to_string();
                            if in_entry {
                                entry.ccy = ccy;
                            }
                        }
                    }
                }
            }

            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                if name == "Ntry" && in_entry {
                    if !entry.booking_date.is_empty() {
                        let signed = if entry.credit { entry.amt } else { -entry.amt };
                        let ccy = if entry.ccy.is_empty() {
                            currency.clone()
                        } else {
                            entry.ccy.clone()
                        };
                        let hash =
                            txn_hash(&entry.booking_date, &signed, entry.reference.as_deref());
                        txns.push(ParsedTxn {
                            booking_date: entry.booking_date.clone(),
                            value_date: entry.value_date.clone(),
                            amount: signed,
                            currency: ccy,
                            counterparty_name: entry.cpty_name.clone(),
                            counterparty_iban: entry.cpty_iban.clone(),
                            counterparty_cui: None, // CAMT.053 rarely has CUI in structured form
                            reference: entry.reference.clone(),
                            txn_hash: hash,
                        });
                    }
                    in_entry = false;
                }

                if path.last().map(|s| s.as_str()) == Some(&name) {
                    path.pop();
                }
            }

            Ok(Event::Text(e)) => {
                let text_val = match e.unescape() {
                    Ok(s) => s.trim().to_string(),
                    Err(_) => continue,
                };
                if text_val.is_empty() {
                    continue;
                }

                // Current element and its ancestors
                let tag = path.last().cloned().unwrap_or_default();
                let parent = path.iter().rev().nth(1).cloned().unwrap_or_default();
                let grandp = path.iter().rev().nth(2).cloned().unwrap_or_default();

                match tag.as_str() {
                    // Statement reference
                    "Id" if parent == "Stmt" && statement_ref.is_empty() => {
                        statement_ref = text_val;
                    }
                    // Statement date (ISO datetime or date)
                    "CreDtTm" | "FrDtTm"
                        if (parent == "Stmt" || grandp == "Stmt") && statement_date.is_empty() =>
                    {
                        statement_date = text_val.chars().take(10).collect();
                    }

                    // Balance type code
                    "Cd" if parent == "CdOrPrtry" => {
                        cur_bal_type = text_val;
                    }
                    // Balance direction
                    "CdtDbtInd" if parent == "Bal" => {
                        cur_bal_credit = text_val == "CRDT";
                    }
                    // Balance amount
                    "Amt" if parent == "Bal" => {
                        if let Ok(a) = Decimal::from_str(text_val.trim()) {
                            let signed = if cur_bal_credit { a } else { -a };
                            match cur_bal_type.as_str() {
                                "OPBD" => opening_balance = signed,
                                "CLBD" => closing_balance = signed,
                                _ => {}
                            }
                        }
                    }

                    // Entry fields
                    "Amt" if in_entry && parent == "Ntry" => {
                        if let Ok(a) = Decimal::from_str(text_val.trim()) {
                            entry.amt = a;
                        }
                    }
                    "CdtDbtInd" if in_entry => {
                        entry.credit = text_val == "CRDT";
                    }
                    "Dt" if in_entry && parent == "BookgDt" && entry.booking_date.is_empty() => {
                        entry.booking_date = text_val;
                    }
                    "Dt" if in_entry && parent == "ValDt" && entry.value_date.is_none() => {
                        entry.value_date = Some(text_val);
                    }
                    // Counterparty name (debtor or creditor)
                    "Nm" if in_entry
                        && (parent == "Dbtr" || parent == "Cdtr")
                        && entry.cpty_name.is_none() =>
                    {
                        entry.cpty_name = Some(text_val);
                    }
                    // Counterparty IBAN
                    "IBAN" if in_entry && entry.cpty_iban.is_none() => {
                        entry.cpty_iban = Some(text_val);
                    }
                    // Remittance info (unstructured)
                    "Ustrd" if in_entry => match &mut entry.reference {
                        None => entry.reference = Some(text_val),
                        Some(r) => {
                            r.push(' ');
                            r.push_str(&text_val);
                        }
                    },
                    _ => {}
                }
            }

            _ => {}
        }
        buf.clear();
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
        currency,
        txns,
        warnings,
        integrity_ok,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
<BkToCstmrStmt>
  <Stmt>
    <Id>STMT-2026-001</Id>
    <CreDtTm>2026-01-31T23:59:00</CreDtTm>
    <Bal>
      <Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp>
      <Amt Ccy="RON">5000.00</Amt>
      <CdtDbtInd>CRDT</CdtDbtInd>
    </Bal>
    <Bal>
      <Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
      <Amt Ccy="RON">5700.00</Amt>
      <CdtDbtInd>CRDT</CdtDbtInd>
    </Bal>
    <Ntry>
      <Amt Ccy="RON">1000.00</Amt>
      <CdtDbtInd>CRDT</CdtDbtInd>
      <BookgDt><Dt>2026-01-15</Dt></BookgDt>
      <ValDt><Dt>2026-01-15</Dt></ValDt>
      <NtryDtls>
        <TxDtls>
          <RltdPties>
            <Dbtr><Nm>CLIENT ALFA SRL</Nm></Dbtr>
          </RltdPties>
          <RmtInf><Ustrd>Factura F2026/001</Ustrd></RmtInf>
        </TxDtls>
      </NtryDtls>
    </Ntry>
    <Ntry>
      <Amt Ccy="RON">300.00</Amt>
      <CdtDbtInd>DBIT</CdtDbtInd>
      <BookgDt><Dt>2026-01-20</Dt></BookgDt>
      <NtryDtls>
        <TxDtls>
          <RltdPties>
            <Cdtr><Nm>FURNIZOR BETA SRL</Nm></Cdtr>
          </RltdPties>
          <RmtInf><Ustrd>Achizitie materiale</Ustrd></RmtInf>
        </TxDtls>
      </NtryDtls>
    </Ntry>
  </Stmt>
</BkToCstmrStmt>
</Document>"#;

    #[test]
    fn camt053_parses_two_entries() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.txns.len(), 2);
    }

    #[test]
    fn camt053_credit_positive_debit_negative() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.txns[0].amount, Decimal::from_str("1000").unwrap());
        assert_eq!(stmt.txns[1].amount, Decimal::from_str("-300").unwrap());
    }

    #[test]
    fn camt053_opening_closing_balances() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.opening_balance, Decimal::from_str("5000").unwrap());
        assert_eq!(stmt.closing_balance, Decimal::from_str("5700").unwrap());
    }

    #[test]
    fn camt053_integrity_ok() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        // 5000 + 1000 - 300 = 5700 == closing
        assert_eq!(stmt.integrity_ok, Some(true));
    }

    #[test]
    fn camt053_counterparty_names() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        assert_eq!(
            stmt.txns[0].counterparty_name.as_deref(),
            Some("CLIENT ALFA SRL")
        );
        assert_eq!(
            stmt.txns[1].counterparty_name.as_deref(),
            Some("FURNIZOR BETA SRL")
        );
    }

    #[test]
    fn camt053_statement_ref_and_date() {
        let stmt = parse_camt053(FIXTURE.as_bytes()).unwrap();
        assert_eq!(stmt.statement_ref, "STMT-2026-001");
        assert_eq!(stmt.statement_date, "2026-01-31");
    }

    #[test]
    fn camt053_integrity_mismatch_produces_warning() {
        // Closing is wrong on purpose
        let bad = r#"<?xml version="1.0"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
<BkToCstmrStmt><Stmt>
  <Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp><Amt Ccy="RON">1000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Bal>
  <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="RON">9999.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Bal>
  <Ntry><Amt Ccy="RON">200.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><BookgDt><Dt>2026-01-01</Dt></BookgDt></Ntry>
</Stmt></BkToCstmrStmt></Document>"#;
        let stmt = parse_camt053(bad.as_bytes()).unwrap();
        assert_eq!(stmt.integrity_ok, Some(false));
        assert!(!stmt.warnings.is_empty());
    }
}
