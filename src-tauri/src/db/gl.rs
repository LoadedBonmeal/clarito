//! GL auto-posting engine — Registru jurnal cu dublu-intrare per OMFP 1802/2014.
//!
//! ## Șabloane de înregistrare (standard RO)
//!
//! **Factură emisă** (VALIDATED / STORNED):
//!   D 4111 (Clienți)         = gross
//!   C 707 (Venituri mărfuri) = net    [implicit; 704 pt servicii — neimplementat în v1]
//!   C 4427 (TVA colectată)   = VAT
//!
//! **Factură primită** (received_invoice_vat_lines per cotă):
//!   D 607 (Cheltuieli mărfuri) = net per linie VAT  [implicit; 371 neimplementat în v1]
//!   D 4426 (TVA deductibilă)   = VAT per linie
//!   C 401 (Furnizori)          = gross
//!
//! **Plată client primită**:
//!   D 5121 (Bancă lei)   = amount
//!   C 4111 (Clienți)     = amount
//!
//! **Taxare inversă / autolichidare** (categorie AE sau K pe achiziții):
//!   Înregistrare de bază (607/401) CA MAI SUS, plus:
//!   D 4426 (TVA deductibilă) = VAT calculat
//!   C 4427 (TVA colectată)   = VAT calculat   (efect net TVA = 0)
//!
//! **Storno / notă de credit** (storno_of_invoice_id != NULL sau tip 381):
//!   Aceleași conturi ca factura de vânzare dar cu SUME NEGATIVE (stornare în roșu).
//!
//! ## Simplificări / amânări explicite (v1)
//!   - Cont venituri fix 707 (nu distingem 704 servicii vs 707 mărfuri — lipsă câmp tip).
//!   - Cont cheltuieli fix 607 (nu distingem 371 stocuri vs 607 — lipsă câmp tip achiziție).
//!   - Diferențe de curs FX (665/765 vs 4111): neimplementat în v1 deoarece rata la
//!     data plății nu e stocată în tabela payments (câmp currency există, dar nu rate).
//!   - Facturi primite fără defalcare TVA (net_amount IS NULL): omise din postare
//!     (înregistrate ca count în GlPostResult.skipped_received).

use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::anaf_decl::cash_vat::{allocate_collection, RateBucket};
use crate::anaf_decl::saft::masterfiles::{canonical_partner_id, saft_tax_code_dir, TaxDirection};
use crate::db::models::new_id;
use crate::error::AppResult;
use crate::ubl::fx::{amount_to_ron, parse_rate};

// ─── Result types ──────────────────────────────────────────────────────────────

/// Rezultatul unei rulări `generate_gl_entries`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlPostResult {
    pub journals_inserted: i64,
    pub entries_inserted: i64,
    pub journals_replaced: i64,
    pub skipped_received: i64,
}

/// Raport de reconciliere GL ↔ D300.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    /// Σdebit == Σcredit pe toate intrările GL din perioadă.
    pub balanced: bool,
    /// Σdebit total (RON, 2 zecimale ca text).
    pub total_debit: String,
    /// Σcredit total (RON, 2 zecimale ca text).
    pub total_credit: String,
    /// Σ credit cont 4427 din GL (TVA colectată).
    pub vat_collected_gl: String,
    /// TVA colectată din D300 (recomputată pe aceeași perioadă).
    pub vat_collected_d300: String,
    /// Σ debit cont 4426 din GL (TVA deductibilă).
    pub vat_deductible_gl: String,
    /// TVA deductibilă din D300.
    pub vat_deductible_d300: String,
    /// Listă de discrepanțe (gol înseamnă reconciliere completă).
    pub discrepancies: Vec<String>,
}

// ─── Internal structs ─────────────────────────────────────────────────────────

struct GlJournal {
    id: String,
    company_id: String,
    journal_id: String,
    journal_type: String,
    transaction_id: String,
    transaction_date: String,
    description: Option<String>,
    source_type: String,
    source_id: String,
    customer_id: Option<String>,
    supplier_id: Option<String>,
}

struct GlEntry {
    id: String,
    record_id: i64,
    account_code: String,
    debit: Decimal,
    credit: Decimal,
    partner_cui: Option<String>,
    customer_id: Option<String>,
    supplier_id: Option<String>,
    tax_type: String,
    tax_code: String,
    tax_percentage: Option<String>,
    tax_base: Option<String>,
    tax_amount: Option<String>,
}

// ─── Decimal helpers ──────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    match Decimal::from_str(s.trim()) {
        Ok(d) => d,
        Err(_) => {
            // Don't silently zero a corrupted amount without a trace — a malformed
            // value would otherwise produce a zero-valued GL entry with no signal.
            if !s.trim().is_empty() {
                tracing::warn!(value = %s, "GL: valoare monetară invalidă — se folosește 0");
            }
            Decimal::ZERO
        }
    }
}

fn fmt_dec(d: Decimal) -> String {
    format!("{:.2}", d)
}

// ─── Tax code helpers ─────────────────────────────────────────────────────────

fn sales_tax_code_str(category: &str, rate: Decimal) -> String {
    saft_tax_code_dir(category, rate, TaxDirection::Sales).to_string()
}

fn purchase_tax_code_str(category: &str, rate: Decimal) -> String {
    saft_tax_code_dir(category, rate, TaxDirection::Purchase).to_string()
}

// ─── Posting templates (pure functions) ──────────────────────────────────────

/// Postare factură emisă (vânzări) — per-rate groups approach.
///
/// `vat_groups`: slice of (net_ron, vat_ron, category, rate) — one entry per
/// (vat_category, vat_rate) group.  The gross (D 4111) is computed as
/// Σnet_ron + Σvat_ron so the journal always balances exactly regardless of
/// any rounding skew in the stored total_amount column.
///
/// Returns (journal, entries).  Storno = same accounts with negated amounts.
#[allow(clippy::too_many_arguments)]
fn post_sales_invoice(
    company_id: &str,
    invoice_id: &str,
    full_number: &str,
    issue_date: &str,
    contact_id_raw: &str,
    partner_cui: Option<&str>,
    vat_groups: &[(Decimal, Decimal, String, Decimal)], // (net, vat, category, rate)
    is_storno: bool,
    // TVA la încasare: when true, the standard-rate ("S") output VAT is not yet exigible at
    // invoice date — credit 4428 "TVA neexigibilă" instead of 4427; it transfers to 4427 on
    // collection (see post_payment). Excluded categories (AE/E/Z/K) keep 4427.
    cash_vat_applies: bool,
) -> (GlJournal, Vec<GlEntry>) {
    // Use canonical partner ID (CUI-based) so it matches MasterFiles and SourceDocuments
    let contact_id = canonical_partner_id(contact_id_raw, partner_cui.unwrap_or(""));
    // Storno: negăm toate sumele (stornare în roșu conform OMFP 1802/2014).
    let sign = if is_storno {
        Decimal::NEGATIVE_ONE
    } else {
        Decimal::ONE
    };

    // FIX 2: Compute gross as Σnet + Σvat so the GL always balances exactly,
    // independent of any rounding discrepancy in the stored total_amount.
    let gross_raw: Decimal = vat_groups.iter().map(|(n, v, _, _)| *n + *v).sum();
    let gross = gross_raw * sign;

    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "VANZARI".to_string(),
        journal_type: "SALES".to_string(),
        transaction_id: full_number.to_string(),
        transaction_date: issue_date.to_string(),
        description: Some(format!(
            "{} {}",
            if is_storno {
                "Storno factura"
            } else {
                "Factura"
            },
            full_number
        )),
        source_type: "INVOICE".to_string(),
        source_id: invoice_id.to_string(),
        customer_id: Some(contact_id.clone()),
        supplier_id: None,
    };

    let mut entries: Vec<GlEntry> = Vec::new();
    let mut record_id: i64 = 1;

    // D 4111 Clienți = gross (linie cont terțe, fără TVA detaliat)
    entries.push(GlEntry {
        id: new_id(),
        record_id,
        account_code: "4111".to_string(),
        debit: if gross > Decimal::ZERO {
            gross
        } else {
            Decimal::ZERO
        },
        credit: if gross < Decimal::ZERO {
            -gross
        } else {
            Decimal::ZERO
        },
        partner_cui: partner_cui.map(|s| s.to_string()),
        customer_id: Some(contact_id.clone()),
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    });
    record_id += 1;

    // FIX 1: Per-(category, rate) group: C 707 + C 4427 — mirrors purchase posting.
    for (net_ron, vat_ron, category, rate) in vat_groups {
        let net = *net_ron * sign;
        let vat = *vat_ron * sign;
        let tc = sales_tax_code_str(category, *rate);
        let rate_str = fmt_dec(*rate);

        // C 707 Venituri mărfuri = net (poartă info TVA per cotă)
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "707".to_string(),
            debit: if net < Decimal::ZERO {
                -net
            } else {
                Decimal::ZERO
            },
            credit: if net > Decimal::ZERO {
                net
            } else {
                Decimal::ZERO
            },
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "300".to_string(),
            tax_code: tc.clone(),
            tax_percentage: Some(rate_str.clone()),
            tax_base: Some(fmt_dec(net)),
            tax_amount: Some(fmt_dec(vat)),
        });
        record_id += 1;

        // C 4427 TVA colectată = VAT (only when group has VAT). Under cash VAT the standard
        // "S" VAT is not yet exigible → 4428 "TVA neexigibilă" (transferred to 4427 on
        // collection). Excluded categories keep normal exigibility (4427).
        let vat_account = if cash_vat_applies && category == "S" {
            "4428"
        } else {
            "4427"
        };
        if *vat_ron != Decimal::ZERO {
            entries.push(GlEntry {
                id: new_id(),
                record_id,
                account_code: vat_account.to_string(),
                debit: if vat < Decimal::ZERO {
                    -vat
                } else {
                    Decimal::ZERO
                },
                credit: if vat > Decimal::ZERO {
                    vat
                } else {
                    Decimal::ZERO
                },
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "300".to_string(),
                tax_code: tc,
                tax_percentage: Some(rate_str),
                tax_base: None,
                tax_amount: None,
            });
            record_id += 1;
        }
    }

    (journal, entries)
}

/// Postare factură primită (achiziții) — un grup de linii VAT (per cotă).
///
/// Returnează (journal, entries).
/// Pentru reverse-charge (AE / K): adaugă leg D 4426 = C 4427.
fn post_purchase_invoice(
    company_id: &str,
    received_invoice_id: &str,
    doc_number: &str,
    issue_date: &str,
    issuer_cui: &str,
    gross_ron: Decimal,
    vat_lines: &[(Decimal, Decimal, String, Decimal)], // (net, vat, category, rate)
) -> (GlJournal, Vec<GlEntry>) {
    // Canonical supplier ID = RO-stripped CUI digits
    let supplier_canon = canonical_partner_id(received_invoice_id, issuer_cui);
    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "CUMPARARI".to_string(),
        journal_type: "PURCHASE".to_string(),
        transaction_id: doc_number.to_string(),
        transaction_date: issue_date.to_string(),
        description: Some(format!("Factura primita {doc_number}")),
        source_type: "RECEIVED_INVOICE".to_string(),
        source_id: received_invoice_id.to_string(),
        customer_id: None,
        supplier_id: Some(supplier_canon.clone()),
    };

    let mut entries: Vec<GlEntry> = Vec::new();
    let mut record_id: i64 = 1;

    // Per-linie TVA: D 607 + D 4426
    for (net, vat, category, rate) in vat_lines {
        let tc = purchase_tax_code_str(category, *rate);
        let rate_str = fmt_dec(*rate);
        let is_reverse_charge = category == "AE" || category == "K";

        // D 607 Cheltuieli mărfuri = net
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "607".to_string(),
            debit: *net,
            credit: Decimal::ZERO,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "300".to_string(),
            tax_code: tc.clone(),
            tax_percentage: Some(rate_str.clone()),
            tax_base: Some(fmt_dec(*net)),
            tax_amount: Some(fmt_dec(*vat)),
        });
        record_id += 1;

        // D 4426 TVA deductibilă = VAT (pentru taxe non-reverse-charge, sau auto-assessment)
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "4426".to_string(),
            debit: *vat,
            credit: Decimal::ZERO,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "300".to_string(),
            tax_code: tc.clone(),
            tax_percentage: Some(rate_str.clone()),
            tax_base: None,
            tax_amount: None,
        });
        record_id += 1;

        // Reverse-charge: D 4426 = C 4427 (auto-assessment, net TVA = 0)
        if is_reverse_charge && *vat > Decimal::ZERO {
            entries.push(GlEntry {
                id: new_id(),
                record_id,
                account_code: "4427".to_string(),
                debit: Decimal::ZERO,
                credit: *vat,
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "300".to_string(),
                tax_code: tc,
                tax_percentage: Some(rate_str),
                tax_base: None,
                tax_amount: None,
            });
            record_id += 1;
        }
    }

    // C 401 Furnizori = gross
    entries.push(GlEntry {
        id: new_id(),
        record_id,
        account_code: "401".to_string(),
        debit: Decimal::ZERO,
        credit: gross_ron,
        partner_cui: Some(issuer_cui.to_string()),
        customer_id: None,
        supplier_id: Some(supplier_canon),
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    });

    (journal, entries)
}

/// Postare plată client primită.
#[allow(clippy::too_many_arguments)]
fn post_payment(
    company_id: &str,
    payment_id: &str,
    invoice_id: &str,
    paid_at: &str,
    contact_id_raw: &str,
    partner_cui: Option<&str>,
    amount_ron: Decimal,
    method: &str,
    // TVA la încasare: per-rate VAT made exigible by THIS collection (rate, vat_ron). For a
    // cash-VAT invoice each entry posts the exigibility transfer D 4428 / C 4427; empty for
    // normal-VAT invoices (no second leg). Cumulative over the invoice's receipts this clears
    // 4428 to zero exactly (vat_released trues up the final receipt).
    released: &[(Decimal, Decimal)],
) -> (GlJournal, Vec<GlEntry>) {
    // Use canonical partner ID so it matches MasterFiles and SourceDocuments
    let contact_id = canonical_partner_id(contact_id_raw, partner_cui.unwrap_or(""));
    // Route the debit to the account matching the payment instrument: cash hits
    // 5311 "Casa" (CASA journal); everything else (bank transfer / card) hits 5121.
    let (debit_account, journal_id) = match method.to_ascii_lowercase().as_str() {
        "cash" | "numerar" => ("5311", "CASA"),
        _ => ("5121", "BANCA"),
    };
    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: journal_id.to_string(),
        journal_type: "PAYMENT".to_string(),
        transaction_id: payment_id.to_string(),
        transaction_date: paid_at.to_string(),
        description: Some(format!("Incasare factura {invoice_id}")),
        source_type: "PAYMENT".to_string(),
        source_id: payment_id.to_string(),
        customer_id: Some(contact_id.clone()),
        supplier_id: None,
    };

    let mut entries = vec![
        // D 5121 Bancă / 5311 Casa (per payment method) = amount
        GlEntry {
            id: new_id(),
            record_id: 1,
            account_code: debit_account.to_string(),
            debit: amount_ron,
            credit: Decimal::ZERO,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        },
        // C 4111 Clienți = amount
        GlEntry {
            id: new_id(),
            record_id: 2,
            account_code: "4111".to_string(),
            debit: Decimal::ZERO,
            credit: amount_ron,
            partner_cui: partner_cui.map(|s| s.to_string()),
            customer_id: Some(contact_id.clone()),
            supplier_id: None,
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        },
    ];

    // TVA la încasare — exigibility transfer for the VAT made exigible by this collection:
    // per rate, D 4428 "TVA neexigibilă" / C 4427 "TVA colectată". Now the VAT enters the decont.
    let mut record_id: i64 = 3;
    for (rate, vat) in released {
        if *vat == Decimal::ZERO {
            continue;
        }
        let tc = sales_tax_code_str("S", *rate);
        let rate_str = fmt_dec(*rate);
        // D 4428 — release out of TVA neexigibilă.
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "4428".to_string(),
            debit: *vat,
            credit: Decimal::ZERO,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "300".to_string(),
            tax_code: tc.clone(),
            tax_percentage: Some(rate_str.clone()),
            tax_base: None,
            tax_amount: None,
        });
        record_id += 1;
        // C 4427 — now exigible TVA colectată.
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "4427".to_string(),
            debit: Decimal::ZERO,
            credit: *vat,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "300".to_string(),
            tax_code: tc,
            tax_percentage: Some(rate_str),
            tax_base: None,
            tax_amount: Some(fmt_dec(*vat)),
        });
        record_id += 1;
    }

    (journal, entries)
}

/// RON `Decimal` → integer bani (round half-away-from-zero), matching declarations::ron_to_bani.
fn to_bani(d: Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::RoundingStrategy;
    (d * Decimal::from(100))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

/// Per-rate VAT `(rate, vat_ron)` made exigible by a single collection on a cash-VAT sales
/// invoice — the `D 4428 / C 4427` transfer for post_payment. Builds the invoice's standard
/// ("S") rate buckets + full gross in RON bani, the cumulative collected BEFORE this receipt
/// (strictly earlier by paid_at, then id), then `allocate_collection` (proportional, true-up
/// on the final receipt so 4428 clears to exactly zero). Empty if the invoice has no "S" lines.
async fn cash_vat_release_for_payment(
    pool: &SqlitePool,
    invoice_id: &str,
    payment_id: &str,
    paid_at: &str,
    currency: &str,
    fx: Option<Decimal>,
    amount_ron: Decimal,
) -> AppResult<Vec<(Decimal, Decimal)>> {
    use rust_decimal::prelude::ToPrimitive;
    use std::collections::BTreeMap;

    let line_rows = sqlx::query(
        "SELECT vat_category, vat_rate, subtotal_amount, vat_amount \
         FROM invoice_line_items WHERE invoice_id = ?1",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;

    let mut gross_bani: i64 = 0;
    let mut bucket_acc: BTreeMap<i64, (Decimal, i64, i64)> = BTreeMap::new();
    for l in &line_rows {
        let cat: String = l
            .try_get("vat_category")
            .unwrap_or_else(|_| "S".to_string());
        let rate_s: String = l.try_get("vat_rate").unwrap_or_default();
        let base_s: String = l.try_get("subtotal_amount").unwrap_or_default();
        let vat_s: String = l.try_get("vat_amount").unwrap_or_default();
        let base_ron = amount_to_ron(dec(&base_s), currency, fx);
        let vat_ron = amount_to_ron(dec(&vat_s), currency, fx);
        gross_bani += to_bani(base_ron) + to_bani(vat_ron);
        if cat.trim() == "S" {
            let rate = dec(&rate_s);
            let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
            let e = bucket_acc.entry(rate_key).or_insert((rate, 0, 0));
            e.1 += to_bani(base_ron);
            e.2 += to_bani(vat_ron);
        }
    }
    if bucket_acc.is_empty() || gross_bani <= 0 {
        return Ok(Vec::new());
    }

    let buckets: Vec<RateBucket> = bucket_acc
        .iter()
        .map(|(k, (_r, b, v))| RateBucket {
            rate_key: *k,
            base_bani: *b,
            vat_bani: *v,
        })
        .collect();
    let rate_of: BTreeMap<i64, Decimal> =
        bucket_acc.iter().map(|(k, (r, _, _))| (*k, *r)).collect();

    // Cumulative collected BEFORE this receipt (strictly earlier by paid_at, then id),
    // converted+rounded PER payment (skipping non-positive rows) so paid_before is byte-
    // identical to declarations::cash_vat_collected_groups — otherwise round2(Σ) vs Σ round2
    // would drift by a bani on FX invoices and GL 4427 would diverge from D300 collected.
    let prior_rows = sqlx::query(
        "SELECT amount FROM payments \
         WHERE invoice_id = ?1 \
           AND (substr(paid_at,1,10) < substr(?2,1,10) \
                OR (substr(paid_at,1,10) = substr(?2,1,10) AND id < ?3))",
    )
    .bind(invoice_id)
    .bind(paid_at)
    .bind(payment_id)
    .fetch_all(pool)
    .await?;
    let mut paid_before_bani: i64 = 0;
    for pr in &prior_rows {
        let a: String = pr.try_get("amount").unwrap_or_default();
        let b = to_bani(amount_to_ron(dec(&a), currency, fx));
        if b > 0 {
            paid_before_bani += b;
        }
    }
    let payment_bani = to_bani(amount_ron);

    let mut out: Vec<(Decimal, Decimal)> = Vec::new();
    for rb in allocate_collection(gross_bani, &buckets, paid_before_bani, payment_bani) {
        if rb.vat_bani == 0 {
            continue;
        }
        let rate = *rate_of.get(&rb.rate_key).unwrap_or(&Decimal::ZERO);
        out.push((rate, Decimal::from(rb.vat_bani) / Decimal::from(100)));
    }
    Ok(out)
}

// ─── DB insert helpers ────────────────────────────────────────────────────────

/// FIX 3: helpers accept a transaction executor so each document's
/// delete+insert pair is atomic.
async fn insert_journal(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    j: &GlJournal,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO gl_journal \
         (id, company_id, journal_id, journal_type, transaction_id, transaction_date, \
          description, source_type, source_id, customer_id, supplier_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
    )
    .bind(&j.id)
    .bind(&j.company_id)
    .bind(&j.journal_id)
    .bind(&j.journal_type)
    .bind(&j.transaction_id)
    .bind(&j.transaction_date)
    .bind(&j.description)
    .bind(&j.source_type)
    .bind(&j.source_id)
    .bind(&j.customer_id)
    .bind(&j.supplier_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_entry(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    journal_pk: &str,
    e: &GlEntry,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO gl_entry \
         (id, journal_pk, record_id, account_code, debit, credit, \
          partner_cui, customer_id, supplier_id, \
          tax_type, tax_code, tax_percentage, tax_base, tax_amount) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
    )
    .bind(&e.id)
    .bind(journal_pk)
    .bind(e.record_id)
    .bind(&e.account_code)
    .bind(fmt_dec(e.debit))
    .bind(fmt_dec(e.credit))
    .bind(&e.partner_cui)
    .bind(&e.customer_id)
    .bind(&e.supplier_id)
    .bind(&e.tax_type)
    .bind(&e.tax_code)
    .bind(&e.tax_percentage)
    .bind(&e.tax_base)
    .bind(&e.tax_amount)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ─── Balance guard ────────────────────────────────────────────────────────────

/// Verifică că Σdebit == Σcredit (în limita a 0.005 RON) înainte de inserare.
/// Returnează eroare dacă jurnalul este dezechilibrat.
fn assert_balanced(entries: &[GlEntry], source_id: &str) -> AppResult<()> {
    let total_d: Decimal = entries.iter().map(|e| e.debit).sum();
    let total_c: Decimal = entries.iter().map(|e| e.credit).sum();
    let diff = (total_d - total_c).abs();
    let tol = Decimal::new(5, 3); // 0.005 RON
    if diff > tol {
        return Err(crate::error::AppError::Other(format!(
            "GL dezechilibrat pentru {source_id}: Σdebit={total_d} Σcredit={total_c} diferenta={diff}"
        )));
    }
    Ok(())
}

// ─── Main posting function ────────────────────────────────────────────────────

/// Generează (sau re-generează) notele contabile GL pentru o perioadă.
///
/// **Idempotent**: orice jurnal existent cu același (company_id, source_type, source_id)
/// este șters și reinsertat (CASCADE pe gl_entry). Astfel rularea de două ori
/// produce exact același rezultat fără duplicate.
///
/// **Atomic**: fiecare document (factură / plată) este procesat într-o tranzacție
/// proprie (`pool.begin()` … `tx.commit()`) — un eșec parțial nu lasă date
/// incomplete.
///
/// Acoperă:
/// 1. Facturi emise (VALIDATED / STORNED) în perioadă.
/// 2. Facturi primite (cu defalcare TVA pe linii) în perioadă.
/// 3. Plăți client înregistrate în perioadă.
pub async fn generate_gl_entries(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<GlPostResult> {
    let mut journals_inserted: i64 = 0;
    let mut entries_inserted: i64 = 0;
    let mut journals_replaced: i64 = 0;

    // Cash-VAT (TVA la încasare) regime flags: drive 4428 routing + the collection release.
    let (company_cash_vat, cash_vat_start, cash_vat_end): (bool, Option<String>, Option<String>) = {
        let r = sqlx::query(
            "SELECT cash_vat, cash_vat_start, cash_vat_end FROM companies WHERE id = ?1 LIMIT 1",
        )
        .bind(company_id)
        .fetch_optional(pool)
        .await?;
        match r {
            Some(row) => (
                row.try_get::<bool, _>("cash_vat").unwrap_or(false),
                row.try_get::<Option<String>, _>("cash_vat_start")
                    .unwrap_or(None),
                row.try_get::<Option<String>, _>("cash_vat_end")
                    .unwrap_or(None),
            ),
            None => (false, None, None),
        }
    };
    // True when an invoice issued on `issue_date` is under the cash-VAT regime window.
    let in_cash_vat_window = |issue_date: &str| -> bool {
        company_cash_vat
            && cash_vat_start.as_deref().is_none_or(|s| issue_date >= s)
            && cash_vat_end.as_deref().is_none_or(|e| issue_date <= e)
    };

    // ── 1. Facturi emise ──────────────────────────────────────────────────────

    // FIX 1: Fetch invoice headers without aggregate — we query per-rate groups separately.
    let sales_rows = sqlx::query(
        "SELECT i.id, i.full_number, i.issue_date, i.contact_id, i.storno_of_invoice_id, \
                i.status, c.cui as contact_cui, \
                COALESCE(i.currency,'RON') as currency, i.exchange_rate \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    for row in &sales_rows {
        let inv_id: String = row.try_get("id").unwrap_or_default();
        let full_number: String = row.try_get("full_number").unwrap_or_default();
        let issue_date: String = row.try_get("issue_date").unwrap_or_default();
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let contact_cui: Option<String> = row.try_get("contact_cui").unwrap_or(None);
        let storno_ref: Option<String> = row.try_get("storno_of_invoice_id").unwrap_or(None);
        let status: String = row.try_get("status").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        // FIX 1: Fetch per-(vat_category, vat_rate) groups from invoice_line_items.
        // This mirrors the purchase per-rate approach and correctly handles mixed-rate invoices.
        let group_rows = sqlx::query(
            "SELECT vat_category, vat_rate, \
                    COALESCE(SUM(CAST(subtotal_amount AS REAL)),0.0) as net_sum, \
                    COALESCE(SUM(CAST(vat_amount AS REAL)),0.0) as vat_sum \
             FROM invoice_line_items \
             WHERE invoice_id = ?1 \
             GROUP BY vat_category, vat_rate",
        )
        .bind(&inv_id)
        .fetch_all(pool)
        .await?;

        let vat_groups: Vec<(Decimal, Decimal, String, Decimal)> = group_rows
            .iter()
            .map(|r| {
                let cat: String = r
                    .try_get("vat_category")
                    .unwrap_or_else(|_| "S".to_string());
                let rate_s: String = r.try_get("vat_rate").unwrap_or_else(|_| "19".to_string());
                let net_f: f64 = r.try_get("net_sum").unwrap_or(0.0);
                let vat_f: f64 = r.try_get("vat_sum").unwrap_or(0.0);
                let net = amount_to_ron(
                    Decimal::try_from(net_f).unwrap_or(Decimal::ZERO),
                    &currency,
                    fx,
                );
                let vat = amount_to_ron(
                    Decimal::try_from(vat_f).unwrap_or(Decimal::ZERO),
                    &currency,
                    fx,
                );
                (net, vat, cat, dec(&rate_s))
            })
            .collect();

        if vat_groups.is_empty() {
            continue; // invoice with no lines — skip
        }

        // Storno: dacă are referință la factură originală SAU status == STORNED
        let is_storno = storno_ref.is_some() || status == "STORNED";

        // Cash VAT: route standard ("S") output VAT to 4428 (neexigibilă) only for fresh
        // (non-storno) invoices issued within the regime window. Storno keeps today's 4427
        // behaviour (the proportional 4428/4427 reversal is deferred — see CASH_VAT_DESIGN.md).
        let cash_vat_applies = !is_storno && in_cash_vat_window(&issue_date);

        let (journal, entries) = post_sales_invoice(
            company_id,
            &inv_id,
            &full_number,
            &issue_date,
            &contact_id,
            contact_cui.as_deref(),
            &vat_groups,
            is_storno,
            cash_vat_applies,
        );

        // FIX 2: Balance guard — reject before writing.
        assert_balanced(&entries, &inv_id)?;

        // FIX 3: Atomic per-document transaction.
        let mut tx = pool.begin().await?;

        let deleted = sqlx::query(
            "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='INVOICE' AND source_id=?2",
        )
        .bind(company_id)
        .bind(&inv_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted > 0 {
            journals_replaced += 1;
        }

        let journal_id = journal.id.clone();
        insert_journal(&mut tx, &journal).await?;
        journals_inserted += 1;

        for entry in &entries {
            insert_entry(&mut tx, &journal_id, entry).await?;
            entries_inserted += 1;
        }

        tx.commit().await?;
    }

    // ── 2. Facturi primite ────────────────────────────────────────────────────

    // Fetch received invoices cu linii TVA (skip cele fără defalcare).
    let recv_rows = sqlx::query(
        "SELECT ri.id, ri.issuer_cui, ri.issuer_name, ri.issue_date, \
                COALESCE(ri.series,'') as series, COALESCE(ri.number,'') as number, \
                COALESCE(ri.currency,'RON') as currency, ri.exchange_rate \
         FROM received_invoices ri \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED' \
           AND ri.net_amount IS NOT NULL",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    // Count skipped (fără defalcare)
    let mut skipped_received: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM received_invoices \
         WHERE company_id=?1 AND issue_date>=?2 AND issue_date<=?3 \
           AND status != 'REJECTED' AND net_amount IS NULL",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    for row in &recv_rows {
        let recv_id: String = row.try_get("id").unwrap_or_default();
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let issue_date: String = row.try_get("issue_date").unwrap_or_default();
        let series: String = row.try_get("series").unwrap_or_default();
        let number: String = row.try_get("number").unwrap_or_default();
        let doc_number = if series.is_empty() && number.is_empty() {
            recv_id.clone()
        } else {
            format!("{series}{number}")
        };
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        // Fetch liniile TVA per cotă
        let vat_lines_rows = sqlx::query(
            "SELECT vat_category, vat_rate, base_amount, vat_amount \
             FROM received_invoice_vat_lines \
             WHERE received_invoice_id = ?1",
        )
        .bind(&recv_id)
        .fetch_all(pool)
        .await?;

        let vat_lines: Vec<(Decimal, Decimal, String, Decimal)> = vat_lines_rows
            .iter()
            .map(|r| {
                let cat: String = r
                    .try_get("vat_category")
                    .unwrap_or_else(|_| "S".to_string());
                let rate_s: String = r.try_get("vat_rate").unwrap_or_default();
                let base_s: String = r.try_get("base_amount").unwrap_or_default();
                let vat_s: String = r.try_get("vat_amount").unwrap_or_default();
                let rate = dec(&rate_s);
                let base_ron = amount_to_ron(dec(&base_s), &currency, fx);
                let vat_ron = amount_to_ron(dec(&vat_s), &currency, fx);
                (base_ron, vat_ron, cat, rate)
            })
            .collect();

        if vat_lines.is_empty() {
            skipped_received += 1;
            continue;
        }

        // FIX 2: Compute purchase gross as Σ(net + vat_owed_to_supplier).
        // For normal lines (non-AE/K), vat_owed = vat (charged by supplier).
        // For AE/K reverse-charge lines, vat_owed = 0 (VAT is self-assessed, not paid to supplier).
        // This ensures C401 = what is actually owed to the supplier and the journal balances.
        let gross_ron: Decimal = vat_lines
            .iter()
            .map(|(n, v, cat, _)| {
                let is_rc = cat == "AE" || cat == "K";
                if is_rc {
                    *n
                } else {
                    *n + *v
                }
            })
            .sum();

        let (journal, entries) = post_purchase_invoice(
            company_id,
            &recv_id,
            &doc_number,
            &issue_date,
            &issuer_cui,
            gross_ron,
            &vat_lines,
        );

        // FIX 2: Balance guard.
        assert_balanced(&entries, &recv_id)?;

        // FIX 3: Atomic per-document transaction.
        let mut tx = pool.begin().await?;

        let deleted = sqlx::query(
            "DELETE FROM gl_journal \
             WHERE company_id=?1 AND source_type='RECEIVED_INVOICE' AND source_id=?2",
        )
        .bind(company_id)
        .bind(&recv_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted > 0 {
            journals_replaced += 1;
        }

        let journal_id = journal.id.clone();
        insert_journal(&mut tx, &journal).await?;
        journals_inserted += 1;

        for entry in &entries {
            insert_entry(&mut tx, &journal_id, entry).await?;
            entries_inserted += 1;
        }

        tx.commit().await?;
    }

    // ── 3. Plăți clienți ─────────────────────────────────────────────────────

    let payment_rows = sqlx::query(
        "SELECT p.id, p.invoice_id, p.paid_at, p.amount, p.currency, p.method, \
                i.contact_id, c.cui as contact_cui, i.exchange_rate, \
                i.issue_date as inv_issue_date, i.status as inv_status, \
                i.storno_of_invoice_id as inv_storno_ref \
         FROM payments p \
         JOIN invoices i ON i.id = p.invoice_id \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE p.company_id = ?1 \
           AND p.paid_at >= ?2 \
           AND p.paid_at <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    for row in &payment_rows {
        let pay_id: String = row.try_get("id").unwrap_or_default();
        let inv_id: String = row.try_get("invoice_id").unwrap_or_default();
        let paid_at: String = row.try_get("paid_at").unwrap_or_default();
        let amount_s: String = row.try_get("amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let contact_cui: Option<String> = row.try_get("contact_cui").unwrap_or(None);
        let method: String = row
            .try_get("method")
            .unwrap_or_else(|_| "transfer".to_string());

        // Convert a foreign-currency payment to RON using the invoice's exchange
        // rate (the rate the receivable in 4111 was booked at). This makes the
        // bank/receivable legs balance in RON instead of posting the raw foreign
        // amount as if it were RON. The FX gain/loss leg (665/765) — the delta
        // between invoice-date and payment-date rates — remains deferred in v1
        // because the payment-date rate is not stored.
        let inv_fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let amount_ron = amount_to_ron(dec(&amount_s), &currency, inv_fx);

        // Cash VAT: if this payment settles a (non-storno) cash-VAT invoice issued in-window,
        // compute the per-rate VAT it makes exigible — post_payment adds D 4428 / C 4427.
        let inv_issue: String = row.try_get("inv_issue_date").unwrap_or_default();
        let inv_status: String = row.try_get("inv_status").unwrap_or_default();
        let inv_storno_ref: Option<String> = row.try_get("inv_storno_ref").unwrap_or(None);
        let inv_is_storno = inv_storno_ref.is_some() || inv_status == "STORNED";
        let released: Vec<(Decimal, Decimal)> = if !inv_is_storno && in_cash_vat_window(&inv_issue)
        {
            cash_vat_release_for_payment(
                pool, &inv_id, &pay_id, &paid_at, &currency, inv_fx, amount_ron,
            )
            .await?
        } else {
            Vec::new()
        };

        let (journal, entries) = post_payment(
            company_id,
            &pay_id,
            &inv_id,
            &paid_at,
            &contact_id,
            contact_cui.as_deref(),
            amount_ron,
            &method,
            &released,
        );

        // FIX 2: Balance guard.
        assert_balanced(&entries, &pay_id)?;

        // FIX 3: Atomic per-document transaction.
        let mut tx = pool.begin().await?;

        let deleted = sqlx::query(
            "DELETE FROM gl_journal \
             WHERE company_id=?1 AND source_type='PAYMENT' AND source_id=?2",
        )
        .bind(company_id)
        .bind(&pay_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted > 0 {
            journals_replaced += 1;
        }

        let journal_id = journal.id.clone();
        insert_journal(&mut tx, &journal).await?;
        journals_inserted += 1;

        for entry in &entries {
            insert_entry(&mut tx, &journal_id, entry).await?;
            entries_inserted += 1;
        }

        tx.commit().await?;
    }

    Ok(GlPostResult {
        journals_inserted,
        entries_inserted,
        journals_replaced,
        skipped_received,
    })
}

// ─── Reconciliation ──────────────────────────────────────────────────────────

/// Reconciliează GL cu D300 pentru o perioadă.
///
/// Invarianți verificați:
/// 1. Σdebit_total == Σcredit_total (principiul dublei înregistrări).
/// 2. Σcredit cont 4427 (TVA colectată GL) == TVA colectată D300.
/// 3. Σdebit cont 4426 (TVA deductibilă GL) == TVA deductibilă D300.
pub async fn reconcile(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<ReconcileReport> {
    // ── Σdebit, Σcredit ─────────────────────────────────────────────────────
    let totals_row = sqlx::query(
        "SELECT COALESCE(SUM(CAST(e.debit AS REAL)), 0.0) as total_debit, \
                COALESCE(SUM(CAST(e.credit AS REAL)), 0.0) as total_credit \
         FROM gl_entry e \
         JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 \
           AND j.transaction_date >= ?2 \
           AND j.transaction_date <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await?;

    let total_debit_f: f64 = totals_row.try_get("total_debit").unwrap_or(0.0);
    let total_credit_f: f64 = totals_row.try_get("total_credit").unwrap_or(0.0);
    let total_debit = Decimal::try_from(total_debit_f)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let total_credit = Decimal::try_from(total_credit_f)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);

    // ── Σcredit 4427 ────────────────────────────────────────────────────────
    let c4427_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(e.credit AS REAL)), 0.0) \
         FROM gl_entry e \
         JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 \
           AND j.transaction_date >= ?2 \
           AND j.transaction_date <= ?3 \
           AND e.account_code = '4427'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);
    let vat_collected_gl = Decimal::try_from(c4427_f)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);

    // ── Σdebit 4426 ─────────────────────────────────────────────────────────
    let d4426_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(e.debit AS REAL)), 0.0) \
         FROM gl_entry e \
         JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 \
           AND j.transaction_date >= ?2 \
           AND j.transaction_date <= ?3 \
           AND e.account_code = '4426'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);
    let vat_deductible_gl = Decimal::try_from(d4426_f)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);

    // ── D300 TVA colectată + deductibilă via shared core (FIX 4) ────────────
    let (d300_collected, d300_deductible) =
        crate::commands::declarations::d300_vat_totals(pool, company_id, period_from, period_to)
            .await?;

    // ── Discrepanțe ─────────────────────────────────────────────────────────
    let mut discrepancies: Vec<String> = Vec::new();

    let balanced = total_debit == total_credit;
    if !balanced {
        discrepancies.push(format!(
            "Dezechilibru GL: Σdebit={total_debit} ≠ Σcredit={total_credit} (diferenta {})",
            (total_debit - total_credit).abs()
        ));
    }

    let tol = Decimal::new(1, 2); // 0.01 RON toleranță rotunjire
    if (vat_collected_gl - d300_collected).abs() > tol {
        discrepancies.push(format!(
            "TVA colectata: GL 4427={vat_collected_gl} ≠ D300={d300_collected}"
        ));
    }
    if (vat_deductible_gl - d300_deductible).abs() > tol {
        discrepancies.push(format!(
            "TVA deductibila: GL 4426={vat_deductible_gl} ≠ D300={d300_deductible}"
        ));
    }

    Ok(ReconcileReport {
        balanced,
        total_debit: fmt_dec(total_debit),
        total_credit: fmt_dec(total_credit),
        vat_collected_gl: fmt_dec(vat_collected_gl),
        vat_collected_d300: fmt_dec(d300_collected),
        vat_deductible_gl: fmt_dec(vat_deductible_gl),
        vat_deductible_d300: fmt_dec(d300_deductible),
        discrepancies,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec as rdec;
    use sqlx::SqlitePool;

    // ── Helper: in-memory pool cu schema migrată ──────────────────────────────
    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    // ── Helper: inserează o companie minimă ───────────────────────────────────
    async fn insert_company(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .bind(id)
        .execute(pool)
        .await
        .expect("insert company");
    }

    // ── Helper: inserează un contact ──────────────────────────────────────────
    async fn insert_contact(pool: &SqlitePool, company_id: &str, contact_id: &str, cui: &str) {
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name) \
             VALUES (?1,?2,'CUSTOMER',?3,'Client Test')",
        )
        .bind(contact_id)
        .bind(company_id)
        .bind(cui)
        .execute(pool)
        .await
        .expect("insert contact");
    }

    // ── Helper: inserează factură cu linie ────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    async fn insert_invoice(
        pool: &SqlitePool,
        company_id: &str,
        inv_id: &str,
        contact_id: &str,
        status: &str,
        net: &str,
        vat: &str,
        gross: &str,
        storno_of: Option<&str>,
    ) {
        let issue_date = "2025-01-15";
        // Use inv_id as series + 1 as number so (company_id, series, number) is unique per id.
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, storno_of_invoice_id, created_at, updated_at) \
             VALUES (?1,?2,?3,?10,1,?10,?4,'2025-02-15','RON',?5,?6,?7,?8,'42',?9,1,1)",
        )
        .bind(inv_id)
        .bind(company_id)
        .bind(contact_id)
        .bind(issue_date)
        .bind(net)
        .bind(vat)
        .bind(gross)
        .bind(status)
        .bind(storno_of)
        .bind(inv_id) // ?10 = series = full_number = inv_id (unique)
        .execute(pool)
        .await
        .expect("insert invoice");

        sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, \
              vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES (?1,?2,'1','Produs','1','buc','1000','19','S',?3,?4,?5)",
        )
        .bind(format!("line-{inv_id}"))
        .bind(inv_id)
        .bind(net)
        .bind(vat)
        .bind(gross)
        .execute(pool)
        .await
        .expect("insert line item");
    }

    // ── Helper: inserează factură primită cu linii VAT ────────────────────────
    // `gross_override`: dacă None, gross = net + vat (cazul normal).
    //                   dacă Some(v), gross = v  (folosit pentru AE unde gross = net).
    async fn insert_received(
        pool: &SqlitePool,
        company_id: &str,
        rid: &str,
        category: &str,
        rate: &str,
        net: &str,
        vat: &str,
    ) {
        let gross = (dec(net) + dec(vat)).to_string();
        let dl_id = format!("dl-{rid}");
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, net_amount, vat_amount, currency, issue_date, \
              xml_path, status, downloaded_at, created_at) \
             VALUES (?1,?2,?6,'CUI123','Furnizor Test',?3,?4,?5,'RON','2025-01-15', \
                     'x.xml','NEW',1,1)",
        )
        .bind(rid)
        .bind(company_id)
        .bind(&gross)
        .bind(net)
        .bind(vat)
        .bind(&dl_id)
        .execute(pool)
        .await
        .expect("insert received");

        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(format!("vl-{rid}"))
        .bind(rid)
        .bind(rate)
        .bind(category)
        .bind(net)
        .bind(vat)
        .execute(pool)
        .await
        .expect("insert vat line");
    }

    // ── Helper: inserează factură cu DOUĂ linii la rate diferite ─────────────
    /// Inserează factură cu linii:
    ///   linia 1: net1@rate1_category1
    ///   linia 2: net2@rate2_category2
    /// gross pentru factură = net1+vat1+net2+vat2 (poate diferi cu 0.01 pentru testul de rounding).
    #[allow(clippy::too_many_arguments)]
    async fn insert_invoice_multiline(
        pool: &SqlitePool,
        company_id: &str,
        inv_id: &str,
        contact_id: &str,
        net1: &str,
        vat1: &str,
        rate1: &str,
        cat1: &str,
        net2: &str,
        vat2: &str,
        rate2: &str,
        cat2: &str,
        stored_gross_override: Option<&str>, // None → sum of lines
    ) {
        let issue_date = "2025-01-15";
        let computed_gross = dec(net1) + dec(vat1) + dec(net2) + dec(vat2);
        let gross = stored_gross_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| computed_gross.to_string());
        let total_net = dec(net1) + dec(net2);
        let total_vat = dec(vat1) + dec(vat2);

        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, storno_of_invoice_id, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,1,?4,?5,'2025-02-15','RON',?6,?7,?8,'VALIDATED','42',NULL,1,1)",
        )
        .bind(inv_id) // ?1 id
        .bind(company_id) // ?2
        .bind(contact_id) // ?3
        .bind(inv_id) // ?4 series = full_number = inv_id (unique per inv_id)
        .bind(issue_date) // ?5
        .bind(total_net.to_string()) // ?6
        .bind(total_vat.to_string()) // ?7
        .bind(gross) // ?8
        .execute(pool)
        .await
        .expect("insert multiline invoice");

        // Line 1
        sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, \
              vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES (?1,?2,'1','Produs 1','1','buc',?3,?4,?5,?6,?7,?8)",
        )
        .bind(format!("line1-{inv_id}"))
        .bind(inv_id)
        .bind(net1) // unit_price = net1
        .bind(rate1)
        .bind(cat1)
        .bind(net1)
        .bind(vat1)
        .bind((dec(net1) + dec(vat1)).to_string())
        .execute(pool)
        .await
        .expect("insert line1");

        // Line 2
        sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, \
              vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES (?1,?2,'2','Produs 2','1','buc',?3,?4,?5,?6,?7,?8)",
        )
        .bind(format!("line2-{inv_id}"))
        .bind(inv_id)
        .bind(net2)
        .bind(rate2)
        .bind(cat2)
        .bind(net2)
        .bind(vat2)
        .bind((dec(net2) + dec(vat2)).to_string())
        .execute(pool)
        .await
        .expect("insert line2");
    }

    // ── Helper: suma debit/credit per cont dintr-un jurnal ────────────────────
    async fn sum_entries(pool: &SqlitePool, journal_pk: &str) -> (Decimal, Decimal) {
        let rows = sqlx::query("SELECT debit, credit FROM gl_entry WHERE journal_pk = ?1")
            .bind(journal_pk)
            .fetch_all(pool)
            .await
            .unwrap();
        let mut debit = Decimal::ZERO;
        let mut credit = Decimal::ZERO;
        for r in &rows {
            debit += dec(&r.try_get::<String, _>("debit").unwrap_or_default());
            credit += dec(&r.try_get::<String, _>("credit").unwrap_or_default());
        }
        (debit, credit)
    }

    async fn get_journal_pk(pool: &SqlitePool, source_id: &str) -> String {
        sqlx::query_scalar("SELECT id FROM gl_journal WHERE source_id = ?1")
            .bind(source_id)
            .fetch_one(pool)
            .await
            .expect("journal not found")
    }

    async fn get_entry_amount(
        pool: &SqlitePool,
        journal_pk: &str,
        account: &str,
        col: &str,
    ) -> Decimal {
        let sql = format!(
            "SELECT COALESCE({col},'0') FROM gl_entry \
             WHERE journal_pk=?1 AND account_code=?2"
        );
        let s: String = sqlx::query_scalar(&sql)
            .bind(journal_pk)
            .bind(account)
            .fetch_optional(pool)
            .await
            .unwrap()
            .unwrap_or_else(|| "0".to_string());
        dec(&s)
    }

    // ── Test 1: Factură emisă (net=1000, VAT=190, gross=1190) ────────────────
    #[tokio::test]
    async fn test1_sales_invoice_posting() {
        let pool = setup_pool().await;
        let cid = "co1";
        let iid = "inv1";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct1", "CUI999").await;
        insert_invoice(
            &pool,
            cid,
            iid,
            "ct1",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;

        let result = generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate_gl_entries");
        assert_eq!(result.journals_inserted, 1);

        let jpk = get_journal_pk(&pool, iid).await;
        let d4111 = get_entry_amount(&pool, &jpk, "4111", "debit").await;
        let c707 = get_entry_amount(&pool, &jpk, "707", "credit").await;
        let c4427 = get_entry_amount(&pool, &jpk, "4427", "credit").await;

        assert_eq!(d4111, rdec!(1190), "D 4111 = gross 1190");
        assert_eq!(c707, rdec!(1000), "C 707 = net 1000");
        assert_eq!(c4427, rdec!(190), "C 4427 = VAT 190");

        // Σdebit == Σcredit
        let (total_d, total_c) = sum_entries(&pool, &jpk).await;
        assert_eq!(total_d, total_c, "Factura emisă: dezechilibru GL");
    }

    // ── Test 2: Factură primită ────────────────────────────────────────────────
    #[tokio::test]
    async fn test2_purchase_invoice_posting() {
        let pool = setup_pool().await;
        let cid = "co2";
        insert_company(&pool, cid).await;
        insert_received(&pool, cid, "ri1", "S", "19", "1000", "190").await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate_gl_entries");

        let jpk = get_journal_pk(&pool, "ri1").await;
        let d607 = get_entry_amount(&pool, &jpk, "607", "debit").await;
        let d4426 = get_entry_amount(&pool, &jpk, "4426", "debit").await;
        let c401 = get_entry_amount(&pool, &jpk, "401", "credit").await;

        assert_eq!(d607, rdec!(1000), "D 607 = net 1000");
        assert_eq!(d4426, rdec!(190), "D 4426 = VAT 190");
        assert_eq!(c401, rdec!(1190), "C 401 = gross 1190");

        let (total_d, total_c) = sum_entries(&pool, &jpk).await;
        assert_eq!(total_d, total_c, "Factura primita: dezechilibru GL");
    }

    // ── Test 3: Plată client ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test3_payment_posting() {
        let pool = setup_pool().await;
        let cid = "co3";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct3", "CUI3").await;
        insert_invoice(
            &pool,
            cid,
            "inv3",
            "ct3",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;

        // Inserează plată
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method) \
             VALUES ('pay3','inv3',?1,'500','RON','2025-01-20','transfer')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .expect("insert payment");

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let jpk = get_journal_pk(&pool, "pay3").await;
        let d5121 = get_entry_amount(&pool, &jpk, "5121", "debit").await;
        let c4111 = get_entry_amount(&pool, &jpk, "4111", "credit").await;

        assert_eq!(d5121, rdec!(500), "D 5121 = 500");
        assert_eq!(c4111, rdec!(500), "C 4111 = 500");

        let (td, tc) = sum_entries(&pool, &jpk).await;
        assert_eq!(td, tc, "Plata: dezechilibru GL");
    }

    // ── Test 3b: Plată în valută — conversie la cursul facturii ───────────────
    /// O plată în EUR trebuie convertită în RON la cursul facturii (nu postată ca
    /// sumă brută în EUR, ca și cum ar fi RON). 119 EUR × 5.0 = 595 RON.
    #[tokio::test]
    async fn test3b_fx_payment_converts_at_invoice_rate() {
        let pool = setup_pool().await;
        let cid = "co3b";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct3b", "CUI3B").await;
        // Factură EUR, curs 5.0 (1 EUR = 5 RON): net=100, VAT=19, gross=119 (EUR).
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, \
              total_amount, status, payment_means_code, storno_of_invoice_id, created_at, updated_at) \
             VALUES ('inv3b',?1,'ct3b','F3B',1,'F3B','2025-01-15','2025-02-15','EUR',5.0,\
                     '100','19','119','VALIDATED','42',NULL,1,1)",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .expect("insert EUR invoice");
        // Plată de 119 EUR.
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method) \
             VALUES ('pay3b','inv3b',?1,'119','EUR','2025-01-20','transfer')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .expect("insert EUR payment");

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let jpk = get_journal_pk(&pool, "pay3b").await;
        let d5121 = get_entry_amount(&pool, &jpk, "5121", "debit").await;
        let c4111 = get_entry_amount(&pool, &jpk, "4111", "credit").await;
        // 119 EUR × 5.0 = 595 RON (nu 119 ca și cum ar fi RON).
        assert_eq!(d5121, rdec!(595), "D 5121 = 119 EUR × 5.0 = 595 RON");
        assert_eq!(c4111, rdec!(595), "C 4111 = 595 RON");

        let (td, tc) = sum_entries(&pool, &jpk).await;
        assert_eq!(td, tc, "Plata FX: dezechilibru GL");
    }

    // ── Test 4: Taxare inversă (reverse charge AE) ────────────────────────────
    #[tokio::test]
    async fn test4_reverse_charge() {
        let pool = setup_pool().await;
        let cid = "co4";
        insert_company(&pool, cid).await;
        // AE: net=1000, VAT=0 on supplier invoice (gross=net=1000); self-assess 19% = 190.
        // total_amount = 1000 (supplier does not charge VAT for AE).
        // The vat_line records net=1000, vat=190 so we can self-assess.
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, net_amount, vat_amount, currency, issue_date, \
              xml_path, status, downloaded_at, created_at) \
             VALUES ('ri4',?1,'dl-ri4','CUISUP4','Furnizor AE',\
                     '1000','1000','0','RON','2025-01-15','x.xml','NEW',1,1)",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .expect("insert AE received");
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES ('vl-ri4','ri4','19','AE','1000','190')",
        )
        .execute(&pool)
        .await
        .expect("insert AE vat line");

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let jpk = get_journal_pk(&pool, "ri4").await;
        let d4426 = get_entry_amount(&pool, &jpk, "4426", "debit").await;
        let c4427 = get_entry_amount(&pool, &jpk, "4427", "credit").await;

        // D 4426 = C 4427 pentru auto-assessment (efect net TVA = 0)
        assert_eq!(
            d4426, c4427,
            "Reverse charge: 4426 debit trebuie să egaleze 4427 credit"
        );
        assert_eq!(c4427, rdec!(190), "Reverse charge: 4427 credit = 190");

        let (td, tc) = sum_entries(&pool, &jpk).await;
        assert_eq!(td, tc, "Reverse charge: dezechilibru GL");
    }

    // ── Test 5: Storno factură emisă ─────────────────────────────────────────
    #[tokio::test]
    async fn test5_storno_negative_amounts() {
        let pool = setup_pool().await;
        let cid = "co5";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct5", "CUI5").await;
        // Factură originală
        insert_invoice(
            &pool,
            cid,
            "inv5",
            "ct5",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;
        // Notă de credit / storno cu referință la original
        insert_invoice(
            &pool,
            cid,
            "inv5s",
            "ct5",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            Some("inv5"),
        )
        .await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let jpk_storno = get_journal_pk(&pool, "inv5s").await;

        // Storno: 4111 trebuie să fie pe CREDIT (nu debit) — suma negativă => credit
        let d4111 = get_entry_amount(&pool, &jpk_storno, "4111", "debit").await;
        let c4111 = get_entry_amount(&pool, &jpk_storno, "4111", "credit").await;
        let d707 = get_entry_amount(&pool, &jpk_storno, "707", "debit").await;
        let c707 = get_entry_amount(&pool, &jpk_storno, "707", "credit").await;

        // La storno sumele sunt negate: gross negativ => credit 4111; net negativ => debit 707
        assert_eq!(d4111, Decimal::ZERO, "Storno: 4111 nu trebuie debit");
        assert_eq!(c4111, rdec!(1190), "Storno: C 4111 = 1190 (stornare)");
        assert_eq!(c707, Decimal::ZERO, "Storno: 707 nu trebuie credit");
        assert_eq!(d707, rdec!(1000), "Storno: D 707 = 1000 (stornare)");

        let (td, tc) = sum_entries(&pool, &jpk_storno).await;
        assert_eq!(td, tc, "Storno: dezechilibru GL");
    }

    // ── Test 6: Idempotență ───────────────────────────────────────────────────
    #[tokio::test]
    async fn test6_idempotent_generate() {
        let pool = setup_pool().await;
        let cid = "co6";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct6", "CUI6").await;
        insert_invoice(
            &pool,
            cid,
            "inv6",
            "ct6",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;

        // Rulăm de două ori
        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("first run");
        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("second run");

        // Numărul de jurnale trebuie să rămână 1 (nu se duplică)
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM gl_journal WHERE company_id = ?1")
                .bind(cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            count, 1,
            "Idempotenta: trebuie exact 1 jurnal, nu duplicate"
        );

        // Numărul de intrări trebuie de asemenea să rămână stabil
        let entry_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = ?1",
        )
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            entry_count, 3,
            "Idempotenta: trebuie exact 3 intrari GL (4111+707+4427)"
        );
    }

    // ── Test 7: Reconciliere completă ─────────────────────────────────────────
    #[tokio::test]
    async fn test7_reconcile_ties_out() {
        let pool = setup_pool().await;
        let cid = "co7";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct7", "CUI7").await;
        // Factură emisă: net=1000, VAT=190
        insert_invoice(
            &pool,
            cid,
            "inv7",
            "ct7",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;
        // Factură primită: net=500, VAT=95
        insert_received(&pool, cid, "ri7", "S", "19", "500", "95").await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let report = reconcile(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("reconcile");

        // Principiul dublei înregistrări
        assert!(
            report.balanced,
            "GL dezechilibrat: debit={} credit={} discrepante={:?}",
            report.total_debit, report.total_credit, report.discrepancies
        );

        // TVA colectată GL 4427 == D300 collected
        assert_eq!(
            report.vat_collected_gl,
            report.vat_collected_d300,
            "TVA colectata GL != D300: {}",
            report.discrepancies.join("; ")
        );
        assert_eq!(report.vat_collected_gl, "190.00");

        // TVA deductibilă GL 4426 == D300 deductible
        assert_eq!(
            report.vat_deductible_gl,
            report.vat_deductible_d300,
            "TVA deductibila GL != D300: {}",
            report.discrepancies.join("; ")
        );
        assert_eq!(report.vat_deductible_gl, "95.00");

        assert!(
            report.discrepancies.is_empty(),
            "Discrepante: {:?}",
            report.discrepancies
        );
    }

    // ── Test 7b: Reconciliere cu taxare inversă (AE) — fără discrepanțe ───────
    /// Achiziție cu taxare inversă: GL înregistrează D 4426 = C 4427 (autolichidare).
    /// `d300_vat_totals` trebuie să includă TVA-ul autolichidat și pe latura
    /// COLECTATĂ, altfel reconcilierea raporta o discrepanță falsă pentru orice
    /// cumpărător art.331 / intracomunitar (GL 4427 ≠ D300 colectată).
    #[tokio::test]
    async fn test7b_reconcile_reverse_charge_ties_out() {
        let pool = setup_pool().await;
        let cid = "co7b";
        insert_company(&pool, cid).await;
        // Doar o achiziție cu taxare inversă (AE), net=1000, VAT autolichidat=190.
        insert_received(&pool, cid, "ri7b", "AE", "19", "1000", "190").await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        let report = reconcile(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("reconcile");

        assert!(
            report.balanced,
            "GL dezechilibrat: debit={} credit={}",
            report.total_debit, report.total_credit
        );
        // TVA autolichidat apare pe AMBELE laturi: GL 4427 == D300 colectată == 190.
        assert_eq!(report.vat_collected_gl, "190.00");
        assert_eq!(report.vat_collected_gl, report.vat_collected_d300);
        assert_eq!(report.vat_deductible_gl, "190.00");
        assert_eq!(report.vat_deductible_gl, report.vat_deductible_d300);
        assert!(
            report.discrepancies.is_empty(),
            "Reverse-charge ar trebui să reconcilieze fără discrepanțe: {:?}",
            report.discrepancies
        );
    }

    // ── Test 8: Factură cu rate mixte (19% + 9%) ────────────────────────────
    /// FIX 1 + FIX 2: factură cu două linii la cote diferite trebuie să producă
    /// DOUĂ linii 707 + DOUĂ linii 4427 cu tax_code/tax_percentage per cotă;
    /// D4111 = net1+vat1+net2+vat2 = 1000+190+500+45 = 1735.
    #[tokio::test]
    async fn test8_mixed_rate_sales_invoice() {
        let pool = setup_pool().await;
        let cid = "co8";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct8", "CUI8").await;

        // Line 1: 1000 net @ 19% S → VAT 190
        // Line 2: 500 net @ 9% S → VAT 45
        insert_invoice_multiline(
            &pool, cid, "inv8", "ct8", "1000", "190", "19", "S", "500", "45", "9", "S", None,
        )
        .await;

        let result = generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate_gl_entries");
        assert_eq!(result.journals_inserted, 1);

        let jpk = get_journal_pk(&pool, "inv8").await;

        // D 4111 = gross = net1+vat1+net2+vat2 = 1000+190+500+45 = 1735
        let d4111 = get_entry_amount(&pool, &jpk, "4111", "debit").await;
        assert_eq!(
            d4111,
            rdec!(1735),
            "D 4111 should be 1735 (sum of all legs)"
        );

        // Σcredit 707 = 1000 + 500 = 1500
        let c707_total: Decimal = {
            let rows = sqlx::query(
                "SELECT credit FROM gl_entry WHERE journal_pk=?1 AND account_code='707'",
            )
            .bind(&jpk)
            .fetch_all(&pool)
            .await
            .unwrap();
            rows.iter()
                .map(|r| dec(&r.try_get::<String, _>("credit").unwrap_or_default()))
                .sum()
        };
        assert_eq!(c707_total, rdec!(1500), "Σ C 707 should be 1500");

        // Σcredit 4427 = 190 + 45 = 235
        let c4427_total: Decimal = {
            let rows = sqlx::query(
                "SELECT credit FROM gl_entry WHERE journal_pk=?1 AND account_code='4427'",
            )
            .bind(&jpk)
            .fetch_all(&pool)
            .await
            .unwrap();
            rows.iter()
                .map(|r| dec(&r.try_get::<String, _>("credit").unwrap_or_default()))
                .sum()
        };
        assert_eq!(c4427_total, rdec!(235), "Σ C 4427 should be 235");

        // Must have exactly 2 rows for 707 and 2 for 4427
        let count_707: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_entry WHERE journal_pk=?1 AND account_code='707'",
        )
        .bind(&jpk)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            count_707, 2,
            "Must have 2 separate 707 lines (one per VAT rate)"
        );

        let count_4427: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_entry WHERE journal_pk=?1 AND account_code='4427'",
        )
        .bind(&jpk)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            count_4427, 2,
            "Must have 2 separate 4427 lines (one per VAT rate)"
        );

        // Verify per-rate tax_percentage on 707 lines
        let rates: Vec<String> = {
            let rows = sqlx::query(
                "SELECT tax_percentage FROM gl_entry \
                 WHERE journal_pk=?1 AND account_code='707' ORDER BY tax_percentage",
            )
            .bind(&jpk)
            .fetch_all(&pool)
            .await
            .unwrap();
            rows.iter()
                .map(|r| {
                    r.try_get::<Option<String>, _>("tax_percentage")
                        .unwrap_or(None)
                        .unwrap_or_default()
                })
                .collect()
        };
        assert!(rates.contains(&"9.00".to_string()), "Must have 9% 707 line");
        assert!(
            rates.contains(&"19.00".to_string()),
            "Must have 19% 707 line"
        );

        // Σdebit == Σcredit
        let (total_d, total_c) = sum_entries(&pool, &jpk).await;
        assert_eq!(total_d, total_c, "Mixed-rate invoice: dezechilibru GL");
    }

    // ── Test 9: Rounding skew — stored total ≠ net+vat ─────────────────────
    /// FIX 2: when total_amount in the invoice differs from net+vat by 0.01
    /// (rounding), the GL must still balance — gross = Σnet + Σvat, not stored total.
    #[tokio::test]
    async fn test9_rounding_skew_still_balances() {
        let pool = setup_pool().await;
        let cid = "co9";
        insert_company(&pool, cid).await;
        insert_contact(&pool, cid, "ct9", "CUI9").await;

        // net=1000, vat=190, but stored gross = 1190.01 (0.01 skew)
        // The GL gross must be 1190.00 (from net+vat), not 1190.01.
        insert_invoice_multiline(
            &pool,
            cid,
            "inv9",
            "ct9",
            "1000",
            "190",
            "19",
            "S",
            "0",
            "0",
            "19",
            "S",
            Some("1190.01"), // deliberately wrong stored total
        )
        .await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("should succeed — rounding skew is corrected");

        let jpk = get_journal_pk(&pool, "inv9").await;

        // GL gross must be computed from lines, not from stored total
        let d4111 = get_entry_amount(&pool, &jpk, "4111", "debit").await;
        assert_eq!(
            d4111,
            rdec!(1190),
            "Gross must be net+vat=1190, not stored 1190.01"
        );

        // Σdebit == Σcredit (journal balanced)
        let (total_d, total_c) = sum_entries(&pool, &jpk).await;
        assert_eq!(total_d, total_c, "Rounding-skew invoice: dezechilibru GL");
    }

    // ── Test 10: Balance guard rejects unbalanced synthetic input ───────────
    /// FIX 2: The assert_balanced helper must catch a deliberately skewed entry set.
    #[test]
    fn test10_balance_guard_rejects_unbalanced() {
        // Fabricate an unbalanced entry set: D4111=1190, C707=1000 (missing C4427=190)
        let entries = vec![
            GlEntry {
                id: "e1".to_string(),
                record_id: 1,
                account_code: "4111".to_string(),
                debit: rdec!(1190),
                credit: Decimal::ZERO,
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "000".to_string(),
                tax_code: "000000".to_string(),
                tax_percentage: None,
                tax_base: None,
                tax_amount: None,
            },
            GlEntry {
                id: "e2".to_string(),
                record_id: 2,
                account_code: "707".to_string(),
                debit: Decimal::ZERO,
                credit: rdec!(1000),
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "300".to_string(),
                tax_code: "VAT_S_19".to_string(),
                tax_percentage: Some("19.00".to_string()),
                tax_base: Some("1000.00".to_string()),
                tax_amount: Some("190.00".to_string()),
            },
        ];

        let result = assert_balanced(&entries, "test-unbalanced");
        assert!(
            result.is_err(),
            "Balance guard must reject: D=1190, C=1000 (diff=190)"
        );

        // A balanced set must pass
        let balanced = vec![
            GlEntry {
                id: "b1".to_string(),
                record_id: 1,
                account_code: "4111".to_string(),
                debit: rdec!(1190),
                credit: Decimal::ZERO,
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "000".to_string(),
                tax_code: "000000".to_string(),
                tax_percentage: None,
                tax_base: None,
                tax_amount: None,
            },
            GlEntry {
                id: "b2".to_string(),
                record_id: 2,
                account_code: "707".to_string(),
                debit: Decimal::ZERO,
                credit: rdec!(1000),
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "300".to_string(),
                tax_code: "VAT_S_19".to_string(),
                tax_percentage: Some("19.00".to_string()),
                tax_base: Some("1000.00".to_string()),
                tax_amount: Some("190.00".to_string()),
            },
            GlEntry {
                id: "b3".to_string(),
                record_id: 3,
                account_code: "4427".to_string(),
                debit: Decimal::ZERO,
                credit: rdec!(190),
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "300".to_string(),
                tax_code: "VAT_S_19".to_string(),
                tax_percentage: Some("19.00".to_string()),
                tax_base: None,
                tax_amount: None,
            },
        ];

        assert!(
            assert_balanced(&balanced, "test-balanced").is_ok(),
            "Balance guard must accept a balanced set"
        );
    }

    // ── Cash VAT (TVA la încasare) — 4428 postings + collection release ───────
    async fn enable_cash_vat(pool: &SqlitePool, company: &str, start: &str) {
        sqlx::query("UPDATE companies SET cash_vat=1, cash_vat_start=?2 WHERE id=?1")
            .bind(company)
            .bind(start)
            .execute(pool)
            .await
            .expect("enable cash vat");
    }

    async fn insert_pay(
        pool: &SqlitePool,
        company: &str,
        inv: &str,
        pid: &str,
        amount: &str,
        paid_at: &str,
    ) {
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method) \
             VALUES (?1,?2,?3,?4,'RON',?5,'transfer')",
        )
        .bind(pid)
        .bind(inv)
        .bind(company)
        .bind(amount)
        .bind(paid_at)
        .execute(pool)
        .await
        .expect("insert payment");
    }

    /// (Σdebit, Σcredit) for an account across ALL of a company's journals.
    async fn account_balance(
        pool: &SqlitePool,
        company: &str,
        account: &str,
    ) -> (Decimal, Decimal) {
        let rows = sqlx::query(
            "SELECT e.debit, e.credit FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = ?1 AND e.account_code = ?2",
        )
        .bind(company)
        .bind(account)
        .fetch_all(pool)
        .await
        .unwrap();
        let mut d = Decimal::ZERO;
        let mut c = Decimal::ZERO;
        for r in &rows {
            d += dec(&r.try_get::<String, _>("debit").unwrap_or_default());
            c += dec(&r.try_get::<String, _>("credit").unwrap_or_default());
        }
        (d, c)
    }

    #[tokio::test]
    async fn cash_vat_sale_credits_4428_not_4427() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        enable_cash_vat(&pool, "co", "2025-01-01").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        // insert_invoice uses issue_date 2025-01-15, rate 19, category S.
        insert_invoice(
            &pool,
            "co",
            "inv",
            "ct",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        // Output VAT is neexigibilă at invoice date: in 4428, NOT 4427.
        let (_, c4428) = account_balance(&pool, "co", "4428").await;
        let (_, c4427) = account_balance(&pool, "co", "4427").await;
        assert_eq!(c4428, dec("190"), "VAT must be credited to 4428");
        assert_eq!(c4427, Decimal::ZERO, "nothing exigible yet (no collection)");
    }

    #[tokio::test]
    async fn cash_vat_full_collection_transfers_4428_to_4427() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        enable_cash_vat(&pool, "co", "2025-01-01").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        insert_invoice(
            &pool,
            "co",
            "inv",
            "ct",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;
        insert_pay(&pool, "co", "inv", "p1", "1190", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        let (_, c4427) = account_balance(&pool, "co", "4427").await;
        assert_eq!(
            c4428 - d4428,
            Decimal::ZERO,
            "4428 fully cleared on collection"
        );
        assert_eq!(c4427, dec("190"), "full VAT now exigible in 4427");
    }

    #[tokio::test]
    async fn cash_vat_partial_collection_is_proportional() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        enable_cash_vat(&pool, "co", "2025-01-01").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        insert_invoice(
            &pool,
            "co",
            "inv",
            "ct",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;
        // Collect half (595 of 1190) → release 595 × 19/119 = 95.
        insert_pay(&pool, "co", "inv", "p1", "595", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        let (_, c4427) = account_balance(&pool, "co", "4427").await;
        assert_eq!(c4427, dec("95"), "half the VAT exigible");
        assert_eq!(c4428 - d4428, dec("95"), "the other half stays neexigibilă");
    }

    #[tokio::test]
    async fn non_cash_vat_sale_still_credits_4427() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await; // cash_vat stays 0
        insert_contact(&pool, "co", "ct", "CUI999").await;
        insert_invoice(
            &pool,
            "co",
            "inv",
            "ct",
            "VALIDATED",
            "1000",
            "190",
            "1190",
            None,
        )
        .await;
        insert_pay(&pool, "co", "inv", "p1", "1190", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (_, c4427) = account_balance(&pool, "co", "4427").await;
        let (_, c4428) = account_balance(&pool, "co", "4428").await;
        assert_eq!(c4427, dec("190"), "normal VAT: 4427 at invoice date");
        assert_eq!(c4428, Decimal::ZERO, "no 4428 for a non-cash-VAT company");
    }
}
