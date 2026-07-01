//! SAF-T D406 MasterFiles section builder.
//!
//! Populates:
//!   GeneralLedgerAccounts — one Account per chart_of_accounts row
//!   Customers             — one Customer per CUSTOMER/BOTH contact
//!   Suppliers             — one Supplier per SUPPLIER/BOTH contact
//!   TaxTable              — one TaxTableEntry per distinct (vat_category, rate) in the period
//!   UOMTable              — one UOMTableEntry per distinct unit used
//!   Products              — one Product per products row
//!   AnalysisTypeTable     — empty mandatory wrapper
//!   MovementTypeTable     — empty mandatory wrapper
//!   Owners                — empty mandatory wrapper
//!   Assets                — empty mandatory wrapper

use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use sqlx::Row;

use crate::anaf_decl::xml::{end_elem, start_elem, write_text_elem, XmlWriter};
use crate::db::companies::Company;
use crate::error::AppResult;

// ── AccountType mapping ────────────────────────────────────────────────────────
// XSD enum: Activ | Pasiv | Bifunctional
// Romanian PCG class → nature:
//   1 — Pasiv   (capital / equity accounts — normally credit balance)
//   2 — Activ   (assets / fixed assets — debit balance)
//   3 — Activ   (stock accounts — debit balance)
//   4 — Bifunctional (third-party accounts — either side)
//   5 — Activ   (treasury — debit balance)
//   6 — Activ   (expense accounts — debit balance)
//   7 — Pasiv   (revenue accounts — credit balance)
pub fn account_type_for_class(class: i64) -> &'static str {
    match class {
        1 => "Pasiv",
        2 => "Activ",
        3 => "Activ",
        4 => "Bifunctional",
        5 => "Activ",
        6 => "Activ",
        7 => "Pasiv",
        _ => "Bifunctional",
    }
}

/// Per-entity (account / customer / supplier) opening & closing net balance in RON, debit-positive.
/// Opening = Σ(debit−credit) for `transaction_date < date_from`; closing = through `date_to` (=
/// opening + period movements). Mirrors the `trial_balance` pattern (db/gl.rs). `match_col` is a
/// FIXED internal column name ("e.account_code" | "e.customer_id" | "e.supplier_id"), never user
/// input — safe to interpolate. Grouped: one query per entity kind.
async fn balance_map(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    match_col: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<std::collections::HashMap<String, (f64, f64)>> {
    let sql = format!(
        "SELECT {match_col} AS k, \
           COALESCE(SUM(CASE WHEN j.transaction_date < ?2 \
                             THEN CAST(e.debit AS REAL)-CAST(e.credit AS REAL) ELSE 0 END),0.0) AS opening_net, \
           COALESCE(SUM(CAST(e.debit AS REAL)-CAST(e.credit AS REAL)),0.0) AS closing_net \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date <= ?3 \
           AND {match_col} IS NOT NULL AND {match_col} != '' \
         GROUP BY {match_col}"
    );
    let rows = sqlx::query(&sql)
        .bind(company_id)
        .bind(date_from)
        .bind(date_to)
        .fetch_all(pool)
        .await?;
    let mut map = std::collections::HashMap::new();
    for r in &rows {
        let k: String = r.try_get("k").unwrap_or_default();
        if k.is_empty() {
            continue;
        }
        let opening: f64 = r.try_get("opening_net").unwrap_or(0.0);
        let closing: f64 = r.try_get("closing_net").unwrap_or(0.0);
        map.insert(k, (opening, closing));
    }
    Ok(map)
}

/// Emit a sign-based SAF-T balance: the positive magnitude on the side matching the net's sign,
/// exactly ONE element (the schema's `xs:choice` — emitting both Debit and Credit is a DUK reject).
/// `kind` is "Opening" or "Closing". Rounds to bani first so f64 noise near zero can't flip the side.
fn write_signed_balance(w: &mut XmlWriter, kind: &str, net: f64) -> AppResult<()> {
    let rounded = (net * 100.0).round() / 100.0;
    let tag = if rounded >= 0.0 {
        format!("{kind}DebitBalance")
    } else {
        format!("{kind}CreditBalance")
    };
    write_text_elem(w, &tag, &format!("{:.2}", rounded.abs()))
}

// ── Direction for tax-code lookup ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaxDirection {
    Sales,
    Purchase,
}

// ── 6-digit SAF-T tax code mapping ────────────────────────────────────────────
//
// Codes extracted from Ro_SAFT_SchemaDefCod_16.02.2026.xlsx:
//
// LIVRĂRI (Sales) sheet — primary codes for regular taxable supplies:
//   21%  → 310344  (L9, standard rate from 01.08.2025 — current)
//   19%  → 310309  (L9, standard rate until 31.07.2025 — historical)
//   9%   → 310310  (L10)
//   5%   → 310311  (L11)
//   11%  → 310351  (L, dedicated 11% sales code, Activ 01.08.2025)
//   AE (reverse-charge) → 310312  (L12)
//   K (intra-comm exempt) → 310301  (L1)
//   Z (export/zero) → 310313  (L13 — export VAT exempt with deduction)
//   E (exempt without ded) → 310326  (L26)
//   O (out-of-scope) → 310324  (L24)
//   0% fallback → 310324
//
// ACHIZITII DED 100% sheet — domestic purchases (not import, not simplification):
//   21%  → 301104  (A26_100, standard rate from 01.08.2025 — current)
//   19%  → 301101  (A26_100, standard rate until 31.07.2025 — historical)
//   9%   → 301102  (A27_100)
//   5%   → 301103  (A28_100)
//   11%  → 301105  (A27_100, active from 01.08.2025)
//   AE (reverse-charge/simplification, generic) → 300901  (A22_100 @ 19%)
//   K (intra-comm) → 300201  (A4_100 @ 19%)
//   Z / E / O (exempt/out-of-scope) → 308302  (exempt or non-taxable)
//   0% → 308302
//
// When a code cannot be determined precisely, the closest code is used and
// the behaviour is documented here.
pub fn saft_tax_code_dir(category: &str, rate: Decimal, dir: TaxDirection) -> &'static str {
    match dir {
        TaxDirection::Sales => match category {
            "S" | "AA" => {
                let r = rate.to_i64();
                match r {
                    Some(21) => "310344",
                    Some(19) => "310309",
                    Some(9) => "310310",
                    Some(5) => "310311",
                    // 11% standard sales — dedicated code 310351 (Activ 01.08.2025);
                    // NOT 310310 (that is the 9% code) — would misreport the rate.
                    Some(11) => "310351",
                    // Unknown positive rate: fall back to the current standard (21%)
                    // code 310344.  Historically 310309 was used (19%), but 21% is
                    // in-force from 2025-08-01 and is the correct "standard" bucket.
                    _ if rate > Decimal::ZERO => "310344",
                    _ => "310324", // 0% → out of scope
                }
            }
            "AE" => "310312", // reverse-charge
            "K" => "310301",  // intra-comm exempt (L1)
            "Z" => "310313",  // export/zero-rated with deduction right
            "E" => "310326",  // exempt without deduction
            "O" => "310324",  // out of scope
            "G" => "310324",  // governmental — map to out of scope
            _ => {
                // Unknown category — try by rate
                let r = rate.to_i64();
                match r {
                    Some(21) => "310344",
                    Some(19) => "310309",
                    Some(9) => "310310",
                    Some(5) => "310311",
                    // Unknown positive rate: fall back to current standard (21%)
                    _ => "310344",
                }
            }
        },
        TaxDirection::Purchase => match category {
            "S" | "AA" => {
                let r = rate.to_i64();
                match r {
                    Some(21) => "301104",
                    Some(19) => "301101",
                    Some(9) => "301102",
                    Some(5) => "301103",
                    Some(11) => "301105",
                    // Unknown positive rate: fall back to current standard (21%) code 301104.
                    // Historically 301101 (19%) was used, but 21% is in-force from 2025-08-01.
                    _ if rate > Decimal::ZERO => "301104",
                    _ => "308302", // 0% → exempt/non-taxable
                }
            }
            // simplification measures — closest generic reverse-charge code (21% era)
            "AE" => "300901",
            // intra-comm acquisition (closest generic code)
            "K" => "300201",
            "Z" | "E" | "O" | "G" => "308302", // exempt or non-taxable acquisitions
            _ => {
                let r = rate.to_i64();
                match r {
                    Some(21) => "301104",
                    Some(19) => "301101",
                    Some(9) => "301102",
                    Some(5) => "301103",
                    // Unknown positive rate: fall back to current standard (21%)
                    _ => "301104",
                }
            }
        },
    }
}

// ── Decimal to i64 helper (for integer VAT rates) ─────────────────────────────
trait DecimalToI64 {
    fn to_i64(self) -> Option<i64>;
}
impl DecimalToI64 for Decimal {
    fn to_i64(self) -> Option<i64> {
        if self.fract() == Decimal::ZERO {
            self.to_string().parse::<i64>().ok()
        } else {
            None
        }
    }
}

// ── Legacy wrapper (sales direction, for backward compat with TaxTable) ────────
pub fn saft_tax_code(category: &str, rate: Decimal) -> &'static str {
    saft_tax_code_dir(category, rate, TaxDirection::Sales)
}

// ── Tax description from category ─────────────────────────────────────────────
pub fn tax_description(category: &str, rate: Decimal) -> String {
    match category {
        "S" | "AA" if rate > Decimal::ZERO => format!("TVA cotă {rate}%"),
        "AE" => "TVA autolichidare (taxare inversă)".to_string(),
        "E" => "Scutit de TVA".to_string(),
        "Z" => "Zero-rated (export/intracomunitar)".to_string(),
        "O" => "În afara sferei TVA".to_string(),
        "K" => "Livrare intracomunitară scutită".to_string(),
        "G" => "Livrare scutită (guvernamentală)".to_string(),
        _ => format!("TVA {rate}%"),
    }
}

// ── Canonical partner ID (CUI-based, DUK-format) ──────────────────────────────
//
// The DUK validator cross-checks CustomerID/SupplierID between MasterFiles,
// SourceDocuments, and GL. Decompiled from D406TValidator.jar (ValidatorExtension1Impl
// + DECValidatorRoot.checkCUI), the accepted format is:
//
//   ID = "00" + CUI-digits   (IDType prefix "00" + Romanian CUI without "RO" prefix)
//
// Verified empirically:
//   "00" + 8-digit-valid-CUI  → valid only if checkCUI passes
//   "00" + 9-digit-valid-CUI  → valid
//   "0"  (bare zero string)   → always valid (DUK sentinel for "no partner")
//
// Use "0" as the no-partner sentinel (the DUK explicitly accepts the bare "0").
pub fn canonical_partner_id(id: &str, cui: &str) -> String {
    if !cui.is_empty() {
        let stripped = strip_ro(cui);
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            // Romanian CUI: prefix with IDType "00"
            return format!("00{}", stripped);
        }
    }
    // No CUI: distinguish a real-but-unidentified partner (has an internal id) —
    // emit the DUK-accepted anonymized ID — from a genuinely absent partner ("0").
    // (Multiple unidentified partners share the anonymized id; acceptable since they
    // are, by definition, anonymized.)
    if id.trim().is_empty() {
        "0".to_string()
    } else {
        "080000000000000".to_string()
    }
}

// ── Strip "RO" prefix from CUI/registration number ────────────────────────────
fn strip_ro(cui: &str) -> String {
    let s = cui.trim();
    let s = if s.to_uppercase().starts_with("RO") {
        &s[2..]
    } else {
        s
    };
    s.trim().to_string()
}

// ── Escape for XML text content ────────────────────────────────────────────────
use crate::anaf_decl::xml::trunc; // char-safe truncation, shared (anaf_decl::xml)
use crate::anaf_decl::xml_esc as esc;

/// SAF-T D406 `BankAccountNumber` for a partner: the recorded IBAN (trimmed, ≤35, XML-escaped)
/// when on file, else the `"N/A"` mention. The DUK validator rejects an empty value when the
/// `CompanyStructure/BankAccount` element is present, so the fallback must stay non-empty.
fn saft_bank_account_number(iban: &str) -> String {
    let t = iban.trim();
    if t.is_empty() {
        "N/A".to_string()
    } else {
        esc(&trunc(t, 35))
    }
}

// ── AmountStructure helper (RON-only; CurrencyCode=RON, CurrencyAmount=Amount) ─
pub fn write_amount_structure(w: &mut XmlWriter, elem: &str, amount: Decimal) -> AppResult<()> {
    start_elem(w, elem)?;
    write_text_elem(w, "Amount", &format!("{:.2}", amount))?;
    write_text_elem(w, "CurrencyCode", "RON")?;
    write_text_elem(w, "CurrencyAmount", &format!("{:.2}", amount))?;
    end_elem(w, elem)?;
    Ok(())
}

// ── AddressStructure helper ────────────────────────────────────────────────────
pub fn write_address(
    w: &mut XmlWriter,
    street: &str,
    city: &str,
    region: Option<&str>,
    postal: Option<&str>,
    country: &str,
    addr_type: Option<&str>,
) -> AppResult<()> {
    start_elem(w, "Address")?;
    if !street.is_empty() {
        write_text_elem(w, "StreetName", &esc(&trunc(street, 70)))?;
    }
    // City is required in AddressStructure
    let city_val = if city.is_empty() { "N/A" } else { city };
    write_text_elem(w, "City", &esc(&trunc(city_val, 35)))?;
    if let Some(pc) = postal {
        if !pc.is_empty() {
            write_text_elem(w, "PostalCode", &esc(&trunc(pc, 18)))?;
        }
    }
    if let Some(reg) = region {
        if !reg.is_empty() {
            // DUK rule: Region must be ISO-3166-2 "RO-CJ"/"RO-IF" — prefix with "RO-" unless already prefixed
            let reg_upper = reg.to_uppercase();
            let region_val = if reg_upper.starts_with("RO-") {
                reg_upper
            } else {
                format!("RO-{reg_upper}")
            };
            write_text_elem(w, "Region", &esc(&trunc(&region_val, 35)))?;
        }
    }
    // Country must be exactly 2 chars
    let country_2 = if country.len() >= 2 {
        &country[..2]
    } else {
        "RO"
    };
    write_text_elem(w, "Country", country_2)?;
    if let Some(at) = addr_type {
        write_text_elem(w, "AddressType", at)?;
    }
    end_elem(w, "Address")?;
    Ok(())
}

// ── ContactHeaderStructure — ContactPerson + Telephone (both required) ─────────
// XSD: ContactHeaderStructure has ContactPerson (required) + Telephone (required)
pub fn write_contact_header(
    w: &mut XmlWriter,
    first_name: &str,
    last_name: &str,
    telephone: &str,
) -> AppResult<()> {
    start_elem(w, "Contact")?;
    start_elem(w, "ContactPerson")?;
    write_text_elem(w, "FirstName", &esc(&trunc(first_name, 35)))?;
    write_text_elem(w, "LastName", &esc(&trunc(last_name, 70)))?;
    end_elem(w, "ContactPerson")?;
    // Telephone is required in ContactHeaderStructure (max 18 chars)
    write_text_elem(w, "Telephone", &trunc(telephone, 18))?;
    end_elem(w, "Contact")?;
    Ok(())
}

// ── GeneralLedgerAccounts ─────────────────────────────────────────────────────

pub async fn write_general_ledger_accounts(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    start_elem(w, "GeneralLedgerAccounts")?;
    let bal = balance_map(pool, company_id, "e.account_code", date_from, date_to).await?;

    let rows = sqlx::query(
        "SELECT account_code, account_name, account_class \
         FROM chart_of_accounts \
         WHERE company_id = ?1 AND active = 1 \
         ORDER BY account_code",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    for row in &rows {
        let code: String = row.try_get("account_code").unwrap_or_default();
        let name: String = row.try_get("account_name").unwrap_or_default();
        let class: Option<i64> = row.try_get("account_class").unwrap_or(None);
        let acct_type = account_type_for_class(class.unwrap_or(4));
        let (opening, closing) = bal.get(&code).copied().unwrap_or((0.0, 0.0));

        start_elem(w, "Account")?;
        write_text_elem(w, "AccountID", &esc(&trunc(&code, 70)))?;
        write_text_elem(w, "AccountDescription", &esc(&trunc(&name, 256)))?;
        write_text_elem(w, "AccountType", acct_type)?;
        // xs:choice: real opening/closing balance, sign-based (Debit if net ≥ 0, else Credit).
        write_signed_balance(w, "Opening", opening)?;
        write_signed_balance(w, "Closing", closing)?;
        end_elem(w, "Account")?;
    }

    end_elem(w, "GeneralLedgerAccounts")?;
    Ok(())
}

// ── Customers ─────────────────────────────────────────────────────────────────

pub async fn write_customers(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    start_elem(w, "Customers")?;
    let bal = balance_map(pool, company_id, "e.customer_id", date_from, date_to).await?;

    let rows = sqlx::query(
        "SELECT id, contact_type, cui, legal_name, address, city, county, country, iban \
         FROM contacts \
         WHERE company_id = ?1 \
           AND (contact_type = '\"CUSTOMER\"' OR contact_type = '\"BOTH\"' \
                OR contact_type = 'CUSTOMER' OR contact_type = 'BOTH') \
         ORDER BY legal_name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    for row in &rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let cui: String = row.try_get("cui").unwrap_or_default();
        let name: String = row.try_get("legal_name").unwrap_or_default();
        let addr: String = row.try_get("address").unwrap_or_default();
        let city: String = row.try_get("city").unwrap_or_default();
        let county: String = row.try_get("county").unwrap_or_default();
        let country: String = row.try_get("country").unwrap_or_else(|_| "RO".to_string());
        let iban: String = row.try_get("iban").unwrap_or_default();
        let country_2 = if country.len() >= 2 {
            &country[..2]
        } else {
            "RO"
        };

        // Canonical ID = "00" + CUI digits (RO-stripped), or "0" fallback.
        // MUST match the ID used in SourceDocuments/CustomerInfo and GL/CustomerID.
        let canon_id = canonical_partner_id(&id, &cui);

        start_elem(w, "Customer")?;
        // CompanyStructure (optional per XSD on Customer)
        start_elem(w, "CompanyStructure")?;
        // DUK rule: RegistrationNumber uses the same "00"+CUI format as CustomerID
        let reg_number = canon_id.clone();
        write_text_elem(w, "RegistrationNumber", &esc(&trunc(&reg_number, 35)))?;
        write_text_elem(w, "Name", &esc(&trunc(&name, 256)))?;
        // Address (required inside CompanyStructure)
        write_address(
            w,
            &addr,
            &city,
            Some(&county),
            None,
            country_2,
            Some("StreetAddress"),
        )?;
        // DUK rule: BankAccount (minOccurs=1 in CompanyHeaderStructure restriction):
        // CompanyStructure itself has minOccurs=0, but when present it needs a BankAccount with a
        // non-empty BankAccountNumber (DUK rejects an empty IBAN). Emit the partner's real IBAN
        // when on file; fall back to the "N/A" mention only when none is recorded.
        start_elem(w, "BankAccount")?;
        write_text_elem(w, "BankAccountNumber", &saft_bank_account_number(&iban))?;
        end_elem(w, "BankAccount")?;
        end_elem(w, "CompanyStructure")?;
        write_text_elem(w, "CustomerID", &esc(&trunc(&canon_id, 35)))?;
        write_text_elem(w, "AccountID", "4111")?;
        // xs:choice: real per-partner balance, sign-based (Debit if net ≥ 0, else Credit).
        let (opening, closing) = bal.get(&canon_id).copied().unwrap_or((0.0, 0.0));
        write_signed_balance(w, "Opening", opening)?;
        write_signed_balance(w, "Closing", closing)?;
        end_elem(w, "Customer")?;
    }

    end_elem(w, "Customers")?;
    Ok(())
}

// ── Suppliers ─────────────────────────────────────────────────────────────────

pub async fn write_suppliers(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    start_elem(w, "Suppliers")?;
    let bal = balance_map(pool, company_id, "e.supplier_id", date_from, date_to).await?;

    // Contacts that are SUPPLIER or BOTH
    let rows = sqlx::query(
        "SELECT id, contact_type, cui, legal_name, address, city, county, country, iban \
         FROM contacts \
         WHERE company_id = ?1 \
           AND (contact_type = '\"SUPPLIER\"' OR contact_type = '\"BOTH\"' \
                OR contact_type = 'SUPPLIER' OR contact_type = 'BOTH') \
         ORDER BY legal_name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let mut emitted_cuis: BTreeSet<String> = BTreeSet::new();

    for row in &rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let cui: String = row.try_get("cui").unwrap_or_default();
        let name: String = row.try_get("legal_name").unwrap_or_default();
        let addr: String = row.try_get("address").unwrap_or_default();
        let city: String = row.try_get("city").unwrap_or_default();
        let county: String = row.try_get("county").unwrap_or_default();
        let country: String = row.try_get("country").unwrap_or_else(|_| "RO".to_string());
        let iban: String = row.try_get("iban").unwrap_or_default();
        let country_2 = if country.len() >= 2 {
            &country[..2]
        } else {
            "RO"
        };

        // Canonical ID = "00" + CUI digits (RO-stripped), or "0" fallback.
        let canon_id = canonical_partner_id(&id, &cui);

        let dedup_key = canon_id.clone();
        if !emitted_cuis.insert(dedup_key) {
            continue;
        }

        start_elem(w, "Supplier")?;
        start_elem(w, "CompanyStructure")?;
        // DUK rule: RegistrationNumber uses the same "00"+CUI format as SupplierID
        let reg_number = canon_id.clone();
        write_text_elem(w, "RegistrationNumber", &esc(&trunc(&reg_number, 35)))?;
        write_text_elem(w, "Name", &esc(&trunc(&name, 256)))?;
        write_address(
            w,
            &addr,
            &city,
            Some(&county),
            None,
            country_2,
            Some("StreetAddress"),
        )?;
        // DUK rule: BankAccount required per ANAF business rules — emit the partner's real IBAN
        // when on file, else the non-empty "N/A" mention.
        start_elem(w, "BankAccount")?;
        write_text_elem(w, "BankAccountNumber", &saft_bank_account_number(&iban))?;
        end_elem(w, "BankAccount")?;
        end_elem(w, "CompanyStructure")?;
        write_text_elem(w, "SupplierID", &esc(&trunc(&canon_id, 35)))?;
        write_text_elem(w, "AccountID", "401")?;
        // xs:choice: real per-partner balance, sign-based (Credit if net < 0, else Debit).
        let (opening, closing) = bal.get(&canon_id).copied().unwrap_or((0.0, 0.0));
        write_signed_balance(w, "Opening", opening)?;
        write_signed_balance(w, "Closing", closing)?;
        end_elem(w, "Supplier")?;
    }

    // Also include distinct issuers from received_invoices not already covered
    let recv_rows = sqlx::query(
        "SELECT DISTINCT ri.issuer_cui, ri.issuer_name \
         FROM received_invoices ri \
         WHERE ri.company_id = ?1 \
           AND ri.issuer_cui != '' \
         ORDER BY ri.issuer_name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    for row in &recv_rows {
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
        // Use CUI as dedup key
        // Canonical ID for received invoice issuers = "00" + RO-stripped CUI digits
        //
        // KNOWN LIMITATION (found by the pre-publication audit; deferred): this uses
        // canonical_partner_id("", cui) with an EMPTY id, whereas the GL posts received-invoice
        // suppliers with canonical_partner_id(received_invoice_id, cui) (db/gl.rs). For a FOREIGN VAT id
        // (non-numeric, e.g. "DE811234") the CUI branch is skipped, so here the empty id yields "0" →
        // this loop `continue`s and emits NO Supplier record, but the GL/SourceDocuments carry the
        // anonymized id "080000000000000" → a dangling SupplierID with no MasterFiles/Suppliers entry
        // (a referential-integrity gap the D406 validator may flag). It only bites a foreign-CUI issuer
        // that is NOT also a saved no-CUI contact (the contacts loop above already emits the
        // "080000000000000" bucket in that case). Correct fix: emit one anonymized Supplier record for
        // the "080000000000000" bucket whenever any received-invoice issuer maps to it (with its GL
        // balance), then re-validate against the official Ro_SAFT XSD + DUK. Deferred to keep this a
        // tested, XSD-validated change rather than a blind edit.
        let issuer_canon_id = canonical_partner_id("", &issuer_cui);
        if issuer_cui.is_empty()
            || issuer_canon_id == "0"
            || !emitted_cuis.insert(issuer_canon_id.clone())
        {
            continue;
        }
        start_elem(w, "Supplier")?;
        start_elem(w, "CompanyStructure")?;
        // DUK rule: RegistrationNumber uses the same "00"+CUI format
        write_text_elem(w, "RegistrationNumber", &esc(&trunc(&issuer_canon_id, 35)))?;
        write_text_elem(w, "Name", &esc(&trunc(&issuer_name, 256)))?;
        // Minimal address (city required)
        start_elem(w, "Address")?;
        write_text_elem(w, "City", "N/A")?;
        write_text_elem(w, "Country", "RO")?;
        write_text_elem(w, "AddressType", "StreetAddress")?;
        end_elem(w, "Address")?;
        // DUK rule: BankAccount required per ANAF business rules — emit placeholder
        start_elem(w, "BankAccount")?;
        write_text_elem(w, "BankAccountNumber", "N/A")?;
        end_elem(w, "BankAccount")?;
        end_elem(w, "CompanyStructure")?;
        // SupplierID and RegistrationNumber use the same canonical "00"+CUI format
        write_text_elem(w, "SupplierID", &esc(&trunc(&issuer_canon_id, 35)))?;
        write_text_elem(w, "AccountID", "401")?;
        // xs:choice: real per-partner balance, sign-based (Credit if net < 0, else Debit).
        let (opening, closing) = bal.get(&issuer_canon_id).copied().unwrap_or((0.0, 0.0));
        write_signed_balance(w, "Opening", opening)?;
        write_signed_balance(w, "Closing", closing)?;
        end_elem(w, "Supplier")?;
    }

    end_elem(w, "Suppliers")?;
    Ok(())
}

// ── TaxTable ──────────────────────────────────────────────────────────────────

/// Collect distinct (vat_category, vat_rate) pairs used in the period across
/// both sales invoices (invoice_line_items) and received invoices.
pub async fn write_tax_table(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
) -> AppResult<()> {
    start_elem(w, "TaxTable")?;

    // Sales invoice lines
    let sales_rows = sqlx::query(
        "SELECT DISTINCT ili.vat_category, ili.vat_rate \
         FROM invoice_line_items ili \
         JOIN invoices i ON ili.invoice_id = i.id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 AND i.issue_date <= ?3 \
           AND i.status IN ('VALIDATED','STORNED')",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    let mut tax_keys: BTreeMap<(String, String), ()> = BTreeMap::new();
    for row in &sales_rows {
        let cat: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| "S".to_string());
        let rate: String = row.try_get("vat_rate").unwrap_or_else(|_| "0".to_string());
        tax_keys.insert((cat, rate), ());
    }

    // Default: always include standard TVA S/19% even if no invoices
    if tax_keys.is_empty() {
        tax_keys.insert(("S".to_string(), "19".to_string()), ());
    }

    for (category, rate_str) in tax_keys.keys() {
        let rate_dec = rate_str.parse::<Decimal>().unwrap_or(Decimal::ZERO);
        let tax_code = saft_tax_code(category, rate_dec);
        let description = tax_description(category, rate_dec);

        start_elem(w, "TaxTableEntry")?;
        // SAF-T TaxType code for VAT is "300" (consistent with Header + GL entries),
        // not the literal "TVA".
        write_text_elem(w, "TaxType", "300")?;
        write_text_elem(w, "Description", &esc(&trunc(&description, 256)))?;
        start_elem(w, "TaxCodeDetails")?;
        write_text_elem(w, "TaxCode", tax_code)?;
        write_text_elem(w, "TaxPercentage", rate_str)?;
        write_text_elem(w, "BaseRate", "1.0000")?;
        write_text_elem(w, "Country", "RO")?;
        end_elem(w, "TaxCodeDetails")?;
        end_elem(w, "TaxTableEntry")?;
    }

    end_elem(w, "TaxTable")?;
    Ok(())
}

// ── UOMTable ──────────────────────────────────────────────────────────────────

pub async fn write_uom_table(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
) -> AppResult<()> {
    start_elem(w, "UOMTable")?;

    // Distinct units from products
    let rows = sqlx::query(
        "SELECT DISTINCT unit FROM products WHERE company_id = ?1 AND unit != '' ORDER BY unit",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let mut emitted_codes: BTreeSet<String> = BTreeSet::new();

    // Always emit the canonical piece unit (H87)
    emitted_codes.insert("H87".to_string());
    write_uom_entry(w, "H87", "piece (bucată)")?;

    for row in &rows {
        let unit: String = row.try_get("unit").unwrap_or_default();
        let unit_trimmed = unit.trim().to_string();
        if unit_trimmed.is_empty() {
            continue;
        }
        let code = uom_to_rec20(&unit_trimmed);
        if !emitted_codes.insert(code.to_string()) {
            continue; // already emitted this Rec-20 code
        }
        let desc = uom_description(&unit_trimmed);
        write_uom_entry(w, code, desc)?;
    }

    end_elem(w, "UOMTable")?;
    Ok(())
}

fn write_uom_entry(w: &mut XmlWriter, uom_code: &str, desc: &str) -> AppResult<()> {
    start_elem(w, "UOMTableEntry")?;
    write_text_elem(w, "UnitOfMeasure", &trunc(uom_code, 9))?;
    write_text_elem(w, "Description", &esc(&trunc(desc, 256)))?;
    end_elem(w, "UOMTableEntry")?;
    Ok(())
}

// ── UN/ECE Rec-20 code lookup ──────────────────────────────────────────────────
//
// Extracted from Ro_SAFT_SchemaDefCod_16.02.2026.xlsx sheet "Unitati_masura".
// Maps common Romanian unit strings → UN/ECE Rec-20 codes.
// Unknown units default to "H87" (piece) per the ANAF recommendation.
pub fn uom_to_rec20(unit: &str) -> &'static str {
    match unit.to_lowercase().as_str() {
        // Piece / bucată
        "buc" | "bucata" | "bucată" | "buc." | "pcs" | "pc" | "pce" | "piece" | "pcs." => "H87",
        // Hour / oră
        "ora" | "ore" | "h" | "hr" | "hour" | "oră" => "HUR",
        // Kilogram
        "kg" | "kilogram" | "kilograme" => "KGM",
        // Gram
        "g" | "gram" | "grame" => "GRM",
        // Litre / litru
        "l" | "lt" | "ltr" | "litru" | "litre" | "liter" | "litri" => "LTR",
        // Millilitre
        "ml" | "millilitre" | "mililitru" => "MLT",
        // Metre / metru
        "m" | "metru" | "metre" | "meter" | "metri" => "MTR",
        // Square metre / metru pătrat
        "m2" | "mp" | "sqm" | "metru patrat" | "metru pătrat" => "MTK",
        // Cubic metre / metru cub
        "m3" | "mc" | "cbm" | "metru cub" => "MTQ",
        // Kilometre
        "km" | "kilometru" | "kilometre" => "KMT",
        // Tonne
        "t" | "tona" | "tonă" | "tonne" | "ton" => "TNE",
        // Set
        "set" => "SET",
        // Pair / pereche
        "pereche" | "pair" | "pr" => "PR",
        // Month / lună
        "luna" | "lună" | "month" | "luni" => "MON",
        // Day / zi
        "zi" | "zile" | "day" | "days" => "DAY",
        // Box / cutie — no perfect equivalent; use H87 (piece) as closest
        "cutie" | "box" | "bx" => "H87",
        // Package / pachet
        "pachet" | "pack" | "pach" => "H87",
        // Service unit
        "serviciu" | "serv" | "service" => "H87",
        // Default: piece
        _ => "H87",
    }
}

fn uom_description(unit: &str) -> &'static str {
    match uom_to_rec20(unit) {
        "H87" => "piece (bucată)",
        "HUR" => "hour (oră)",
        "KGM" => "kilogram",
        "GRM" => "gram",
        "LTR" => "litre (litru)",
        "MLT" => "millilitre (mililitru)",
        "MTR" => "metre (metru)",
        "MTK" => "square metre (metru pătrat)",
        "MTQ" => "cubic metre (metru cub)",
        "KMT" => "kilometre (kilometru)",
        "TNE" => "tonne (tonă)",
        "SET" => "set",
        "PR" => "pair (pereche)",
        "MON" => "month (lună)",
        "DAY" => "day (zi)",
        _ => "unit (unitate)",
    }
}

// ── Products ──────────────────────────────────────────────────────────────────

pub async fn write_products(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
) -> AppResult<()> {
    start_elem(w, "Products")?;

    let rows = sqlx::query(
        "SELECT id, name, unit, code FROM products WHERE company_id = ?1 ORDER BY name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    for row in &rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();
        let unit: String = row.try_get("unit").unwrap_or_else(|_| "buc".to_string());
        let code: Option<String> = row.try_get("code").unwrap_or(None);

        // ProductCode: use product code if available, else id (max 70 chars)
        let product_code = code.as_deref().filter(|c| !c.is_empty()).unwrap_or(&id);

        // ProductCommodityCode: ANAF accepts "00000000" (8 zeros) for services /
        // when no NC8 code is stored. Goods without a known NC8 → "99999999" catch-all.
        // Since the app cannot reliably distinguish goods from services, default to
        // "00000000" (services) for all products.
        let commodity_code = "00000000";

        let uom_raw = if unit.is_empty() {
            "buc"
        } else {
            unit.as_str()
        };
        // Map to UN/ECE Rec-20 code for UOMBase/UOMStandard
        let uom_code = uom_to_rec20(uom_raw);

        start_elem(w, "Product")?;
        write_text_elem(w, "ProductCode", &esc(&trunc(product_code, 70)))?;
        write_text_elem(w, "Description", &esc(&trunc(&name, 256)))?;
        write_text_elem(w, "ProductCommodityCode", commodity_code)?;
        // UOMBase (required)
        write_text_elem(w, "UOMBase", uom_code)?;
        // UOMStandard + UOMToUOMBaseConversionFactor (both required in the inner sequence)
        write_text_elem(w, "UOMStandard", uom_code)?;
        write_text_elem(w, "UOMToUOMBaseConversionFactor", "1")?;
        end_elem(w, "Product")?;
    }

    end_elem(w, "Products")?;
    Ok(())
}

// ── Empty mandatory wrappers ───────────────────────────────────────────────────

pub fn write_empty_analysis_type_table(w: &mut XmlWriter) -> AppResult<()> {
    // <AnalysisTypeTable/> — mandatory wrapper, zero entries
    start_elem(w, "AnalysisTypeTable")?;
    end_elem(w, "AnalysisTypeTable")?;
    Ok(())
}

pub fn write_empty_movement_type_table(w: &mut XmlWriter) -> AppResult<()> {
    start_elem(w, "MovementTypeTable")?;
    end_elem(w, "MovementTypeTable")?;
    Ok(())
}

pub fn write_empty_owners(w: &mut XmlWriter) -> AppResult<()> {
    start_elem(w, "Owners")?;
    end_elem(w, "Owners")?;
    Ok(())
}

/// Write the Assets section from fixed_assets table.
///
/// # Periodic vs Annual
/// The DUK production validator for declaration type L (periodic) treats Asset as max:0
/// children and rejects populated content. For type A (annual D406A), full content is emitted.
/// Pass `is_annual = true` to populate; for periodic (default), the empty wrapper is emitted.
///
/// XSD element order per Valuation:
///   AssetValuationType, ValuationClass,
///   AcquisitionAndProductionCostsBegin, AcquisitionAndProductionCostsEnd,
///   InvestmentSupport,
///   xs:choice(AssetLifeYear | AssetLifeMonth),
///   AssetAddition, Transfers, AssetDisposal,
///   BookValueBegin, DepreciationMethod, DepreciationPercentage,
///   DepreciationForPeriod, AppreciationForPeriod,
///   ExtraordinaryDepreciationsForPeriod, AccumulatedDepreciation, BookValueEnd
pub async fn write_assets(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    date_from: &str,
    date_to: &str,
    is_annual: bool,
) -> AppResult<()> {
    // For periodic L declarations, DUK enforces max:0 on Asset children.
    // Only populate for annual (D406A) declarations.
    if !is_annual {
        start_elem(w, "Assets")?;
        end_elem(w, "Assets")?;
        return Ok(());
    }

    let asset_rows = sqlx::query(
        "SELECT id, asset_code, account_id, description, valuation_class, \
                supplier_id, supplier_name, date_of_acquisition, start_up_date, \
                acquisition_cost, life_months, depreciation_method, depreciation_pct \
         FROM fixed_assets \
         WHERE company_id = ?1 AND active = 1 \
         ORDER BY asset_code ASC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    start_elem(w, "Assets")?;

    for row in &asset_rows {
        use sqlx::Row;
        let asset_code: String = row.try_get("asset_code").unwrap_or_default();
        let account_id: String = row
            .try_get("account_id")
            .unwrap_or_else(|_| "213".to_string());
        let description: String = row.try_get("description").unwrap_or_default();
        let valuation_class: String = row
            .try_get("valuation_class")
            .unwrap_or_else(|_| "Corporala".to_string());
        let supplier_id: String = row
            .try_get("supplier_id")
            .unwrap_or_else(|_| "0".to_string());
        let supplier_name: String = row.try_get("supplier_name").unwrap_or_default();
        let date_of_acquisition: String = row.try_get("date_of_acquisition").unwrap_or_default();
        let start_up_date: String = row
            .try_get("start_up_date")
            .unwrap_or_else(|_| date_of_acquisition.clone());
        let acquisition_cost_raw: String = row
            .try_get("acquisition_cost")
            .unwrap_or_else(|_| "0.00".to_string());
        let life_months_raw: i64 = row.try_get("life_months").unwrap_or(60);
        let depreciation_method: String = row
            .try_get("depreciation_method")
            .unwrap_or_else(|_| "liniara".to_string());
        let depreciation_pct_raw: String = row
            .try_get("depreciation_pct")
            .unwrap_or_else(|_| "0.00".to_string());

        use rust_decimal::Decimal;
        use std::str::FromStr;
        let cost = Decimal::from_str(acquisition_cost_raw.trim()).unwrap_or(Decimal::ZERO);

        // Build a minimal FixedAsset to reuse the depreciation calculator.
        let fake_asset = crate::db::assets::FixedAsset {
            id: String::new(),
            company_id: company_id.to_string(),
            asset_code: asset_code.clone(),
            account_id: account_id.clone(),
            description: description.clone(),
            valuation_class: valuation_class.clone(),
            supplier_id: supplier_id.clone(),
            supplier_name: supplier_name.clone(),
            date_of_acquisition: date_of_acquisition.clone(),
            start_up_date: start_up_date.clone(),
            acquisition_cost: acquisition_cost_raw.clone(),
            life_months: life_months_raw,
            depreciation_method: depreciation_method.clone(),
            depreciation_pct: depreciation_pct_raw.clone(),
            disposal_date: None,
            active: true,
            created_at: 0,
            updated_at: 0,
            fiscal_method: None,
            is_new: true,
            subgroup: None,
        };
        let depr = crate::db::assets::compute_depreciation(&fake_asset, date_from, date_to);

        // Compute depreciation_pct: if stored as non-zero use it; otherwise compute
        // from life_months (annual straight-line rate = 1/life_months * 12 * 100).
        let depr_pct = {
            let stored_pct =
                Decimal::from_str(depreciation_pct_raw.trim()).unwrap_or(Decimal::ZERO);
            if stored_pct > Decimal::ZERO {
                stored_pct
            } else if life_months_raw > 0 {
                // Annual rate % = 100 / (life_months / 12) = 1200 / life_months
                (Decimal::from(1200) / Decimal::from(life_months_raw))
                    .round_dp_with_strategy(4, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            } else {
                Decimal::ZERO
            }
        };

        start_elem(w, "Asset")?;
        write_text_elem(w, "AssetID", &esc(&trunc(&asset_code, 35)))?;
        write_text_elem(w, "AccountID", &esc(&trunc(&account_id, 35)))?;
        write_text_elem(w, "Description", &esc(&trunc(&description, 256)))?;

        // AssetSupplier (minOccurs=0) — emit only if we have a non-sentinel supplier
        if !supplier_id.is_empty() && supplier_id != "0" {
            start_elem(w, "AssetSupplier")?;
            write_text_elem(
                w,
                "SupplierName",
                &esc(&trunc(
                    if supplier_name.is_empty() {
                        "N/A"
                    } else {
                        &supplier_name
                    },
                    70,
                )),
            )?;
            write_text_elem(w, "SupplierID", &esc(&trunc(&supplier_id, 35)))?;
            // PostalAddress is required inside AssetSupplier
            start_elem(w, "PostalAddress")?;
            write_text_elem(w, "City", "N/A")?;
            write_text_elem(w, "Country", "RO")?;
            write_text_elem(w, "AddressType", "StreetAddress")?;
            end_elem(w, "PostalAddress")?;
            end_elem(w, "AssetSupplier")?;
        }

        write_text_elem(w, "DateOfAcquisition", &date_of_acquisition)?;
        write_text_elem(w, "StartUpDate", &start_up_date)?;

        // Valuations (required, minOccurs=1 child Valuation)
        start_elem(w, "Valuations")?;
        start_elem(w, "Valuation")?;
        // AssetValuationType: "fiscal" = Romanian fiscal (tax) valuation basis
        write_text_elem(w, "AssetValuationType", "fiscal")?;
        write_text_elem(w, "ValuationClass", &esc(&trunc(&valuation_class, 9)))?;
        write_text_elem(
            w,
            "AcquisitionAndProductionCostsBegin",
            &format!("{:.2}", cost),
        )?;
        write_text_elem(
            w,
            "AcquisitionAndProductionCostsEnd",
            &format!("{:.2}", cost),
        )?;
        // InvestmentSupport: always 0 (we don't track grants)
        write_text_elem(w, "InvestmentSupport", "0.00")?;
        // xs:choice: AssetLifeYear | AssetLifeMonth — use months
        write_text_elem(w, "AssetLifeMonth", &life_months_raw.to_string())?;
        // AssetAddition: cost added during the period (0 for existing assets)
        write_text_elem(w, "AssetAddition", "0.00")?;
        write_text_elem(w, "Transfers", "0.00")?;
        write_text_elem(w, "AssetDisposal", "0.00")?;
        write_text_elem(
            w,
            "BookValueBegin",
            &format!("{:.2}", depr.book_value_begin),
        )?;
        write_text_elem(
            w,
            "DepreciationMethod",
            &esc(&trunc(&depreciation_method, 35)),
        )?;
        write_text_elem(w, "DepreciationPercentage", &format!("{:.4}", depr_pct))?;
        write_text_elem(
            w,
            "DepreciationForPeriod",
            &format!("{:.2}", depr.for_period),
        )?;
        // AppreciationForPeriod: always 0 for straight-line
        write_text_elem(w, "AppreciationForPeriod", "0.00")?;
        // ExtraordinaryDepreciationsForPeriod: required wrapper, emit one zero row
        start_elem(w, "ExtraordinaryDepreciationsForPeriod")?;
        start_elem(w, "ExtraordinaryDepreciationForPeriod")?;
        write_text_elem(w, "ExtraordinaryDepreciationMethod", "none")?;
        write_text_elem(w, "ExtraordinaryDepreciationAmountForPeriod", "0.00")?;
        end_elem(w, "ExtraordinaryDepreciationForPeriod")?;
        end_elem(w, "ExtraordinaryDepreciationsForPeriod")?;
        write_text_elem(
            w,
            "AccumulatedDepreciation",
            &format!("{:.2}", depr.accumulated_end),
        )?;
        write_text_elem(w, "BookValueEnd", &format!("{:.2}", depr.book_value_end))?;
        end_elem(w, "Valuation")?;
        end_elem(w, "Valuations")?;

        end_elem(w, "Asset")?;
    }

    end_elem(w, "Assets")?;
    Ok(())
}

// ── MasterFiles top-level ─────────────────────────────────────────────────────

pub async fn write_master_files(
    w: &mut XmlWriter,
    pool: &sqlx::SqlitePool,
    company: &Company,
    date_from: &str,
    date_to: &str,
    is_annual: bool,
) -> AppResult<()> {
    start_elem(w, "MasterFiles")?;

    // GeneralLedgerAccounts and Assets are POPULATED in both L and A profiles.
    write_general_ledger_accounts(w, pool, &company.id, date_from, date_to).await?;

    if is_annual {
        // A-profile: Customers, Suppliers, TaxTable, UOMTable, Products are
        // forbidden (max:0 children in A) — emit empty wrappers only.
        start_elem(w, "Customers")?;
        end_elem(w, "Customers")?;
        start_elem(w, "Suppliers")?;
        end_elem(w, "Suppliers")?;
        start_elem(w, "TaxTable")?;
        end_elem(w, "TaxTable")?;
        start_elem(w, "UOMTable")?;
        end_elem(w, "UOMTable")?;
        // AnalysisTypeTable — empty (omit or empty; A allows omitting)
        write_empty_analysis_type_table(w)?;
        start_elem(w, "MovementTypeTable")?;
        end_elem(w, "MovementTypeTable")?;
        start_elem(w, "Products")?;
        end_elem(w, "Products")?;
        start_elem(w, "Owners")?;
        end_elem(w, "Owners")?;
    } else {
        // L-profile: all MasterFiles sections populated normally.
        write_customers(w, pool, &company.id, date_from, date_to).await?;
        write_suppliers(w, pool, &company.id, date_from, date_to).await?;
        write_tax_table(w, pool, &company.id, date_from, date_to).await?;
        write_uom_table(w, pool, &company.id).await?;
        write_empty_analysis_type_table(w)?;
        write_empty_movement_type_table(w)?;
        write_products(w, pool, &company.id).await?;
        write_empty_owners(w)?;
    }

    // Assets: populated for A, empty wrapper for L.
    write_assets(w, pool, &company.id, date_from, date_to, is_annual).await?;

    end_elem(w, "MasterFiles")?;
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn canonical_partner_id_edge_cases() {
        // RO CUI → IDType "00" + the bare digits (RO prefix stripped).
        assert_eq!(canonical_partner_id("anything", "RO12345678"), "0012345678");
        assert_eq!(canonical_partner_id("anything", "12345678"), "0012345678");
        // No CUI but a real internal id → the DUK-accepted anonymized partner id.
        assert_eq!(canonical_partner_id("contact-7", ""), "080000000000000");
        // No CUI and no id → genuinely absent partner.
        assert_eq!(canonical_partner_id("", ""), "0");
        // Non-numeric CUI (foreign / malformed) → treated as unidentified, not "00<garbage>".
        assert_eq!(canonical_partner_id("c1", "DE811234"), "080000000000000");
    }

    #[test]
    fn account_type_class_mapping() {
        assert_eq!(account_type_for_class(1), "Pasiv");
        assert_eq!(account_type_for_class(2), "Activ");
        assert_eq!(account_type_for_class(3), "Activ");
        assert_eq!(account_type_for_class(4), "Bifunctional");
        assert_eq!(account_type_for_class(5), "Activ");
        assert_eq!(account_type_for_class(6), "Activ");
        assert_eq!(account_type_for_class(7), "Pasiv");
        assert_eq!(account_type_for_class(0), "Bifunctional"); // fallback
    }

    #[test]
    fn saft_tax_code_mapping_sales() {
        // Sales codes — from Livrari sheet
        assert_eq!(saft_tax_code("S", Decimal::from(19)), "310309");
        assert_eq!(saft_tax_code("S", Decimal::from(21)), "310344");
        assert_eq!(saft_tax_code("S", Decimal::from(9)), "310310");
        assert_eq!(saft_tax_code("S", Decimal::from(5)), "310311");
        // 11% sales has its own code (310351) — must NOT collapse to the 9% code.
        assert_eq!(saft_tax_code("S", Decimal::from(11)), "310351");
        assert_ne!(saft_tax_code("S", Decimal::from(11)), "310310");
        assert_eq!(saft_tax_code("AE", Decimal::ZERO), "310312");
        assert_eq!(saft_tax_code("E", Decimal::ZERO), "310326");
        assert_eq!(saft_tax_code("Z", Decimal::ZERO), "310313");
        assert_eq!(saft_tax_code("O", Decimal::ZERO), "310324");
        assert_eq!(saft_tax_code("K", Decimal::ZERO), "310301");
        assert_eq!(saft_tax_code("G", Decimal::ZERO), "310324");
        assert_eq!(saft_tax_code("UNKNOWN", Decimal::from(19)), "310309"); // fallback
    }

    #[test]
    fn saft_tax_code_mapping_purchase() {
        // Purchase codes — from Achizitii ded 100% sheet (domestic, not import)
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(19), TaxDirection::Purchase),
            "301101"
        );
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(21), TaxDirection::Purchase),
            "301104"
        );
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(9), TaxDirection::Purchase),
            "301102"
        );
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(5), TaxDirection::Purchase),
            "301103"
        );
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(11), TaxDirection::Purchase),
            "301105"
        );
        assert_eq!(
            saft_tax_code_dir("AE", Decimal::ZERO, TaxDirection::Purchase),
            "300901"
        );
        assert_eq!(
            saft_tax_code_dir("E", Decimal::ZERO, TaxDirection::Purchase),
            "308302"
        );
        assert_eq!(
            saft_tax_code_dir("Z", Decimal::ZERO, TaxDirection::Purchase),
            "308302"
        );
    }

    // ── FIX 4: SAF-T standard fallback → 21% codes ────────────────────────────

    /// An unmapped positive rate for 'S'/'AA' (sales) must fall back to 310344 (21%), not 310309 (19%).
    #[test]
    fn saft_standard_sales_unmapped_positive_rate_falls_back_to_21_code() {
        // Rate 15% is not in the explicit match — should hit the `_ if rate > ZERO` fallback.
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(15), TaxDirection::Sales),
            "310344",
            "Unmapped positive rate for S/Sales must fall back to 310344 (21% current standard)"
        );
        // Same for unknown category with positive rate
        assert_eq!(
            saft_tax_code_dir("UNKNOWN", Decimal::from(15), TaxDirection::Sales),
            "310344",
            "Unknown category with positive rate must fall back to 310344 (21%)"
        );
        // Explicit 19% mapping must still return 310309 (historical, not the fallback)
        assert_eq!(
            saft_tax_code("S", Decimal::from(19)),
            "310309",
            "Explicit 19% sales must still map to 310309"
        );
    }

    /// An unmapped positive rate for 'S'/'AA' (purchase) must fall back to 301104 (21%), not 301101 (19%).
    #[test]
    fn saft_standard_purchase_unmapped_positive_rate_falls_back_to_21_code() {
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(15), TaxDirection::Purchase),
            "301104",
            "Unmapped positive rate for S/Purchase must fall back to 301104 (21% current standard)"
        );
        // Unknown category with positive rate
        assert_eq!(
            saft_tax_code_dir("UNKNOWN", Decimal::from(15), TaxDirection::Purchase),
            "301104",
            "Unknown category with positive rate must fall back to 301104 (21%)"
        );
        // Explicit 19% mapping must still return 301101 (historical)
        assert_eq!(
            saft_tax_code_dir("S", Decimal::from(19), TaxDirection::Purchase),
            "301101",
            "Explicit 19% purchase must still map to 301101"
        );
    }

    #[test]
    fn uom_rec20_known_units() {
        assert_eq!(uom_to_rec20("buc"), "H87");
        assert_eq!(uom_to_rec20("ora"), "HUR");
        assert_eq!(uom_to_rec20("kg"), "KGM");
        assert_eq!(uom_to_rec20("l"), "LTR");
        assert_eq!(uom_to_rec20("m"), "MTR");
        assert_eq!(uom_to_rec20("mp"), "MTK");
        assert_eq!(uom_to_rec20("mc"), "MTQ");
        assert_eq!(uom_to_rec20("t"), "TNE");
        assert_eq!(uom_to_rec20("km"), "KMT");
        assert_eq!(uom_to_rec20("set"), "SET");
        assert_eq!(uom_to_rec20("pereche"), "PR");
        assert_eq!(uom_to_rec20("luna"), "MON");
        assert_eq!(uom_to_rec20("zi"), "DAY");
        assert_eq!(uom_to_rec20("UNKNOWN_UOM"), "H87"); // default to piece
    }

    #[tokio::test]
    async fn gl_account_balances_real_and_sign_based() {
        // Real opening/closing balances (replacing the old hardcoded "0.00"), variant chosen by the
        // ACTUAL net sign: a receivable (4111) → Debit, a payable (401) → Credit, and a pre-period
        // bank entry → a non-zero OpeningDebitBalance carried from before period_from.
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id,cui,legal_name,address,city,county,country) \
             VALUES ('co1','11111111','Test SRL','S','B','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (code, cls) in [("4111", 4), ("401", 4), ("5121", 5)] {
            sqlx::query(
                "INSERT INTO chart_of_accounts \
                 (id, company_id, account_code, account_name, account_class, active, created_at, updated_at) \
                 VALUES (?1,'co1',?2,?3,?4,1,0,0)",
            )
            .bind(format!("acc-{code}"))
            .bind(code)
            .bind(format!("Cont {code}"))
            .bind(cls)
            .execute(&pool)
            .await
            .unwrap();
        }
        async fn post(
            pool: &sqlx::SqlitePool,
            jid: &str,
            date: &str,
            acct: &str,
            d: &str,
            c: &str,
        ) {
            sqlx::query(
                "INSERT INTO gl_journal (id, company_id, journal_id, journal_type, transaction_id, \
                 transaction_date, source_type, source_id) \
                 VALUES (?1,'co1','DIVERSE','DIVERSE',?1,?2,'TEST',?1)",
            )
            .bind(jid)
            .bind(date)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit) \
                 VALUES (?1,?2,1,?3,?4,?5)",
            )
            .bind(format!("e-{jid}"))
            .bind(jid)
            .bind(acct)
            .bind(d)
            .bind(c)
            .execute(pool)
            .await
            .unwrap();
        }
        post(&pool, "j-open", "2026-01-15", "5121", "200.00", "0.00").await; // before period
        post(&pool, "j-cust", "2026-02-10", "4111", "1000.00", "0.00").await; // receivable (debit)
        post(&pool, "j-supp", "2026-02-12", "401", "0.00", "500.00").await; // payable (credit)

        let mut w = crate::anaf_decl::xml::new_writer().unwrap();
        write_general_ledger_accounts(&mut w, &pool, "co1", "2026-02-01", "2026-02-28")
            .await
            .unwrap();
        let xml = crate::anaf_decl::xml::finish(w).unwrap();

        assert!(
            xml.contains("<OpeningDebitBalance>200.00</OpeningDebitBalance>"),
            "pre-period opening balance not carried into Opening: {xml}"
        );
        assert!(
            xml.contains("<ClosingDebitBalance>1000.00</ClosingDebitBalance>"),
            "receivable closing must be a Debit balance: {xml}"
        );
        assert!(
            xml.contains("<ClosingCreditBalance>500.00</ClosingCreditBalance>"),
            "payable closing must be a positive Credit balance (sign-based variant): {xml}"
        );
    }
}
