//! GL auto-posting engine — Registru jurnal cu dublu-intrare per OMFP 1802/2014.
//!
//! ## Șabloane de înregistrare (standard RO)
//!
//! **Factură emisă** (VALIDATED / STORNED):
//!   D 4111 (Clienți)              = gross
//!   C 70x (Venituri)              = net  — 701/704/707 după revenue_kind, 709 pt reduceri
//!   C 4427 (TVA colectată)        = VAT  (sau 4428 neexigibilă sub TVA la încasare)
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
//! **Plată valutară — diferențe de curs (665/765)**: creanța/datoria se stinge la cursul
//!   FACTURII, iar trezoreria (5124/5314) la cursul PLĂȚII; diferența → 665 (cheltuială) /
//!   765 (venit). Vezi post_payment / post_received_payment.
//!
//! ## Simplificări / amânări explicite (v1)
//!   - Venit pe 70x după revenue_kind (701/704/707/709); implicit goods→707.
//!   - Cont cheltuieli fix 607 (nu distingem 371 stocuri vs 607 — lipsă câmp tip achiziție).
//!   - Reevaluarea lunară a soldurilor în valută (pct. 325 OMFP 1802/2014): neimplementată
//!     încă (necesită cursul BNR de închidere + valoarea contabilă reevaluată per document).
//!   - Reduceri comerciale primite (609) latura achiziție: amânat (facturile primite nu au linii).
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
    // Canonicalise zero so `-Decimal::ZERO` (from `(-net).max(0)` on a settled account) never
    // renders as "-0.00" on a statutory register / balance column.
    let d = if d.is_zero() { Decimal::ZERO } else { d };
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

/// Sales-revenue account for a line by its kind (OMFP 1802/2014, funcțiunea clasei 7):
/// product → 701 (produse finite), service → 704 (servicii), reduction → 709 (reduceri
/// comerciale acordate), goods (default) → 707 (mărfuri).
fn revenue_account(revenue_kind: &str) -> &'static str {
    match revenue_kind.trim() {
        "product" => "701",
        "service" => "704",
        "reduction" => "709",
        _ => "707",
    }
}

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
    vat_groups: &[(Decimal, Decimal, String, Decimal, String)], // (net, vat, category, rate, revenue_kind)
    is_storno: bool,
    // TVA la încasare: when true, the standard-rate ("S") output VAT is not yet exigible at
    // invoice date — credit 4428 "TVA neexigibilă" instead of 4427; it transfers to 4427 on
    // collection (see post_payment). Excluded categories (AE/E/Z/K) keep 4427.
    cash_vat_applies: bool,
) -> (GlJournal, Vec<GlEntry>) {
    // Use canonical partner ID (CUI-based) so it matches MasterFiles and SourceDocuments
    let contact_id = canonical_partner_id(contact_id_raw, partner_cui.unwrap_or(""));
    // Sign is ALWAYS +1: the stored line amounts are already correctly signed (a normal sale is
    // positive; a storno credit note is stored with NEGATIVE lines). This matches D300/D394, which
    // sum the stored signed amounts WITHOUT negation. A STORNED original keeps its positive sale
    // (it happened) and is reversed by the credit note's negative lines — never flipped in place.
    let sign = Decimal::ONE;

    // FIX 2: Compute gross as Σnet + Σvat so the GL always balances exactly,
    // independent of any rounding discrepancy in the stored total_amount.
    let gross_raw: Decimal = vat_groups.iter().map(|(n, v, _, _, _)| *n + *v).sum();
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

    // Per-(category, rate, revenue_kind) group: C 70x revenue + C 4427/4428 VAT.
    for (net_ron, vat_ron, category, rate, revenue_kind) in vat_groups {
        let net = *net_ron * sign;
        let vat = *vat_ron * sign;
        let tc = sales_tax_code_str(category, *rate);
        let rate_str = fmt_dec(*rate);

        // C 70x Venituri = net (701/704/707 by kind; 709 for granted reductions).
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: revenue_account(revenue_kind).to_string(),
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
#[allow(clippy::too_many_arguments)]
fn post_purchase_invoice(
    company_id: &str,
    received_invoice_id: &str,
    doc_number: &str,
    issue_date: &str,
    issuer_cui: &str,
    gross_ron: Decimal,
    vat_lines: &[(Decimal, Decimal, String, Decimal)], // (net, vat, category, rate)
    // Buyer-side TVA la încasare: when true, standard-rate ("S") input VAT is not yet
    // deductible at invoice date — debit 4428 "TVA neexigibilă" instead of 4426; it transfers
    // to 4426 on supplier payment (see post_received_payment). Reverse-charge (AE/K) never
    // defers (self-assessed immediately), so it keeps 4426 + the 4427 auto-assessment leg.
    cash_vat_deferred: bool,
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

        // D 4426 TVA deductibilă = VAT (sau 4428 neexigibilă când deducerea e amânată la plată
        // sub TVA la încasare — doar pentru "S"; AE/K se autolichidează imediat pe 4426).
        let vat_debit_account = if cash_vat_deferred && category == "S" {
            "4428"
        } else {
            "4426"
        };
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: vat_debit_account.to_string(),
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
    // The receivable (4111) is relieved at the INVOICE-date rate; the cash hits the bank at the
    // PAYMENT-date rate. For a foreign-currency invoice the difference is the FX result (665/765).
    cash_ron: Decimal,
    receivable_ron: Decimal,
    // Foreign-currency settlement uses the valută treasury accounts (5124/5314), not 5121/5311.
    foreign: bool,
    method: &str,
    // TVA la încasare: per-rate VAT made exigible by THIS collection (rate, vat_ron). For a
    // cash-VAT invoice each entry posts the exigibility transfer D 4428 / C 4427; empty for
    // normal-VAT invoices (no second leg). Cumulative over the invoice's receipts this clears
    // 4428 to zero exactly (vat_released trues up the final receipt).
    released: &[(Decimal, Decimal)],
) -> (GlJournal, Vec<GlEntry>) {
    // Use canonical partner ID so it matches MasterFiles and SourceDocuments
    let contact_id = canonical_partner_id(contact_id_raw, partner_cui.unwrap_or(""));
    // Treasury account by instrument + currency: cash → 5311/5314, bank/card → 5121/5124.
    let (debit_account, journal_id) = match (method.to_ascii_lowercase().as_str(), foreign) {
        ("cash" | "numerar", false) => ("5311", "CASA"),
        ("cash" | "numerar", true) => ("5314", "CASA"),
        (_, true) => ("5124", "BANCA"),
        (_, false) => ("5121", "BANCA"),
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
        // D 5121/5124/5311/5314 (treasury, payment-date rate) = cash_ron
        GlEntry {
            id: new_id(),
            record_id: 1,
            account_code: debit_account.to_string(),
            debit: cash_ron,
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
        // C 4111 Clienți (invoice-date rate) = receivable_ron
        GlEntry {
            id: new_id(),
            record_id: 2,
            account_code: "4111".to_string(),
            debit: Decimal::ZERO,
            credit: receivable_ron,
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

    let mut record_id: i64 = 3;

    // FX gain/loss (diferențe de curs valutar) — the receipt's RON differs from the receivable
    // because the rate moved between invoice and collection. cash > receivable → favourable
    // (C 765); cash < receivable → unfavourable (D 665).
    let fx_diff = cash_ron - receivable_ron;
    if !fx_diff.is_zero() {
        let (acc, debit, credit) = if fx_diff > Decimal::ZERO {
            ("765", Decimal::ZERO, fx_diff)
        } else {
            ("665", -fx_diff, Decimal::ZERO)
        };
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: acc.to_string(),
            debit,
            credit,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        });
        record_id += 1;
    }

    // TVA la încasare — exigibility transfer for the VAT made exigible by this collection:
    // per rate, D 4428 "TVA neexigibilă" / C 4427 "TVA colectată". Now the VAT enters the decont.
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
        // Denominator = the PAYABLE/collectible. Reverse-charge (AE) / intra-EU (K) VAT is
        // self-assessed and never paid to/by the supplier, so it is excluded — otherwise a
        // fully-settled mixed invoice would never reach gross and the S VAT would never fully
        // release (4428 stuck). (No-op on the sales side, where AE/K lines carry VAT=0.)
        let is_reverse_charge = matches!(cat.trim(), "AE" | "K");
        gross_bani += to_bani(base_ron);
        if !is_reverse_charge {
            gross_bani += to_bani(vat_ron);
        }
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

/// Buyer-side analogue of `cash_vat_release_for_payment`: the per-rate input VAT `(rate,
/// vat_ron)` made DEDUCTIBLE by a single supplier payment on a deferred received invoice — the
/// `D 4426 / C 4428` transfer. Builds the received invoice's "S" rate buckets + full gross
/// from received_invoice_vat_lines, the cumulative paid_before (strictly-earlier received
/// payments by paid_at, then id), then `allocate_collection` (true-up clears 4428 to zero).
async fn cash_vat_release_for_received_payment(
    pool: &SqlitePool,
    received_invoice_id: &str,
    payment_id: &str,
    paid_at: &str,
    currency: &str,
    fx: Option<Decimal>,
    amount_ron: Decimal,
) -> AppResult<Vec<(Decimal, Decimal)>> {
    use rust_decimal::prelude::ToPrimitive;
    use std::collections::BTreeMap;

    let line_rows = sqlx::query(
        "SELECT vat_category, vat_rate, base_amount, vat_amount \
         FROM received_invoice_vat_lines WHERE received_invoice_id = ?1",
    )
    .bind(received_invoice_id)
    .fetch_all(pool)
    .await?;

    let mut gross_bani: i64 = 0;
    let mut bucket_acc: BTreeMap<i64, (Decimal, i64, i64)> = BTreeMap::new();
    for l in &line_rows {
        let cat: String = l
            .try_get("vat_category")
            .unwrap_or_else(|_| "S".to_string());
        let rate_s: String = l.try_get("vat_rate").unwrap_or_default();
        let base_s: String = l.try_get("base_amount").unwrap_or_default();
        let vat_s: String = l.try_get("vat_amount").unwrap_or_default();
        let base_ron = amount_to_ron(dec(&base_s), currency, fx);
        let vat_ron = amount_to_ron(dec(&vat_s), currency, fx);
        // Denominator = the PAYABLE/collectible. Reverse-charge (AE) / intra-EU (K) VAT is
        // self-assessed and never paid to/by the supplier, so it is excluded — otherwise a
        // fully-settled mixed invoice would never reach gross and the S VAT would never fully
        // release (4428 stuck). (No-op on the sales side, where AE/K lines carry VAT=0.)
        let is_reverse_charge = matches!(cat.trim(), "AE" | "K");
        gross_bani += to_bani(base_ron);
        if !is_reverse_charge {
            gross_bani += to_bani(vat_ron);
        }
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

    let prior_rows = sqlx::query(
        "SELECT amount FROM received_invoice_payments \
         WHERE received_invoice_id = ?1 \
           AND (substr(paid_at,1,10) < substr(?2,1,10) \
                OR (substr(paid_at,1,10) = substr(?2,1,10) AND id < ?3))",
    )
    .bind(received_invoice_id)
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

/// Postare plată furnizor (payment-OUT against a received invoice): settle the payable and —
/// for a deferred cash-VAT invoice — release the now-deductible input VAT.
#[allow(clippy::too_many_arguments)]
fn post_received_payment(
    company_id: &str,
    payment_id: &str,
    received_invoice_id: &str,
    paid_at: &str,
    issuer_cui: &str,
    // Payable (401) relieved at INVOICE rate; cash leaves at PAYMENT rate → FX diff (665/765).
    cash_ron: Decimal,
    payable_ron: Decimal,
    foreign: bool,
    method: &str,
    // Per-rate input VAT made exigible/deductible by THIS payment (rate, vat_ron); empty for
    // a non-deferred invoice. Each posts the transfer D 4426 / C 4428.
    released: &[(Decimal, Decimal)],
) -> (GlJournal, Vec<GlEntry>) {
    let supplier_canon = canonical_partner_id(received_invoice_id, issuer_cui);
    // Money leaves: credit the treasury by instrument + currency (cash → 5311/5314, else 5121/5124).
    let (credit_account, journal_id) = match (method.to_ascii_lowercase().as_str(), foreign) {
        ("cash" | "numerar", false) => ("5311", "CASA"),
        ("cash" | "numerar", true) => ("5314", "CASA"),
        (_, true) => ("5124", "BANCA"),
        (_, false) => ("5121", "BANCA"),
    };
    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: journal_id.to_string(),
        journal_type: "PAYMENT".to_string(),
        transaction_id: payment_id.to_string(),
        transaction_date: paid_at.to_string(),
        description: Some(format!("Plata furnizor factura {received_invoice_id}")),
        source_type: "RECEIVED_PAYMENT".to_string(),
        source_id: payment_id.to_string(),
        customer_id: None,
        supplier_id: Some(supplier_canon.clone()),
    };

    let mut entries = vec![
        // D 401 Furnizori (invoice-date rate) = payable_ron
        GlEntry {
            id: new_id(),
            record_id: 1,
            account_code: "401".to_string(),
            debit: payable_ron,
            credit: Decimal::ZERO,
            partner_cui: Some(issuer_cui.to_string()),
            customer_id: None,
            supplier_id: Some(supplier_canon),
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        },
        // C 5121/5124/5311/5314 (treasury, payment-date rate) = cash_ron
        GlEntry {
            id: new_id(),
            record_id: 2,
            account_code: credit_account.to_string(),
            debit: Decimal::ZERO,
            credit: cash_ron,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        },
    ];

    let mut record_id: i64 = 3;

    // FX gain/loss on the payable: cash paid (payment rate) vs payable (invoice rate). Paid
    // MORE lei than the payable → unfavourable (D 665); paid FEWER → favourable (C 765).
    let fx_diff = cash_ron - payable_ron;
    if !fx_diff.is_zero() {
        let (acc, debit, credit) = if fx_diff > Decimal::ZERO {
            ("665", fx_diff, Decimal::ZERO)
        } else {
            ("765", Decimal::ZERO, -fx_diff)
        };
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: acc.to_string(),
            debit,
            credit,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "000".to_string(),
            tax_code: "000000".to_string(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        });
        record_id += 1;
    }

    // TVA la încasare — the deduction becomes exigible: per rate, D 4426 / C 4428.
    for (rate, vat) in released {
        if *vat == Decimal::ZERO {
            continue;
        }
        let tc = purchase_tax_code_str("S", *rate);
        let rate_str = fmt_dec(*rate);
        // D 4426 — now-deductible TVA deductibilă.
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
            tax_amount: Some(fmt_dec(*vat)),
        });
        record_id += 1;
        // C 4428 — release out of TVA neexigibilă.
        entries.push(GlEntry {
            id: new_id(),
            record_id,
            account_code: "4428".to_string(),
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

    (journal, entries)
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

    // Suppliers (contacts) flagged as applying cash VAT (RPATVAÎ), normalised CUIs — drives the
    // art. 297(2) buyer-side deferral (a purchase from such a supplier defers the deduction).
    let cash_vat_supplier_cuis: std::collections::HashSet<String> = {
        let rows = sqlx::query(
            "SELECT REPLACE(UPPER(TRIM(cui)),'RO','') AS ncui FROM contacts \
             WHERE company_id = ?1 AND cash_vat = 1 AND cui IS NOT NULL",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;
        rows.iter()
            .filter_map(|r| r.try_get::<String, _>("ncui").ok())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let supplier_on_cash_vat = |issuer_cui: &str| -> bool {
        cash_vat_supplier_cuis.contains(&issuer_cui.trim().to_uppercase().replace("RO", ""))
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
                    COALESCE(revenue_kind,'goods') AS revenue_kind, \
                    COALESCE(SUM(CAST(subtotal_amount AS REAL)),0.0) as net_sum, \
                    COALESCE(SUM(CAST(vat_amount AS REAL)),0.0) as vat_sum \
             FROM invoice_line_items \
             WHERE invoice_id = ?1 \
             GROUP BY vat_category, vat_rate, revenue_kind",
        )
        .bind(&inv_id)
        .fetch_all(pool)
        .await?;

        let vat_groups: Vec<(Decimal, Decimal, String, Decimal, String)> = group_rows
            .iter()
            .map(|r| {
                let cat: String = r
                    .try_get("vat_category")
                    .unwrap_or_else(|_| "S".to_string());
                let rate_s: String = r.try_get("vat_rate").unwrap_or_else(|_| "19".to_string());
                let revenue_kind: String = r
                    .try_get("revenue_kind")
                    .unwrap_or_else(|_| "goods".to_string());
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
                (net, vat, cat, dec(&rate_s), revenue_kind)
            })
            .collect();

        if vat_groups.is_empty() {
            continue; // invoice with no lines — skip
        }

        // "is_storno" = this document is a credit note (has storno_of_invoice_id). A STORNED
        // ORIGINAL is NOT a storno doc — it keeps its positive sale (it happened) and is reversed
        // by the separate credit note's negative lines. (Was previously also true for status==
        // STORNED, which double-inverted the sign vs D300 — FIX-1.)
        let is_storno = storno_ref.is_some();

        // Cash VAT: defer standard ("S") output VAT to 4428 (neexigibilă) only for a LIVE fresh
        // sale — i.e. not a credit note (is_storno) AND not a STORNED original. A STORNED original
        // will never be collected, so its VAT must stay exigible on 4427 (the payment loop posts no
        // 4428→4427 release for STORNED, and D300 counts it as collected at issue) — keeping the
        // three sides consistent. (Credit-note 4428/4427 reversal under cash VAT is deferred — see
        // CASH_VAT_DESIGN.md.)
        let cash_vat_applies = !is_storno && status != "STORNED" && in_cash_vat_window(&issue_date);

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

        // Buyer-side TVA la încasare: defer the "S" input VAT to 4428 when the supplier applies
        // cash VAT (art. 297(2)) OR the buyer applies it in-window (art. 297(3)).
        let cash_vat_deferred =
            in_cash_vat_window(&issue_date) || supplier_on_cash_vat(&issuer_cui);

        let (journal, entries) = post_purchase_invoice(
            company_id,
            &recv_id,
            &doc_number,
            &issue_date,
            &issuer_cui,
            gross_ron,
            &vat_lines,
            cash_vat_deferred,
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
        "SELECT p.id, p.invoice_id, p.paid_at, p.amount, p.method, \
                p.exchange_rate AS pay_rate, \
                COALESCE(i.currency,'RON') AS inv_currency, \
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
        // Use the INVOICE currency (the receivable was booked in it) — not the payment row's.
        let currency: String = row
            .try_get("inv_currency")
            .unwrap_or_else(|_| "RON".to_string());
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let contact_cui: Option<String> = row.try_get("contact_cui").unwrap_or(None);
        let method: String = row
            .try_get("method")
            .unwrap_or_else(|_| "transfer".to_string());

        // FX: the receivable in 4111 was booked at the INVOICE rate; the cash moves at the
        // PAYMENT rate, and the difference is the FX result (665/765).
        let inv_fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let pay_fx =
            parse_rate(row.try_get::<Option<f64>, _>("pay_rate").unwrap_or(None)).or(inv_fx);
        let receivable_ron = amount_to_ron(dec(&amount_s), &currency, inv_fx);
        // Only recognise an FX diff when the INVOICE rate is known (else the receivable itself
        // is the raw amount and a payment rate would book a fictitious gain/loss).
        let cash_ron = if inv_fx.is_some() {
            amount_to_ron(dec(&amount_s), &currency, pay_fx)
        } else {
            receivable_ron
        };
        let foreign = !currency.eq_ignore_ascii_case("RON");
        // Cash-VAT release works off the invoice-rate amount (the basis the 4428 was booked at).
        let amount_ron = receivable_ron;

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
            cash_ron,
            receivable_ron,
            foreign,
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

    // ── 4. Plăți furnizori (payments-out) ─────────────────────────────────────
    // Settle the payable (D 401 / C 512x) and, for a deferred cash-VAT invoice, release the
    // now-deductible input VAT (D 4426 / C 4428).
    let recv_payment_rows = sqlx::query(
        "SELECT rp.id, rp.received_invoice_id, rp.paid_at, rp.amount, rp.method, \
                rp.exchange_rate AS pay_rate, \
                ri.issuer_cui, ri.issue_date AS inv_issue_date, ri.exchange_rate, \
                COALESCE(ri.currency,'RON') AS inv_currency \
         FROM received_invoice_payments rp \
         JOIN received_invoices ri ON ri.id = rp.received_invoice_id \
         WHERE rp.company_id = ?1 \
           AND substr(rp.paid_at,1,10) >= ?2 \
           AND substr(rp.paid_at,1,10) <= ?3 \
           AND ri.status != 'REJECTED'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    for row in &recv_payment_rows {
        let pay_id: String = row.try_get("id").unwrap_or_default();
        let recv_id: String = row.try_get("received_invoice_id").unwrap_or_default();
        let paid_at: String = row.try_get("paid_at").unwrap_or_default();
        let amount_s: String = row.try_get("amount").unwrap_or_default();
        // Convert in the INVOICE currency (the VAT lines + payable live there; payments default
        // to it) — avoids a mismatch if a payment row's currency was overridden differently.
        let currency: String = row
            .try_get("inv_currency")
            .unwrap_or_else(|_| "RON".to_string());
        let method: String = row
            .try_get("method")
            .unwrap_or_else(|_| "transfer".to_string());
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let inv_issue: String = row.try_get("inv_issue_date").unwrap_or_default();
        let inv_fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let pay_fx =
            parse_rate(row.try_get::<Option<f64>, _>("pay_rate").unwrap_or(None)).or(inv_fx);
        let payable_ron = amount_to_ron(dec(&amount_s), &currency, inv_fx);
        // Recognise FX only when the invoice rate is known (else no fictitious diff).
        let cash_ron = if inv_fx.is_some() {
            amount_to_ron(dec(&amount_s), &currency, pay_fx)
        } else {
            payable_ron
        };
        let foreign = !currency.eq_ignore_ascii_case("RON");
        // Cash-VAT release works off the invoice-rate amount (the basis the 4428 was booked at).
        let amount_ron = payable_ron;

        // Release the deferred input VAT only for a deferred invoice (supplier OR buyer cash VAT).
        let deferred = in_cash_vat_window(&inv_issue) || supplier_on_cash_vat(&issuer_cui);
        let released: Vec<(Decimal, Decimal)> = if deferred {
            cash_vat_release_for_received_payment(
                pool, &recv_id, &pay_id, &paid_at, &currency, inv_fx, amount_ron,
            )
            .await?
        } else {
            Vec::new()
        };

        let (journal, entries) = post_received_payment(
            company_id,
            &pay_id,
            &recv_id,
            &paid_at,
            &issuer_cui,
            cash_ron,
            payable_ron,
            foreign,
            &method,
            &released,
        );
        assert_balanced(&entries, &pay_id)?;

        let mut tx = pool.begin().await?;
        let deleted = sqlx::query(
            "DELETE FROM gl_journal \
             WHERE company_id=?1 AND source_type='RECEIVED_PAYMENT' AND source_id=?2",
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

    // ── Net 4427 (credit − debit) ─────────────────────────────────────────────
    // Net, not Σcredit: a VAT-bearing reduction / credit note posts 4427 as a DEBIT, so the
    // exigible colectată is credit − debit — matching post_vat_settlement and D300.
    let c4427_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(e.credit AS REAL) - CAST(e.debit AS REAL)), 0.0) \
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

    // ── Net 4426 (debit − credit) ─────────────────────────────────────────────
    // Net, so a received credit note (4426 credit) reduces the deductibilă symmetrically.
    let d4426_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(e.debit AS REAL) - CAST(e.credit AS REAL)), 0.0) \
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

    // Tolerance, not strict ==, to match trial_balance/journal_register: per-journal rounding
    // (assert_balanced allows up to 0.005) can accumulate a sub-cent period imbalance.
    let balanced = (total_debit - total_credit).abs() < Decimal::new(1, 2);
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

// ─── VAT settlement (închiderea / regularizarea TVA) ─────────────────────────

/// Rezultatul închiderii TVA pentru o perioadă fiscală.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatSettlementResult {
    /// Net exigible TVA colectată (4427) for the period (RON).
    pub collected: String,
    /// Net exigible TVA deductibilă (4426) for the period (RON).
    pub deductible: String,
    /// collected − deductible.
    pub net_vat: String,
    /// TVA de plată (credit 4423) — the positive net, else "0.00".
    pub de_plata: String,
    /// TVA de recuperat (debit 4424) — the absolute negative net, else "0.00".
    pub de_recuperat: String,
    /// Date the closing note carries (last day of the period).
    pub entry_date: String,
    /// False when there is nothing to close (both 4426 and 4427 already zero).
    pub posted: bool,
}

/// Închiderea / regularizarea TVA la sfârșitul perioadei fiscale (OMFP 1802/2014). Netează
/// DOAR conturile EXIGIBILE 4426/4427 în 4423 "TVA de plată" sau 4424 "TVA de recuperat",
/// aducându-le la sold ZERO. Contul 4428 "TVA neexigibilă" și notele de închidere anterioare
/// (VAT_CLOSE) sunt EXCLUSE din netting — astfel taxarea inversă (D 4426 = C 4427) și
/// transferurile TVA la încasare (4428→4426/4427) sunt deja încorporate în solduri.
///
/// Nota este datată ultima zi a perioadei și este idempotentă (source_type='VAT_CLOSE').
/// NU compensează soldul 4424 din perioada precedentă (rămâne pe bilanț / se reportează în
/// D300 rd.38/40) și NU postează plata 4423 → 5121 — operațiuni separate.
pub async fn post_vat_settlement(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<VatSettlementResult> {
    // Net exigible balances straight from the GL, excluding 4428 and any prior close.
    let row = sqlx::query(
        "SELECT \
           COALESCE(SUM(CASE WHEN e.account_code='4427' \
                             THEN CAST(e.credit AS REAL)-CAST(e.debit AS REAL) ELSE 0 END),0.0) AS collected, \
           COALESCE(SUM(CASE WHEN e.account_code='4426' \
                             THEN CAST(e.debit AS REAL)-CAST(e.credit AS REAL) ELSE 0 END),0.0) AS deductible \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 \
           AND j.transaction_date >= ?2 AND j.transaction_date <= ?3 \
           AND j.source_type <> 'VAT_CLOSE' \
           AND e.account_code IN ('4426','4427')",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await?;

    let collected = Decimal::try_from(row.try_get::<f64, _>("collected").unwrap_or(0.0))
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let deductible = Decimal::try_from(row.try_get::<f64, _>("deductible").unwrap_or(0.0))
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let net_vat = collected - deductible;
    let source_id = format!("{period_from}_{period_to}");

    let mut tx = pool.begin().await?;
    // Idempotent: drop any prior close for this exact period before re-posting.
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='VAT_CLOSE' AND source_id=?2",
    )
    .bind(company_id)
    .bind(&source_id)
    .execute(&mut *tx)
    .await?;

    let de_plata = net_vat.max(Decimal::ZERO);
    let de_recuperat = (-net_vat).max(Decimal::ZERO);

    // Nothing to close (both exigible accounts already zero) → no journal.
    if collected.is_zero() && deductible.is_zero() {
        tx.commit().await?;
        return Ok(VatSettlementResult {
            collected: fmt_dec(collected),
            deductible: fmt_dec(deductible),
            net_vat: fmt_dec(net_vat),
            de_plata: fmt_dec(de_plata),
            de_recuperat: fmt_dec(de_recuperat),
            entry_date: period_to.to_string(),
            posted: false,
        });
    }

    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "DIVERSE".to_string(),
        journal_type: "VAT_CLOSE".to_string(),
        transaction_id: format!("REGTVA-{period_to}"),
        transaction_date: period_to.to_string(),
        description: Some(format!("Regularizare TVA {period_from} … {period_to}")),
        source_type: "VAT_CLOSE".to_string(),
        source_id: source_id.clone(),
        customer_id: None,
        supplier_id: None,
    };

    let mk = |record_id: i64, account: &str, debit: Decimal, credit: Decimal| GlEntry {
        id: new_id(),
        record_id,
        account_code: account.to_string(),
        debit,
        credit,
        partner_cui: None,
        customer_id: None,
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    };

    let mut entries: Vec<GlEntry> = Vec::new();
    let mut record_id: i64 = 1;
    // D 4427 — zero the collected.
    if !collected.is_zero() {
        entries.push(mk(record_id, "4427", collected, Decimal::ZERO));
        record_id += 1;
    }
    // C 4426 — zero the deductible.
    if !deductible.is_zero() {
        entries.push(mk(record_id, "4426", Decimal::ZERO, deductible));
        record_id += 1;
    }
    // Difference → 4423 (de plată) or 4424 (de recuperat); never both.
    if net_vat > Decimal::ZERO {
        entries.push(mk(record_id, "4423", Decimal::ZERO, net_vat));
    } else if net_vat < Decimal::ZERO {
        entries.push(mk(record_id, "4424", -net_vat, Decimal::ZERO));
    }

    assert_balanced(&entries, &source_id)?;

    let journal_pk = journal.id.clone();
    insert_journal(&mut tx, &journal).await?;
    for e in &entries {
        insert_entry(&mut tx, &journal_pk, e).await?;
    }
    tx.commit().await?;

    Ok(VatSettlementResult {
        collected: fmt_dec(collected),
        deductible: fmt_dec(deductible),
        net_vat: fmt_dec(net_vat),
        de_plata: fmt_dec(de_plata),
        de_recuperat: fmt_dec(de_recuperat),
        entry_date: period_to.to_string(),
        posted: true,
    })
}

// ─── Închiderea conturilor 6/7 → 121 (rezultatul perioadei) ──────────────────

/// Result of posting the period-close (6/7 → 121).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosePeriodResult {
    pub total_revenue: String,
    pub total_expense: String,
    pub result: String,
    pub entries_count: i64,
    pub posted: bool,
    pub entry_date: String,
}

/// Net class-6/7 balances for the period (debit-positive), EXCLUDING any prior period-close
/// (`source_type='PNL_CLOSE'`) so the figures are the pre-close activity and re-posting is
/// idempotent. Returns (account_code, account_name, net_debit) for non-zero accounts only.
async fn class67_balances(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<Vec<(String, String, Decimal)>> {
    use std::collections::HashMap;
    let name_rows = sqlx::query(
        "SELECT account_code, account_name FROM chart_of_accounts WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut names: HashMap<String, String> = HashMap::new();
    for r in &name_rows {
        let c: String = r.try_get("account_code").unwrap_or_default();
        let n: String = r.try_get("account_name").unwrap_or_default();
        names.insert(c, n);
    }
    let rows = sqlx::query(
        "SELECT e.account_code, \
           COALESCE(SUM(CAST(e.debit AS REAL)-CAST(e.credit AS REAL)),0.0) AS net_debit \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date >= ?2 AND j.transaction_date <= ?3 \
           AND j.source_type <> 'PNL_CLOSE' \
           AND (e.account_code LIKE '6%' OR e.account_code LIKE '7%') \
         GROUP BY e.account_code ORDER BY e.account_code",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        let code: String = r.try_get("account_code").unwrap_or_default();
        let net_debit = dec_f(r.try_get::<f64, _>("net_debit").unwrap_or(0.0));
        if net_debit.is_zero() {
            continue;
        }
        let name = names.get(&code).cloned().unwrap_or_else(|| code.clone());
        out.push((code, name, net_debit));
    }
    Ok(out)
}

/// Trial-balance rows (closing balances only) for the P&L, built from `class67_balances` so the
/// P&L reflects the period's revenue/expense even after the formal close has zeroed 6/7 in the GL.
async fn pnl_rows(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<Vec<TrialBalanceRow>> {
    let balances = class67_balances(pool, company_id, period_from, period_to).await?;
    Ok(balances
        .into_iter()
        .map(|(code, name, net_debit)| TrialBalanceRow {
            account_code: code,
            account_name: name,
            opening_debit: "0.00".into(),
            opening_credit: "0.00".into(),
            period_debit: "0.00".into(),
            period_credit: "0.00".into(),
            total_debit: "0.00".into(),
            total_credit: "0.00".into(),
            closing_debit: fmt_dec(net_debit.max(Decimal::ZERO)),
            closing_credit: fmt_dec((-net_debit).max(Decimal::ZERO)),
        })
        .collect())
}

/// Build the P&L for a period (regime-aware), reading class-6/7 activity excluding any prior close.
pub async fn profit_and_loss(
    pool: &SqlitePool,
    company_id: &str,
    tax_regime: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<ProfitLoss> {
    let rows = pnl_rows(pool, company_id, period_from, period_to).await?;
    Ok(compute_pnl(&rows, tax_regime, period_from, period_to))
}

/// Post the period-close: sweep every class-6/7 balance into 121 (OMFP 1802/2014 — D 7xx / C 121
/// for revenue, D 121 / C 6xx for expense; contra accounts handled by balance sign). Idempotent
/// per `(company_id, 'PNL_CLOSE', period)`. Does NOT post the income-tax expense (the accountant
/// books 691/698 separately with the exact figure) and does NOT touch the annual 121→117 reset.
pub async fn post_period_close(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<ClosePeriodResult> {
    let balances = class67_balances(pool, company_id, period_from, period_to).await?;
    let source_id = format!("{period_from}_{period_to}");

    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='PNL_CLOSE' AND source_id=?2",
    )
    .bind(company_id)
    .bind(&source_id)
    .execute(&mut *tx)
    .await?;

    let mk = |record_id: i64, account: &str, debit: Decimal, credit: Decimal| GlEntry {
        id: new_id(),
        record_id,
        account_code: account.to_string(),
        debit,
        credit,
        partner_cui: None,
        customer_id: None,
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    };

    let mut entries: Vec<GlEntry> = Vec::new();
    let mut record_id: i64 = 1;
    let mut debit_121 = Decimal::ZERO; // total expenses swept (D 121)
    let mut credit_121 = Decimal::ZERO; // total revenue swept (C 121)
    for (code, _name, net_debit) in &balances {
        if *net_debit > Decimal::ZERO {
            // Debit-balance account (expense / contra-revenue) → credit it to zero, debit 121.
            entries.push(mk(record_id, code, Decimal::ZERO, *net_debit));
            debit_121 += *net_debit;
        } else {
            // Credit-balance account (revenue / contra-expense) → debit it to zero, credit 121.
            let cr = -*net_debit;
            entries.push(mk(record_id, code, cr, Decimal::ZERO));
            credit_121 += cr;
        }
        record_id += 1;
    }
    let result = credit_121 - debit_121;

    if entries.is_empty() {
        tx.commit().await?;
        return Ok(ClosePeriodResult {
            total_revenue: fmt_dec(credit_121),
            total_expense: fmt_dec(debit_121),
            result: fmt_dec(result),
            entries_count: 0,
            posted: false,
            entry_date: period_to.to_string(),
        });
    }
    // The two 121 legs (gross revenue close + gross expense close) — keeps 121's turnover correct.
    if !credit_121.is_zero() {
        entries.push(mk(record_id, "121", Decimal::ZERO, credit_121));
        record_id += 1;
    }
    if !debit_121.is_zero() {
        entries.push(mk(record_id, "121", debit_121, Decimal::ZERO));
    }
    assert_balanced(&entries, &source_id)?;

    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "DIVERSE".to_string(),
        journal_type: "PNL_CLOSE".to_string(),
        transaction_id: format!("INCHID-{period_to}"),
        transaction_date: period_to.to_string(),
        description: Some(format!(
            "Închidere conturi 6/7 → 121 ({period_from} … {period_to})"
        )),
        source_type: "PNL_CLOSE".to_string(),
        source_id: source_id.clone(),
        customer_id: None,
        supplier_id: None,
    };
    let journal_pk = journal.id.clone();
    insert_journal(&mut tx, &journal).await?;
    for e in &entries {
        insert_entry(&mut tx, &journal_pk, e).await?;
    }
    tx.commit().await?;

    Ok(ClosePeriodResult {
        total_revenue: fmt_dec(credit_121),
        total_expense: fmt_dec(debit_121),
        result: fmt_dec(result),
        entries_count: entries.len() as i64,
        posted: true,
        entry_date: period_to.to_string(),
    })
}

// ─── Impozit pe venit/profit (691/698 → 4411/4418) + închidere anuală 121 → 117 ─

/// Result of posting the income-tax expense.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomeTaxResult {
    pub tax_regime: String,
    pub expense_account: String,
    pub payable_account: String,
    pub amount: String,
    pub estimated: bool,
    pub posted: bool,
    pub entry_date: String,
}

/// Post the income-tax expense for the period: micro → D 698 / C 4418 (1% × venituri); profit →
/// D 691 / C 4411 (16% × rezultat brut pozitiv). `amount` overrides the estimate (e.g. the exact
/// D101 figure). Idempotent per `(company, 'TAX_CLOSE', period)`. The 698/691 balance is later
/// swept into 121 by `post_period_close` (so the result is net of tax) — run this BEFORE the close.
pub async fn post_income_tax(
    pool: &SqlitePool,
    company_id: &str,
    tax_regime: &str,
    period_from: &str,
    period_to: &str,
    amount: Option<Decimal>,
) -> AppResult<IncomeTaxResult> {
    let (expense_account, payable_account) = if tax_regime == "micro" {
        ("698", "4418")
    } else {
        ("691", "4411")
    };
    // Estimate from the pre-tax P&L if no explicit amount: micro 1% × venituri, profit 16% × brut+.
    let (amount, estimated) = match amount {
        Some(a) => (a.max(Decimal::ZERO).round_dp(2), false),
        None => {
            let pnl = profit_and_loss(pool, company_id, tax_regime, period_from, period_to).await?;
            let v = if tax_regime == "micro" {
                pnl.total_revenue
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO)
                    * Decimal::new(1, 2)
            } else {
                pnl.gross_result
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO)
                    .max(Decimal::ZERO)
                    * Decimal::new(16, 2)
            };
            (v.round_dp(2), true)
        }
    };
    let source_id = format!("{period_from}_{period_to}");

    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='TAX_CLOSE' AND source_id=?2",
    )
    .bind(company_id)
    .bind(&source_id)
    .execute(&mut *tx)
    .await?;

    if amount.is_zero() {
        tx.commit().await?;
        return Ok(IncomeTaxResult {
            tax_regime: tax_regime.to_string(),
            expense_account: expense_account.to_string(),
            payable_account: payable_account.to_string(),
            amount: fmt_dec(amount),
            estimated,
            posted: false,
            entry_date: period_to.to_string(),
        });
    }

    let mk = |record_id: i64, account: &str, debit: Decimal, credit: Decimal| GlEntry {
        id: new_id(),
        record_id,
        account_code: account.to_string(),
        debit,
        credit,
        partner_cui: None,
        customer_id: None,
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    };
    let entries = vec![
        mk(1, expense_account, amount, Decimal::ZERO),
        mk(2, payable_account, Decimal::ZERO, amount),
    ];
    assert_balanced(&entries, &source_id)?;
    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "DIVERSE".to_string(),
        journal_type: "TAX_CLOSE".to_string(),
        transaction_id: format!("IMPOZIT-{period_to}"),
        transaction_date: period_to.to_string(),
        description: Some(format!(
            "Impozit pe {} {period_from} … {period_to}",
            if tax_regime == "micro" {
                "venit"
            } else {
                "profit"
            }
        )),
        source_type: "TAX_CLOSE".to_string(),
        source_id: source_id.clone(),
        customer_id: None,
        supplier_id: None,
    };
    let journal_pk = journal.id.clone();
    insert_journal(&mut tx, &journal).await?;
    for e in &entries {
        insert_entry(&mut tx, &journal_pk, e).await?;
    }
    tx.commit().await?;

    Ok(IncomeTaxResult {
        tax_regime: tax_regime.to_string(),
        expense_account: expense_account.to_string(),
        payable_account: payable_account.to_string(),
        amount: fmt_dec(amount),
        estimated,
        posted: true,
        entry_date: period_to.to_string(),
    })
}

/// Result of the annual 121 → 117 reset.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnualCloseResult {
    pub year: i32,
    pub result_121: String,
    /// "profit" (D 121 / C 117) | "loss" (D 117 / C 121) | "zero".
    pub kind: String,
    pub posted: bool,
    pub entry_date: String,
}

/// Annual reset: transfer the year's 121 «Profit sau pierdere» balance to 117 «Rezultatul
/// reportat» (OMFP 1802/2014) — profit (121 credit) → D 121 / C 117; loss (121 debit) → D 117 /
/// C 121. Posted at the START of the next year. Idempotent per `(company, 'ANNUAL_CLOSE', year)`.
/// Reads the 121 balance EXCLUDING any prior ANNUAL_CLOSE so re-running is safe.
pub async fn post_annual_close(
    pool: &SqlitePool,
    company_id: &str,
    year: i32,
) -> AppResult<AnnualCloseResult> {
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let row = sqlx::query(
        "SELECT COALESCE(SUM(CAST(e.credit AS REAL)-CAST(e.debit AS REAL)),0.0) AS net_credit \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id=?1 AND e.account_code='121' \
           AND j.transaction_date >= ?2 AND j.transaction_date <= ?3 \
           AND j.source_type <> 'ANNUAL_CLOSE'",
    )
    .bind(company_id)
    .bind(&from)
    .bind(&to)
    .fetch_one(pool)
    .await?;
    let net_credit = dec_f(row.try_get::<f64, _>("net_credit").unwrap_or(0.0)); // profit if > 0
    let source_id = year.to_string();
    let entry_date = format!("{}-01-01", year + 1);

    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='ANNUAL_CLOSE' AND source_id=?2",
    )
    .bind(company_id)
    .bind(&source_id)
    .execute(&mut *tx)
    .await?;

    if net_credit.is_zero() {
        tx.commit().await?;
        return Ok(AnnualCloseResult {
            year,
            result_121: fmt_dec(net_credit),
            kind: "zero".into(),
            posted: false,
            entry_date,
        });
    }

    let mk = |record_id: i64, account: &str, debit: Decimal, credit: Decimal| GlEntry {
        id: new_id(),
        record_id,
        account_code: account.to_string(),
        debit,
        credit,
        partner_cui: None,
        customer_id: None,
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    };
    // Profit (121 credit balance): D 121 / C 117. Loss (121 debit balance): D 117 / C 121.
    let (kind, entries) = if net_credit > Decimal::ZERO {
        (
            "profit",
            vec![
                mk(1, "121", net_credit, Decimal::ZERO),
                mk(2, "117", Decimal::ZERO, net_credit),
            ],
        )
    } else {
        let loss = -net_credit;
        (
            "loss",
            vec![
                mk(1, "117", loss, Decimal::ZERO),
                mk(2, "121", Decimal::ZERO, loss),
            ],
        )
    };
    assert_balanced(&entries, &source_id)?;
    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "DIVERSE".to_string(),
        journal_type: "ANNUAL_CLOSE".to_string(),
        transaction_id: format!("REPORTAT-{year}"),
        transaction_date: entry_date.clone(),
        description: Some(format!("Închidere anuală 121 → 117 (rezultat {year})")),
        source_type: "ANNUAL_CLOSE".to_string(),
        source_id: source_id.clone(),
        customer_id: None,
        supplier_id: None,
    };
    let journal_pk = journal.id.clone();
    insert_journal(&mut tx, &journal).await?;
    for e in &entries {
        insert_entry(&mut tx, &journal_pk, e).await?;
    }
    tx.commit().await?;

    Ok(AnnualCloseResult {
        year,
        result_121: fmt_dec(net_credit),
        kind: kind.into(),
        posted: true,
        entry_date,
    })
}

// ─── Salarii (statul de plată → GL) ──────────────────────────────────────────

/// Result of posting the monthly payroll to the GL.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollPostResult {
    pub gross: String,
    pub net: String,
    pub posted: bool,
    pub entry_date: String,
}

/// Post the monthly payroll aggregate to the GL (OMFP 1802/2014 monograph): D 641 / C 421 (gross);
/// D 421 / C 4315 (CAS), C 4316 (CASS), C 444 (impozit) — withholdings; D 646 / C 436 (CAM,
/// employer). After this, 421 = net payable. Idempotent per `(company,'PAYROLL',period)`.
#[allow(clippy::too_many_arguments)]
pub async fn post_payroll(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
    gross: Decimal,
    cas: Decimal,
    cass: Decimal,
    impozit: Decimal,
    cam: Decimal,
) -> AppResult<PayrollPostResult> {
    let source_id = format!("{period_from}_{period_to}");
    let net = gross - cas - cass - impozit;

    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='PAYROLL' AND source_id=?2",
    )
    .bind(company_id)
    .bind(&source_id)
    .execute(&mut *tx)
    .await?;

    if gross.is_zero() {
        tx.commit().await?;
        return Ok(PayrollPostResult {
            gross: fmt_dec(gross),
            net: fmt_dec(net),
            posted: false,
            entry_date: period_to.to_string(),
        });
    }

    let mk = |record_id: i64, account: &str, debit: Decimal, credit: Decimal| GlEntry {
        id: new_id(),
        record_id,
        account_code: account.to_string(),
        debit,
        credit,
        partner_cui: None,
        customer_id: None,
        supplier_id: None,
        tax_type: "000".to_string(),
        tax_code: "000000".to_string(),
        tax_percentage: None,
        tax_base: None,
        tax_amount: None,
    };
    let mut entries = vec![
        mk(1, "641", gross, Decimal::ZERO), // cheltuieli salarii
        mk(2, "421", Decimal::ZERO, gross), // salarii datorate
    ];
    let mut rec = 3;
    let withholding = cas + cass + impozit;
    if !withholding.is_zero() {
        entries.push(mk(rec, "421", withholding, Decimal::ZERO));
        rec += 1;
    }
    if !cas.is_zero() {
        entries.push(mk(rec, "4315", Decimal::ZERO, cas));
        rec += 1;
    }
    if !cass.is_zero() {
        entries.push(mk(rec, "4316", Decimal::ZERO, cass));
        rec += 1;
    }
    if !impozit.is_zero() {
        entries.push(mk(rec, "444", Decimal::ZERO, impozit));
        rec += 1;
    }
    if !cam.is_zero() {
        entries.push(mk(rec, "646", cam, Decimal::ZERO)); // cheltuieli CAM (angajator)
        entries.push(mk(rec + 1, "436", Decimal::ZERO, cam)); // CAM datorată
    }
    assert_balanced(&entries, &source_id)?;

    let journal = GlJournal {
        id: new_id(),
        company_id: company_id.to_string(),
        journal_id: "SALARII".to_string(),
        journal_type: "PAYROLL".to_string(),
        transaction_id: format!("SAL-{period_to}"),
        transaction_date: period_to.to_string(),
        description: Some(format!("State de salarii {period_from} … {period_to}")),
        source_type: "PAYROLL".to_string(),
        source_id: source_id.clone(),
        customer_id: None,
        supplier_id: None,
    };
    let journal_pk = journal.id.clone();
    insert_journal(&mut tx, &journal).await?;
    for e in &entries {
        insert_entry(&mut tx, &journal_pk, e).await?;
    }
    tx.commit().await?;

    Ok(PayrollPostResult {
        gross: fmt_dec(gross),
        net: fmt_dec(net),
        posted: true,
        entry_date: period_to.to_string(),
    })
}

// ─── Balanța de verificare (cod 14-6-30, patru egalități) ────────────────────

/// f64 → Decimal rounded to 2 decimals (GL sums come back as REAL).
fn dec_f(f: f64) -> Decimal {
    Decimal::try_from(f).unwrap_or(Decimal::ZERO).round_dp(2)
}

/// One account row of the balanța de verificare.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalanceRow {
    pub account_code: String,
    pub account_name: String,
    pub opening_debit: String,
    pub opening_credit: String,
    pub period_debit: String,
    pub period_credit: String,
    pub total_debit: String,
    pub total_credit: String,
    pub closing_debit: String,
    pub closing_credit: String,
}

/// Balanța de verificare with the four column-pairs + footer totals + the balanced flag.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialBalance {
    pub rows: Vec<TrialBalanceRow>,
    pub total_opening_debit: String,
    pub total_opening_credit: String,
    pub total_period_debit: String,
    pub total_period_credit: String,
    pub total_total_debit: String,
    pub total_total_credit: String,
    pub total_closing_debit: String,
    pub total_closing_credit: String,
    /// True when all four egalități hold (within 0.01 RON).
    pub balanced: bool,
}

/// Balanța de verificare (model cod 14-6-30, "cu patru egalități"; OMFP 2634/2015), derived
/// from the GL. Per synthetic account: solduri inițiale (net sold before `period_from`),
/// rulajele perioadei (debit/credit în interval), total sume (= sold inițial + rulaj, pe parte)
/// și solduri finale. Obligatorie LUNAR (Legea 82/1991, modificată prin OUG 138/2024).
pub async fn trial_balance(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<TrialBalance> {
    use std::collections::HashMap;

    // Account names from the chart.
    let name_rows = sqlx::query(
        "SELECT account_code, account_name FROM chart_of_accounts WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut names: HashMap<String, String> = HashMap::new();
    for r in &name_rows {
        let c: String = r.try_get("account_code").unwrap_or_default();
        let n: String = r.try_get("account_name").unwrap_or_default();
        names.insert(c, n);
    }

    // Per account: opening net (< period_from) + period debit/credit ([period_from, period_to]).
    let rows = sqlx::query(
        "SELECT e.account_code, \
           COALESCE(SUM(CASE WHEN j.transaction_date < ?2 \
                             THEN CAST(e.debit AS REAL)-CAST(e.credit AS REAL) ELSE 0 END),0.0) AS opening_net, \
           COALESCE(SUM(CASE WHEN j.transaction_date >= ?2 THEN CAST(e.debit AS REAL) ELSE 0 END),0.0) AS period_debit, \
           COALESCE(SUM(CASE WHEN j.transaction_date >= ?2 THEN CAST(e.credit AS REAL) ELSE 0 END),0.0) AS period_credit \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date <= ?3 \
         GROUP BY e.account_code ORDER BY e.account_code",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    let mut out: Vec<TrialBalanceRow> = Vec::new();
    let mut t_od = Decimal::ZERO;
    let mut t_oc = Decimal::ZERO;
    let mut t_pd = Decimal::ZERO;
    let mut t_pc = Decimal::ZERO;
    let mut t_td = Decimal::ZERO;
    let mut t_tc = Decimal::ZERO;
    let mut t_cd = Decimal::ZERO;
    let mut t_cc = Decimal::ZERO;

    for r in &rows {
        let code: String = r.try_get("account_code").unwrap_or_default();
        let opening_net = dec_f(r.try_get::<f64, _>("opening_net").unwrap_or(0.0));
        let period_d = dec_f(r.try_get::<f64, _>("period_debit").unwrap_or(0.0));
        let period_c = dec_f(r.try_get::<f64, _>("period_credit").unwrap_or(0.0));
        let opening_d = opening_net.max(Decimal::ZERO);
        let opening_c = (-opening_net).max(Decimal::ZERO);
        let total_d = opening_d + period_d;
        let total_c = opening_c + period_c;
        let closing_net = total_d - total_c;
        let closing_d = closing_net.max(Decimal::ZERO);
        let closing_c = (-closing_net).max(Decimal::ZERO);

        // Skip accounts with no opening balance and no period movement.
        if opening_d.is_zero() && opening_c.is_zero() && period_d.is_zero() && period_c.is_zero() {
            continue;
        }

        t_od += opening_d;
        t_oc += opening_c;
        t_pd += period_d;
        t_pc += period_c;
        t_td += total_d;
        t_tc += total_c;
        t_cd += closing_d;
        t_cc += closing_c;

        let name = names.get(&code).cloned().unwrap_or_else(|| code.clone());
        out.push(TrialBalanceRow {
            account_code: code,
            account_name: name,
            opening_debit: fmt_dec(opening_d),
            opening_credit: fmt_dec(opening_c),
            period_debit: fmt_dec(period_d),
            period_credit: fmt_dec(period_c),
            total_debit: fmt_dec(total_d),
            total_credit: fmt_dec(total_c),
            closing_debit: fmt_dec(closing_d),
            closing_credit: fmt_dec(closing_c),
        });
    }

    let tol = Decimal::new(1, 2); // 0.01 RON
    let balanced = (t_od - t_oc).abs() < tol
        && (t_pd - t_pc).abs() < tol
        && (t_td - t_tc).abs() < tol
        && (t_cd - t_cc).abs() < tol;

    Ok(TrialBalance {
        rows: out,
        total_opening_debit: fmt_dec(t_od),
        total_opening_credit: fmt_dec(t_oc),
        total_period_debit: fmt_dec(t_pd),
        total_period_credit: fmt_dec(t_pc),
        total_total_debit: fmt_dec(t_td),
        total_total_credit: fmt_dec(t_tc),
        total_closing_debit: fmt_dec(t_cd),
        total_closing_credit: fmt_dec(t_cc),
        balanced,
    })
}

// ─── Cont de profit și pierdere + închiderea conturilor 6/7 → 121 ────────────

/// One revenue/expense line of the P&L.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PnlLine {
    pub code: String,
    pub name: String,
    pub amount: String,
}

/// One closing entry (D account / C 121, or D 121 / C account).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosingEntry {
    pub debit_account: String,
    pub credit_account: String,
    pub amount: String,
}

/// Contul de profit și pierdere (P&L) for a period, derived from the trial balance: class-7
/// balances are revenue, class-6 are expenses. `income_tax` is the booked 691/698 if present,
/// else estimated by regime (micro 1% × venituri, profit 16% × rezultat brut). `closing_entries`
/// previews the OMFP-1802 close (D 7xx / C 121 and D 121 / C 6xx) the accountant would post.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitLoss {
    pub period_from: String,
    pub period_to: String,
    pub tax_regime: String,
    pub revenue_lines: Vec<PnlLine>,
    pub expense_lines: Vec<PnlLine>,
    pub operating_revenue: String,
    pub financial_revenue: String,
    pub total_revenue: String,
    pub operating_expense: String,
    pub financial_expense: String,
    pub total_expense: String,
    /// venituri − cheltuieli (excluding the income-tax expense 691/698).
    pub gross_result: String,
    pub income_tax: String,
    /// True when `income_tax` is an estimate (nothing booked to 691/698 yet).
    pub income_tax_estimated: bool,
    pub net_result: String,
    pub closing_entries: Vec<ClosingEntry>,
}

/// Build the P&L from trial-balance rows. Pure + testable. `tax_regime` is "micro" or "profit".
pub fn compute_pnl(
    rows: &[TrialBalanceRow],
    tax_regime: &str,
    period_from: &str,
    period_to: &str,
) -> ProfitLoss {
    let parse = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let mut revenue_lines = Vec::new();
    let mut expense_lines = Vec::new();
    let mut closing_entries = Vec::new();
    let mut op_rev = Decimal::ZERO;
    let mut fin_rev = Decimal::ZERO;
    let mut op_exp = Decimal::ZERO;
    let mut fin_exp = Decimal::ZERO;
    let mut income_tax_booked = Decimal::ZERO;

    for r in rows {
        let code = &r.account_code;
        let net_credit = parse(&r.closing_credit) - parse(&r.closing_debit);
        let net_debit = -net_credit;
        if code.starts_with('7') {
            // Revenue: normal credit balance. Skip zero. Financial = 76x/78x, else operating.
            if net_credit.is_zero() {
                continue;
            }
            if code.starts_with("76") || code.starts_with("78") {
                fin_rev += net_credit;
            } else {
                op_rev += net_credit;
            }
            revenue_lines.push(PnlLine {
                code: code.clone(),
                name: r.account_name.clone(),
                amount: fmt_dec(net_credit),
            });
            // D 7xx / C 121 (sign handles contra-revenue like 709 automatically via net_credit<0).
            closing_entries.push(ClosingEntry {
                debit_account: code.clone(),
                credit_account: "121".into(),
                amount: fmt_dec(net_credit),
            });
        } else if code.starts_with('6') {
            // Expense: normal debit balance. 691/698 (income tax) are reported separately.
            if net_debit.is_zero() {
                continue;
            }
            if code == "691" || code == "698" {
                income_tax_booked += net_debit;
                continue;
            }
            if code.starts_with("66") || code == "686" {
                fin_exp += net_debit;
            } else {
                op_exp += net_debit;
            }
            expense_lines.push(PnlLine {
                code: code.clone(),
                name: r.account_name.clone(),
                amount: fmt_dec(net_debit),
            });
            // D 121 / C 6xx.
            closing_entries.push(ClosingEntry {
                debit_account: "121".into(),
                credit_account: code.clone(),
                amount: fmt_dec(net_debit),
            });
        }
    }

    let total_revenue = op_rev + fin_rev;
    let total_expense = op_exp + fin_exp;
    let gross_result = total_revenue - total_expense;
    // Income tax: booked 691/698 if any, else estimate by regime. Micro is 1% of revenue;
    // profit is 16% of the positive gross result (accounting result — fiscal adjustments aside).
    let (income_tax, income_tax_estimated) = if !income_tax_booked.is_zero() {
        (income_tax_booked, false)
    } else if tax_regime == "micro" {
        ((total_revenue * Decimal::new(1, 2)).round_dp(2), true)
    } else {
        (
            (gross_result.max(Decimal::ZERO) * Decimal::new(16, 2)).round_dp(2),
            true,
        )
    };
    let net_result = gross_result - income_tax;

    ProfitLoss {
        period_from: period_from.to_string(),
        period_to: period_to.to_string(),
        tax_regime: tax_regime.to_string(),
        revenue_lines,
        expense_lines,
        operating_revenue: fmt_dec(op_rev),
        financial_revenue: fmt_dec(fin_rev),
        total_revenue: fmt_dec(total_revenue),
        operating_expense: fmt_dec(op_exp),
        financial_expense: fmt_dec(fin_exp),
        total_expense: fmt_dec(total_expense),
        gross_result: fmt_dec(gross_result),
        income_tax: fmt_dec(income_tax),
        income_tax_estimated,
        net_result: fmt_dec(net_result),
        closing_entries,
    }
}

// ─── Bilanț contabil (balance sheet) ─────────────────────────────────────────

/// Synthetic-level balance sheet (bilanț prescurtat essence), derived from the trial balance.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BilantReport {
    pub period_to: String,
    // ACTIVE
    pub immobilized_assets: String,
    pub inventory: String,
    pub receivables: String,
    pub short_investments: String,
    pub cash_bank: String,
    pub prepaid_expenses: String,
    pub total_assets: String,
    // CAPITALURI ȘI DATORII
    pub equity: String,
    /// Rezultatul exercițiului (inclus în capitaluri); 0 dacă perioada e deja închisă în 121.
    pub current_result: String,
    pub provisions: String,
    pub long_term_debt: String,
    pub current_liabilities: String,
    pub deferred_revenue: String,
    pub total_equity_liabilities: String,
    /// Active = Capitaluri + Datorii (în limita a 0,01 lei).
    pub balanced: bool,
    pub entity_size_note: String,
}

/// Build the balance sheet from the (full) trial-balance rows. Class 1-5 are classified by code +
/// balance side; the class-6/7 net result is folded into equity as the period result, so the sheet
/// balances whether or not the formal 6/7 → 121 close has been posted. Pure + testable.
pub fn compute_bilant(rows: &[TrialBalanceRow], period_to: &str) -> BilantReport {
    let parse = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let z = Decimal::ZERO;
    let (mut immob, mut inv, mut recv, mut shinv, mut cash, mut prepaid) = (z, z, z, z, z, z);
    let (mut equity, mut provisions, mut ltd, mut curr_liab, mut def_rev) = (z, z, z, z, z);
    let (mut rev7, mut exp6) = (z, z);
    for r in rows {
        let code = &r.account_code;
        let nd = parse(&r.closing_debit) - parse(&r.closing_credit); // net debit (asset side +)
        let nc = -nd; // net credit (liability/equity side +)
        match code.chars().next() {
            Some('2') => immob += nd, // 20x/21x/23x/26x net of 28x/29x (credit) by sign
            Some('3') => inv += nd,   // stocuri net of 39x
            Some('4') => {
                if code == "471" {
                    prepaid += nd;
                } else if code == "472" || code.starts_with("475") {
                    def_rev += nc;
                } else if nd > z {
                    recv += nd;
                } else {
                    curr_liab += nc;
                }
            }
            Some('5') => {
                if code.starts_with("50") || code.starts_with("59") {
                    shinv += nd;
                } else if nd > z {
                    cash += nd;
                } else {
                    curr_liab += nc; // e.g. 519 credite bancare pe termen scurt
                }
            }
            Some('1') => {
                if code.starts_with("15") {
                    provisions += nc;
                } else if code.starts_with("16") {
                    ltd += nc;
                } else {
                    equity += nc; // 10x/11x/12x (incl. 121 if already closed; 129 debit subtracts)
                }
            }
            Some('6') => exp6 += nd,
            Some('7') => rev7 += nc,
            _ => {}
        }
    }
    let current_result = rev7 - exp6; // 0 when 6/7 already swept to 121
    let equity_total = equity + current_result;
    let total_assets = immob + inv + recv + shinv + cash + prepaid;
    let total_el = equity_total + provisions + ltd + curr_liab + def_rev;
    let balanced = (total_assets - total_el).abs() < Decimal::new(1, 2);
    let entity_size_note = if total_assets <= Decimal::from(2_250_000) {
        "Probabil microîntreprindere (active ≤ 2.250.000 lei) → bilanț prescurtat, formular S1005. \
         Încadrarea finală cere 2 din 3 criterii (active, cifră de afaceri, nr. salariați)."
    } else if total_assets <= Decimal::from(25_000_000) {
        "Probabil entitate mică → bilanț prescurtat, formular S1003."
    } else {
        "Probabil entitate mijlocie/mare → bilanț dezvoltat, formular S1002."
    }
    .to_string();

    BilantReport {
        period_to: period_to.to_string(),
        immobilized_assets: fmt_dec(immob),
        inventory: fmt_dec(inv),
        receivables: fmt_dec(recv),
        short_investments: fmt_dec(shinv),
        cash_bank: fmt_dec(cash),
        prepaid_expenses: fmt_dec(prepaid),
        total_assets: fmt_dec(total_assets),
        equity: fmt_dec(equity_total),
        current_result: fmt_dec(current_result),
        provisions: fmt_dec(provisions),
        long_term_debt: fmt_dec(ltd),
        current_liabilities: fmt_dec(curr_liab),
        deferred_revenue: fmt_dec(def_rev),
        total_equity_liabilities: fmt_dec(total_el),
        balanced,
        entity_size_note,
    }
}

/// Build the bilanț for a period from the GL trial balance.
pub async fn bilant(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<BilantReport> {
    let tb = trial_balance(pool, company_id, period_from, period_to).await?;
    Ok(compute_bilant(&tb.rows, period_to))
}

// ─── Registru-jurnal (cod 14-1-1) ────────────────────────────────────────────

/// One line of the Registru-jurnal (model 14-1-1): one GL entry, with the account on its
/// own side (debit or credit) and the sum.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalRegisterRow {
    pub nr_crt: i64,
    pub date: String,
    pub document: String,
    pub explanation: String,
    pub debit_account: String,
    pub credit_account: String,
    pub debit: String,
    pub credit: String,
}

/// Registru-jurnal for a period + footer totals (Σ debit must equal Σ credit).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalRegister {
    pub rows: Vec<JournalRegisterRow>,
    pub total_debit: String,
    pub total_credit: String,
    pub balanced: bool,
}

/// Registru-jurnal (model cod 14-1-1, OMFP 2634/2015): the chronological list of all GL
/// entries in the period, with the debit/credit account symbol and the sums. Mandatory
/// register (Legea 82/1991 art. 20); may be kept electronic, printed on demand.
pub async fn journal_register(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<JournalRegister> {
    let rows_q = sqlx::query(
        "SELECT j.transaction_date AS d, j.journal_id, j.transaction_id, j.description, \
                e.account_code, e.debit, e.credit \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date >= ?2 AND j.transaction_date <= ?3 \
         ORDER BY j.transaction_date, j.transaction_id, e.record_id",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    let mut rows = Vec::new();
    let mut total_d = Decimal::ZERO;
    let mut total_c = Decimal::ZERO;
    for (i, r) in rows_q.iter().enumerate() {
        let debit = dec(&r.try_get::<String, _>("debit").unwrap_or_default());
        let credit = dec(&r.try_get::<String, _>("credit").unwrap_or_default());
        let account: String = r.try_get("account_code").unwrap_or_default();
        let journal_id: String = r.try_get("journal_id").unwrap_or_default();
        let tx_id: String = r.try_get("transaction_id").unwrap_or_default();
        total_d += debit;
        total_c += credit;
        rows.push(JournalRegisterRow {
            nr_crt: (i + 1) as i64,
            date: r.try_get("d").unwrap_or_default(),
            document: format!("{journal_id} {tx_id}").trim().to_string(),
            explanation: r
                .try_get::<Option<String>, _>("description")
                .unwrap_or(None)
                .unwrap_or_default(),
            debit_account: if debit > Decimal::ZERO {
                account.clone()
            } else {
                String::new()
            },
            credit_account: if credit > Decimal::ZERO {
                account
            } else {
                String::new()
            },
            debit: fmt_dec(debit),
            credit: fmt_dec(credit),
        });
    }
    let balanced = (total_d - total_c).abs() < Decimal::new(1, 2);
    Ok(JournalRegister {
        rows,
        total_debit: fmt_dec(total_d),
        total_credit: fmt_dec(total_c),
        balanced,
    })
}

// ─── Cartea mare / fișă de cont (cod 14-1-3) ─────────────────────────────────

/// One movement line of an account's ledger sheet (fișă de cont), with the corresponding
/// account(s) and the running balance after the line.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub date: String,
    pub document: String,
    pub explanation: String,
    pub contra: String,
    pub debit: String,
    pub credit: String,
    pub balance: String,
    pub balance_side: String,
}

/// One synthetic account's ledger sheet (filă din Cartea mare).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerAccount {
    pub account_code: String,
    pub account_name: String,
    pub opening_debit: String,
    pub opening_credit: String,
    pub entries: Vec<LedgerEntry>,
    pub total_debit: String,
    pub total_credit: String,
    pub closing_debit: String,
    pub closing_credit: String,
}

/// Cartea mare (model cod 14-1-3 / fișă de cont pentru operațiuni diverse): one sheet per
/// synthetic account, with the opening balance, the period movements (with corespondent
/// account + running sold) and the closing balance. Mandatory register (Legea 82/1991).
pub async fn general_ledger(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<Vec<LedgerAccount>> {
    use std::collections::{BTreeMap, HashMap};

    // Account names from the chart.
    let name_rows = sqlx::query(
        "SELECT account_code, account_name FROM chart_of_accounts WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut names: HashMap<String, String> = HashMap::new();
    for r in &name_rows {
        let c: String = r.try_get("account_code").unwrap_or_default();
        let n: String = r.try_get("account_name").unwrap_or_default();
        names.insert(c, n);
    }

    // Opening balance per account (net debit−credit) before the period.
    let opening_rows = sqlx::query(
        "SELECT e.account_code, \
                COALESCE(SUM(CAST(e.debit AS REAL)-CAST(e.credit AS REAL)),0.0) AS net \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date < ?2 \
         GROUP BY e.account_code",
    )
    .bind(company_id)
    .bind(period_from)
    .fetch_all(pool)
    .await?;
    let mut opening: HashMap<String, Decimal> = HashMap::new();
    for r in &opening_rows {
        let c: String = r.try_get("account_code").unwrap_or_default();
        opening.insert(c, dec_f(r.try_get::<f64, _>("net").unwrap_or(0.0)));
    }

    // All period entries with their journal, to derive corespondent accounts per journal.
    let ent_rows = sqlx::query(
        "SELECT e.journal_pk, j.transaction_date AS d, j.journal_id, j.transaction_id, \
                j.description, e.account_code, e.debit, e.credit, e.record_id \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND j.transaction_date >= ?2 AND j.transaction_date <= ?3 \
         ORDER BY j.transaction_date, j.transaction_id, e.record_id",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    // Per journal: the debit-side and credit-side account sets (for corespondent display).
    let mut jrnl_debit: HashMap<String, Vec<String>> = HashMap::new();
    let mut jrnl_credit: HashMap<String, Vec<String>> = HashMap::new();
    for r in &ent_rows {
        let jpk: String = r.try_get("journal_pk").unwrap_or_default();
        let acc: String = r.try_get("account_code").unwrap_or_default();
        let d = dec(&r.try_get::<String, _>("debit").unwrap_or_default());
        let c = dec(&r.try_get::<String, _>("credit").unwrap_or_default());
        if d > Decimal::ZERO {
            jrnl_debit.entry(jpk).or_default().push(acc);
        } else if c > Decimal::ZERO {
            jrnl_credit.entry(jpk).or_default().push(acc);
        }
    }
    // Corespondent account(s) of the opposite side, excluding the line's own account and
    // de-duplicated: one distinct account → its symbol; several → "%" (operațiuni diverse).
    let contra = |opposite: Option<&Vec<String>>, own: &str| -> String {
        let mut uniq: Vec<&str> = Vec::new();
        if let Some(v) = opposite {
            for a in v {
                if a.as_str() != own && !uniq.contains(&a.as_str()) {
                    uniq.push(a.as_str());
                }
            }
        }
        match uniq.len() {
            0 => String::new(),
            1 => uniq[0].to_string(),
            _ => "%".to_string(),
        }
    };

    // Build per-account ledger sheets (ordered by account_code).
    let mut accounts: BTreeMap<String, LedgerAccount> = BTreeMap::new();
    // Seed accounts that have only an opening balance (no period movement).
    for (code, net) in &opening {
        accounts
            .entry(code.clone())
            .or_insert_with(|| LedgerAccount {
                account_code: code.clone(),
                account_name: names.get(code).cloned().unwrap_or_else(|| code.clone()),
                opening_debit: fmt_dec((*net).max(Decimal::ZERO)),
                opening_credit: fmt_dec((-*net).max(Decimal::ZERO)),
                entries: Vec::new(),
                total_debit: "0.00".into(),
                total_credit: "0.00".into(),
                closing_debit: "0.00".into(),
                closing_credit: "0.00".into(),
            });
    }

    // Running balances start from opening.
    let mut running: HashMap<String, Decimal> = opening.clone();
    let mut totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();

    for r in &ent_rows {
        let acc: String = r.try_get("account_code").unwrap_or_default();
        let jpk: String = r.try_get("journal_pk").unwrap_or_default();
        let debit = dec(&r.try_get::<String, _>("debit").unwrap_or_default());
        let credit = dec(&r.try_get::<String, _>("credit").unwrap_or_default());
        let journal_id: String = r.try_get("journal_id").unwrap_or_default();
        let tx_id: String = r.try_get("transaction_id").unwrap_or_default();

        let acct = accounts
            .entry(acc.clone())
            .or_insert_with(|| LedgerAccount {
                account_code: acc.clone(),
                account_name: names.get(&acc).cloned().unwrap_or_else(|| acc.clone()),
                opening_debit: "0.00".into(),
                opening_credit: "0.00".into(),
                entries: Vec::new(),
                total_debit: "0.00".into(),
                total_credit: "0.00".into(),
                closing_debit: "0.00".into(),
                closing_credit: "0.00".into(),
            });

        // Corespondent = the opposite side's account(s) of this journal.
        let contra_acc = if debit > Decimal::ZERO {
            contra(jrnl_credit.get(&jpk), &acc)
        } else {
            contra(jrnl_debit.get(&jpk), &acc)
        };

        let bal = running.entry(acc.clone()).or_insert(Decimal::ZERO);
        *bal += debit - credit;
        let side = if *bal >= Decimal::ZERO { "D" } else { "C" };

        acct.entries.push(LedgerEntry {
            date: r.try_get("d").unwrap_or_default(),
            document: format!("{journal_id} {tx_id}").trim().to_string(),
            explanation: r
                .try_get::<Option<String>, _>("description")
                .unwrap_or(None)
                .unwrap_or_default(),
            contra: contra_acc,
            debit: fmt_dec(debit),
            credit: fmt_dec(credit),
            balance: fmt_dec((*bal).abs()),
            balance_side: side.to_string(),
        });

        let t = totals.entry(acc).or_insert((Decimal::ZERO, Decimal::ZERO));
        t.0 += debit;
        t.1 += credit;
    }

    // Finalise per-account totals + closing.
    for (code, acct) in accounts.iter_mut() {
        let (td, tc) = totals
            .get(code)
            .copied()
            .unwrap_or((Decimal::ZERO, Decimal::ZERO));
        let open = opening.get(code).copied().unwrap_or(Decimal::ZERO);
        let closing = open + td - tc;
        acct.total_debit = fmt_dec(td);
        acct.total_credit = fmt_dec(tc);
        acct.closing_debit = fmt_dec(closing.max(Decimal::ZERO));
        acct.closing_credit = fmt_dec((-closing).max(Decimal::ZERO));
    }

    Ok(accounts.into_values().collect())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec as rdec;
    use sqlx::SqlitePool;

    fn tb_row(
        code: &str,
        name: &str,
        closing_debit: &str,
        closing_credit: &str,
    ) -> TrialBalanceRow {
        TrialBalanceRow {
            account_code: code.into(),
            account_name: name.into(),
            opening_debit: "0.00".into(),
            opening_credit: "0.00".into(),
            period_debit: "0.00".into(),
            period_credit: "0.00".into(),
            total_debit: "0.00".into(),
            total_credit: "0.00".into(),
            closing_debit: closing_debit.into(),
            closing_credit: closing_credit.into(),
        }
    }

    #[test]
    fn pnl_aggregates_revenue_expense_and_estimates_micro_tax() {
        // 707 revenue 10.000 (credit), 607 expense 6.000 (debit), 665 fin. expense 100, 765 fin.
        // revenue 50. Gross = (10.000+50) − (6.000+100) = 3.950.
        let rows = vec![
            tb_row("707", "Venituri mărfuri", "0.00", "10000.00"),
            tb_row("765", "Venituri din diferențe de curs", "0.00", "50.00"),
            tb_row("607", "Cheltuieli mărfuri", "6000.00", "0.00"),
            tb_row("665", "Cheltuieli din diferențe de curs", "100.00", "0.00"),
            tb_row("4111", "Clienți", "11900.00", "0.00"), // balance-sheet acct → ignored
        ];
        let pnl = compute_pnl(&rows, "micro", "2026-01-01", "2026-12-31");
        assert_eq!(pnl.total_revenue, "10050.00");
        assert_eq!(pnl.financial_revenue, "50.00");
        assert_eq!(pnl.total_expense, "6100.00");
        assert_eq!(pnl.financial_expense, "100.00");
        assert_eq!(pnl.gross_result, "3950.00");
        // micro tax estimate = 1% × revenue 10.050 = 100.50
        assert!(pnl.income_tax_estimated);
        assert_eq!(pnl.income_tax, "100.50");
        assert_eq!(pnl.net_result, "3849.50");
        // closing entries: D 707/765 / C 121 and D 121 / C 607/665 (4 lines, no balance-sheet acct).
        assert_eq!(pnl.closing_entries.len(), 4);
    }

    #[test]
    fn pnl_uses_booked_income_tax_when_present_and_profit_16pct() {
        // profit regime, with 691 already booked 320 → income_tax is the booked figure, not 16%.
        let rows = vec![
            tb_row("704", "Venituri servicii", "0.00", "5000.00"),
            tb_row("641", "Cheltuieli salarii", "3000.00", "0.00"),
            tb_row("691", "Cheltuieli impozit profit", "320.00", "0.00"),
        ];
        let pnl = compute_pnl(&rows, "profit", "2026-01-01", "2026-12-31");
        assert_eq!(pnl.gross_result, "2000.00"); // 5000 − 3000 (691 excluded from expense)
        assert!(!pnl.income_tax_estimated, "booked 691 used");
        assert_eq!(pnl.income_tax, "320.00");
        assert_eq!(pnl.net_result, "1680.00");
        // 691 is not in expense_lines (reported as income tax, not operating expense).
        assert!(!pnl.expense_lines.iter().any(|l| l.code == "691"));
    }

    #[tokio::test]
    async fn period_close_sweeps_67_to_121_and_is_idempotent() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;
        // Manual journal: a sale (D 4111 / C 707 = 10.000) + an expense (D 607 / C 401 = 6.000).
        let mut tx = pool.begin().await.unwrap();
        let j = GlJournal {
            id: new_id(),
            company_id: "co1".into(),
            journal_id: "DIVERSE".into(),
            journal_type: "MANUAL".into(),
            transaction_id: "M1".into(),
            transaction_date: "2026-03-15".into(),
            description: None,
            source_type: "MANUAL".into(),
            source_id: "m1".into(),
            customer_id: None,
            supplier_id: None,
        };
        let jpk = j.id.clone();
        insert_journal(&mut tx, &j).await.unwrap();
        let mk = |rec: i64, acc: &str, d: Decimal, c: Decimal| GlEntry {
            id: new_id(),
            record_id: rec,
            account_code: acc.into(),
            debit: d,
            credit: c,
            partner_cui: None,
            customer_id: None,
            supplier_id: None,
            tax_type: "000".into(),
            tax_code: "000000".into(),
            tax_percentage: None,
            tax_base: None,
            tax_amount: None,
        };
        for e in [
            mk(1, "4111", rdec!(10000), Decimal::ZERO),
            mk(2, "707", Decimal::ZERO, rdec!(10000)),
            mk(3, "607", rdec!(6000), Decimal::ZERO),
            mk(4, "401", Decimal::ZERO, rdec!(6000)),
        ] {
            insert_entry(&mut tx, &jpk, &e).await.unwrap();
        }
        tx.commit().await.unwrap();

        let r = post_period_close(&pool, "co1", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert!(r.posted);
        assert_eq!(r.total_revenue, "10000.00");
        assert_eq!(r.total_expense, "6000.00");
        assert_eq!(r.result, "4000.00");

        // After the close, 707/607 net to zero and 121 carries the result (4.000 credit = profit).
        let c121 = |tb: &TrialBalance| {
            tb.rows
                .iter()
                .find(|x| x.account_code == "121")
                .map(|x| x.closing_credit.clone())
        };
        let tb = trial_balance(&pool, "co1", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let r707 = tb.rows.iter().find(|x| x.account_code == "707").unwrap();
        assert_eq!(r707.closing_debit, "0.00");
        assert_eq!(r707.closing_credit, "0.00");
        assert_eq!(c121(&tb), Some("4000.00".into()));
        assert!(tb.balanced, "trial balance still balances after the close");

        // Idempotent: re-running does not double 121.
        let r2 = post_period_close(&pool, "co1", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(r2.result, "4000.00");
        let tb2 = trial_balance(&pool, "co1", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(
            c121(&tb2),
            Some("4000.00".into()),
            "idempotent — 121 not doubled"
        );
    }

    #[test]
    fn bilant_balances_assets_against_equity_and_liabilities() {
        let rows = vec![
            tb_row("2131", "Echipamente", "50000.00", "0.00"),
            tb_row("2813", "Amortizare echipamente", "0.00", "10000.00"), // contra → nets immob
            tb_row("371", "Mărfuri", "20000.00", "0.00"),
            tb_row("4111", "Clienți", "30000.00", "0.00"),
            tb_row("5121", "Bancă", "15000.00", "0.00"),
            tb_row("101", "Capital social", "0.00", "50000.00"),
            tb_row("401", "Furnizori", "0.00", "25000.00"),
            tb_row("162", "Credite bancare termen lung", "0.00", "20000.00"),
            tb_row("707", "Venituri mărfuri", "0.00", "10000.00"), // not yet closed
        ];
        let b = compute_bilant(&rows, "2026-12-31");
        assert_eq!(b.immobilized_assets, "40000.00"); // 50.000 − 10.000 amortizare
        assert_eq!(b.inventory, "20000.00");
        assert_eq!(b.receivables, "30000.00");
        assert_eq!(b.cash_bank, "15000.00");
        assert_eq!(b.total_assets, "105000.00");
        assert_eq!(b.current_result, "10000.00"); // 707 folded into equity even before the close
        assert_eq!(b.equity, "60000.00"); // 50.000 capital + 10.000 rezultat
        assert_eq!(b.current_liabilities, "25000.00");
        assert_eq!(b.long_term_debt, "20000.00");
        assert_eq!(b.total_equity_liabilities, "105000.00");
        assert!(b.balanced, "Active = Capitaluri + Datorii");
    }

    /// Insert a manual balanced journal (account, debit, credit lines) for the close tests.
    async fn manual_journal(
        pool: &SqlitePool,
        company_id: &str,
        date: &str,
        src: &str,
        lines: &[(&str, Decimal, Decimal)],
    ) {
        let mut tx = pool.begin().await.unwrap();
        let j = GlJournal {
            id: new_id(),
            company_id: company_id.into(),
            journal_id: "DIVERSE".into(),
            journal_type: "MANUAL".into(),
            transaction_id: src.into(),
            transaction_date: date.into(),
            description: None,
            source_type: "MANUAL".into(),
            source_id: src.into(),
            customer_id: None,
            supplier_id: None,
        };
        let jpk = j.id.clone();
        insert_journal(&mut tx, &j).await.unwrap();
        for (i, (acc, d, c)) in lines.iter().enumerate() {
            let e = GlEntry {
                id: new_id(),
                record_id: i as i64 + 1,
                account_code: acc.to_string(),
                debit: *d,
                credit: *c,
                partner_cui: None,
                customer_id: None,
                supplier_id: None,
                tax_type: "000".into(),
                tax_code: "000000".into(),
                tax_percentage: None,
                tax_base: None,
                tax_amount: None,
            };
            insert_entry(&mut tx, &jpk, &e).await.unwrap();
        }
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn income_tax_micro_then_close_then_annual_reset() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;
        // A sale (C 707 10.000 / D 4111) and an expense (D 607 6.000 / C 401).
        manual_journal(
            &pool,
            "co1",
            "2026-06-15",
            "s1",
            &[
                ("4111", rdec!(10000), Decimal::ZERO),
                ("707", Decimal::ZERO, rdec!(10000)),
            ],
        )
        .await;
        manual_journal(
            &pool,
            "co1",
            "2026-06-15",
            "e1",
            &[
                ("607", rdec!(6000), Decimal::ZERO),
                ("401", Decimal::ZERO, rdec!(6000)),
            ],
        )
        .await;

        // Income tax (micro, estimate) = 1% × venituri 10.000 = 100 → D 698 / C 4418.
        let t = post_income_tax(&pool, "co1", "micro", "2026-01-01", "2026-12-31", None)
            .await
            .unwrap();
        assert!(t.posted);
        assert_eq!(t.expense_account, "698");
        assert_eq!(t.amount, "100.00");
        assert!(t.estimated);

        // Close 6/7 → 121: sweeps 707 (10.000), 607 (6.000) AND 698 (100) → 121 credit = 3.900.
        let c = post_period_close(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        assert_eq!(c.result, "3900.00");
        let tb = trial_balance(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        let c121 = tb.rows.iter().find(|x| x.account_code == "121").unwrap();
        assert_eq!(c121.closing_credit, "3900.00");
        // 4418 (impozit pe venit de plată) carries the tax liability.
        let p4418 = tb.rows.iter().find(|x| x.account_code == "4418").unwrap();
        assert_eq!(p4418.closing_credit, "100.00");

        // Annual reset: 121 (3.900 credit) → D 121 / C 117. Idempotent.
        let a = post_annual_close(&pool, "co1", 2026).await.unwrap();
        assert!(a.posted);
        assert_eq!(a.kind, "profit");
        assert_eq!(a.result_121, "3900.00");
        assert_eq!(a.entry_date, "2027-01-01");
        let a2 = post_annual_close(&pool, "co1", 2026).await.unwrap();
        assert_eq!(a2.result_121, "3900.00", "idempotent");
        // After the reset, 117 holds the carried-forward profit.
        let tb2 = trial_balance(&pool, "co1", "2026-01-01", "2027-12-31")
            .await
            .unwrap();
        let c117 = tb2.rows.iter().find(|x| x.account_code == "117").unwrap();
        assert_eq!(c117.closing_credit, "3900.00");
    }

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
        // EUR cash hits the valută bank account 5124 (not the lei 5121). No payment-date rate
        // stored → cash converts at the invoice rate, so no FX leg and the legs match.
        let d5124 = get_entry_amount(&pool, &jpk, "5124", "debit").await;
        let d5121 = get_entry_amount(&pool, &jpk, "5121", "debit").await;
        let c4111 = get_entry_amount(&pool, &jpk, "4111", "credit").await;
        assert_eq!(d5124, rdec!(595), "D 5124 = 119 EUR × 5.0 = 595 RON");
        assert_eq!(d5121, rdec!(0), "lei bank untouched for an EUR receipt");
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
        // Realistic storno (mirrors commands::storno_invoice): the ORIGINAL is set to STORNED but
        // keeps its POSITIVE lines (the sale happened), and a SEPARATE credit note with
        // storno_of_invoice_id carries NEGATIVE lines (the reversal). FIX-1: no sign flip — the
        // stored amounts are already signed, matching D300.
        insert_invoice(
            &pool, cid, "inv5", "ct5", "STORNED", "1000", "190", "1190", None,
        )
        .await;
        insert_invoice(
            &pool,
            cid,
            "inv5s",
            "ct5",
            "VALIDATED",
            "-1000",
            "-190",
            "-1190",
            Some("inv5"),
        )
        .await;

        generate_gl_entries(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .expect("generate");

        // The STORNED original still posts the REAL (positive) sale — not a backwards reversal.
        let jpk_orig = get_journal_pk(&pool, "inv5").await;
        assert_eq!(
            get_entry_amount(&pool, &jpk_orig, "4111", "debit").await,
            rdec!(1190),
            "STORNED original keeps D 4111 = 1190"
        );
        assert_eq!(
            get_entry_amount(&pool, &jpk_orig, "707", "credit").await,
            rdec!(1000),
            "STORNED original keeps C 707 = 1000"
        );

        // The credit note (negative stored lines) posts the reversal: C 4111 / D 707.
        let jpk_storno = get_journal_pk(&pool, "inv5s").await;
        assert_eq!(
            get_entry_amount(&pool, &jpk_storno, "4111", "debit").await,
            Decimal::ZERO,
            "Storno: 4111 nu trebuie debit"
        );
        assert_eq!(
            get_entry_amount(&pool, &jpk_storno, "4111", "credit").await,
            rdec!(1190),
            "Storno: C 4111 = 1190 (stornare)"
        );
        assert_eq!(
            get_entry_amount(&pool, &jpk_storno, "707", "credit").await,
            Decimal::ZERO,
            "Storno: 707 nu trebuie credit"
        );
        assert_eq!(
            get_entry_amount(&pool, &jpk_storno, "707", "debit").await,
            rdec!(1000),
            "Storno: D 707 = 1000 (stornare)"
        );

        let (td, tc) = sum_entries(&pool, &jpk_storno).await;
        assert_eq!(td, tc, "Storno: dezechilibru GL");

        // GL ↔ D300 ties out: original +190 and credit note -190 net to 0 collected, in-period
        // AND on a re-run (no sign inversion of prior-period revenue — FIX-1 regression guard).
        let rec = reconcile(&pool, cid, "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(
            rec.discrepancies.is_empty(),
            "reconcile must be clean: {:?}",
            rec.discrepancies
        );
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

    // A STORNED cash-VAT original must NOT defer to 4428 (it will never be collected) — its VAT
    // stays exigible on 4427, the credit note reverses it, and GL ↔ D300 ties out. (Regression
    // guard for the FIX-1 cash-VAT follow-up.)
    #[tokio::test]
    async fn cash_vat_storno_does_not_strand_vat_in_4428_and_reconciles() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        enable_cash_vat(&pool, "co", "2025-01-01").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        insert_invoice(
            &pool, "co", "o", "ct", "STORNED", "1000", "190", "1190", None,
        )
        .await;
        insert_invoice(
            &pool,
            "co",
            "cn",
            "ct",
            "VALIDATED",
            "-1000",
            "-190",
            "-1190",
            Some("o"),
        )
        .await;
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        // Nothing stranded in 4428: the storned sale's VAT went to 4427, not deferred.
        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        assert_eq!(d4428, Decimal::ZERO);
        assert_eq!(
            c4428,
            Decimal::ZERO,
            "no S VAT stranded in 4428 for a storned sale"
        );
        // GL ↔ D300 ties: +190 (storned original) − 190 (credit note) = 0 collected.
        let rec = reconcile(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(
            rec.discrepancies.is_empty(),
            "cash-VAT storno must reconcile: {:?}",
            rec.discrepancies
        );
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

    // ── Buyer-side (slice 7d) — input VAT 4428 + release on supplier payment ──
    async fn insert_cash_vat_supplier(pool: &SqlitePool, company: &str, cui: &str) {
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name, cash_vat) \
             VALUES (?1,?2,'SUPPLIER',?3,'Furnizor TI',1)",
        )
        .bind(format!("sup-{cui}"))
        .bind(company)
        .bind(cui)
        .execute(pool)
        .await
        .expect("insert cash-vat supplier");
    }

    async fn insert_recv_pay(
        pool: &SqlitePool,
        company: &str,
        rid: &str,
        pid: &str,
        amount: &str,
        paid_at: &str,
    ) {
        sqlx::query(
            "INSERT INTO received_invoice_payments \
             (id, received_invoice_id, company_id, amount, currency, paid_at, method) \
             VALUES (?1,?2,?3,?4,'RON',?5,'transfer')",
        )
        .bind(pid)
        .bind(rid)
        .bind(company)
        .bind(amount)
        .bind(paid_at)
        .execute(pool)
        .await
        .expect("insert received payment");
    }

    #[tokio::test]
    async fn buyer_cash_vat_supplier_defers_input_to_4428_then_releases() {
        // Buyer not on cash VAT, but the supplier (CUI123, matched) is → input VAT parks in
        // 4428 at invoice, then transfers to 4426 on payment, clearing 4428.
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_cash_vat_supplier(&pool, "co", "CUI123").await;
        insert_received(&pool, "co", "ri", "S", "19", "1000", "190").await;
        insert_recv_pay(&pool, "co", "ri", "rp1", "1190", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        let (d4426, _) = account_balance(&pool, "co", "4426").await;
        assert_eq!(d4428 - c4428, Decimal::ZERO, "4428 cleared on payment");
        assert_eq!(d4426, dec("190"), "input VAT deductible after payment");
    }

    #[tokio::test]
    async fn non_cash_vat_purchase_uses_4426_at_invoice() {
        // No cash-VAT supplier, buyer not on cash VAT → input VAT deductible at invoice date.
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_received(&pool, "co", "ri", "S", "19", "1000", "190").await;
        insert_recv_pay(&pool, "co", "ri", "rp1", "1190", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4426, _) = account_balance(&pool, "co", "4426").await;
        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        assert_eq!(d4426, dec("190"), "input VAT deductible at invoice date");
        assert_eq!(
            d4428 + c4428,
            Decimal::ZERO,
            "no 4428 for a normal supplier"
        );
    }

    #[tokio::test]
    async fn buyer_mixed_s_and_reverse_charge_releases_full_s_vat() {
        // S line 1000/190 (deferred) + AE line 1000/190 self-assessed. The payable is 2190
        // (AE VAT not paid to the supplier). A full 2190 payment must release the WHOLE S VAT
        // (190) — the AE VAT must not inflate the denominator.
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_cash_vat_supplier(&pool, "co", "CUI123").await;
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              net_amount, vat_amount, currency, issue_date, xml_path, status, downloaded_at, created_at) \
             VALUES ('ri','co','dl','CUI123','Furnizor','2190','2000','190','RON','2025-01-15','x.xml','NEW',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES ('vlS','ri','19','S','1000','190'),('vlAE','ri','19','AE','1000','190')",
        )
        .execute(&pool)
        .await
        .unwrap();
        insert_recv_pay(&pool, "co", "ri", "rp1", "2190", "2025-01-20").await;

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        let (d4426, _) = account_balance(&pool, "co", "4426").await;
        assert_eq!(
            d4428 - c4428,
            Decimal::ZERO,
            "S 4428 fully cleared (AE VAT excluded from the payable denominator)"
        );
        // 4426 debit = AE auto-assessment (190 at invoice) + S release (190 at payment) = 380.
        assert_eq!(d4426, dec("380"));
    }

    #[tokio::test]
    async fn rejected_received_invoice_posts_no_deduction_or_release() {
        // A REJECTED received invoice contributes nothing to GL (matches D300 exclusion).
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_cash_vat_supplier(&pool, "co", "CUI123").await;
        insert_received(&pool, "co", "ri", "S", "19", "1000", "190").await;
        insert_recv_pay(&pool, "co", "ri", "rp1", "1190", "2025-01-20").await;
        sqlx::query("UPDATE received_invoices SET status='REJECTED' WHERE id='ri'")
            .execute(&pool)
            .await
            .unwrap();

        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d4426, _) = account_balance(&pool, "co", "4426").await;
        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        assert_eq!(d4426, Decimal::ZERO, "no deduction for a rejected invoice");
        assert_eq!(
            d4428 + c4428,
            Decimal::ZERO,
            "no 4428 movement for a rejected invoice"
        );
    }

    // ── VAT settlement / închiderea TVA (Phase 2.2) ──────────────────────────
    #[tokio::test]
    async fn vat_settlement_de_plata() {
        // Collected 190 (sale) > deductible 95 (purchase) → 4423 de plată 95; 4426/4427 zeroed.
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
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
        insert_received(&pool, "co", "ri", "S", "19", "500", "95").await;
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let r = post_vat_settlement(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(r.posted);
        assert_eq!(dec(&r.collected), dec("190"));
        assert_eq!(dec(&r.deductible), dec("95"));
        assert_eq!(dec(&r.de_plata), dec("95"));
        assert_eq!(dec(&r.de_recuperat), dec("0"));
        assert_eq!(r.entry_date, "2025-01-31");

        let (d4427, c4427) = account_balance(&pool, "co", "4427").await;
        let (d4426, c4426) = account_balance(&pool, "co", "4426").await;
        let (_, c4423) = account_balance(&pool, "co", "4423").await;
        assert_eq!(c4427 - d4427, Decimal::ZERO, "4427 closed to zero");
        assert_eq!(d4426 - c4426, Decimal::ZERO, "4426 closed to zero");
        assert_eq!(c4423, dec("95"), "TVA de plată on 4423");
    }

    #[tokio::test]
    async fn vat_settlement_de_recuperat() {
        // Collected 95 < deductible 190 → 4424 de recuperat 95.
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        insert_invoice(
            &pool,
            "co",
            "inv",
            "ct",
            "VALIDATED",
            "500",
            "95",
            "595",
            None,
        )
        .await;
        insert_received(&pool, "co", "ri", "S", "19", "1000", "190").await;
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let r = post_vat_settlement(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert_eq!(dec(&r.de_recuperat), dec("95"));
        assert_eq!(dec(&r.de_plata), dec("0"));
        let (d4424, _) = account_balance(&pool, "co", "4424").await;
        assert_eq!(d4424, dec("95"), "TVA de recuperat on 4424");
    }

    #[tokio::test]
    async fn vat_settlement_idempotent() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
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
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        post_vat_settlement(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        post_vat_settlement(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        // Re-running replaces, not duplicates → 4423 credit is 190 once, not 380.
        let (_, c4423) = account_balance(&pool, "co", "4423").await;
        assert_eq!(c4423, dec("190"));
    }

    #[tokio::test]
    async fn vat_settlement_excludes_4428_neexigibila() {
        // Cash-VAT sale never collected → output VAT sits in 4428. The close must NOT touch it
        // (nothing exigible to settle).
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
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let r = post_vat_settlement(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(!r.posted, "nothing exigible to close");
        assert_eq!(dec(&r.collected), dec("0"));
        // The 4428 balance is untouched (still credit 190).
        let (d4428, c4428) = account_balance(&pool, "co", "4428").await;
        assert_eq!(c4428 - d4428, dec("190"), "4428 neexigibilă left intact");
        // No 4423/4424 movement.
        let (_, c4423) = account_balance(&pool, "co", "4423").await;
        assert_eq!(c4423, Decimal::ZERO);
    }

    // ── Balanța de verificare (Phase 2.4) ────────────────────────────────────
    #[tokio::test]
    async fn trial_balance_satisfies_four_equalities() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
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

        let tb = trial_balance(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(tb.balanced, "the four egalități must hold");
        assert_eq!(tb.total_period_debit, tb.total_period_credit);
        assert_eq!(tb.total_closing_debit, tb.total_closing_credit);

        let find = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .expect("account row present")
        };
        // Sale D 4111/C 707/C 4427 + receipt D 5121/C 4111.
        assert_eq!(dec(&find("5121").closing_debit), dec("1190"));
        assert_eq!(dec(&find("707").closing_credit), dec("1000"));
        assert_eq!(dec(&find("4427").closing_credit), dec("190"));
        // 4111 fully settled within the period → no closing balance, but rulaj shows the flow.
        assert_eq!(dec(&find("4111").closing_debit), dec("0"));
        assert_eq!(dec(&find("4111").period_debit), dec("1190"));
        assert_eq!(dec(&find("4111").period_credit), dec("1190"));
        // A settled account must render canonical "0.00", never "-0.00".
        assert_eq!(find("4111").closing_credit, "0.00");
        assert_eq!(find("4111").closing_debit, "0.00");
    }

    // ── Registru-jurnal + Cartea mare (Phase 2.4) ────────────────────────────
    #[tokio::test]
    async fn journal_register_is_chronological_and_balanced() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
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

        let jr = journal_register(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert!(jr.balanced, "Σ debit = Σ credit");
        assert_eq!(jr.total_debit, jr.total_credit);
        // nr_crt is sequential from 1.
        assert_eq!(jr.rows.first().unwrap().nr_crt, 1);
        assert_eq!(
            jr.rows.last().unwrap().nr_crt,
            jr.rows.len() as i64,
            "nr_crt continuous"
        );
        // Each line has exactly one side populated.
        for row in &jr.rows {
            let has_d = !row.debit_account.is_empty();
            let has_c = !row.credit_account.is_empty();
            assert!(
                has_d ^ has_c,
                "exactly one of debit/credit account per line"
            );
        }
    }

    #[tokio::test]
    async fn general_ledger_sheets_have_running_balance() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
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

        let gl = general_ledger(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        let acc = |code: &str| {
            gl.iter()
                .find(|a| a.account_code == code)
                .expect("account sheet")
        };

        // 4111: debit 1190 (sale) then credit 1190 (receipt) → closing zero, 2 movements.
        let a4111 = acc("4111");
        assert_eq!(a4111.entries.len(), 2);
        assert_eq!(dec(&a4111.total_debit), dec("1190"));
        assert_eq!(dec(&a4111.total_credit), dec("1190"));
        assert_eq!(dec(&a4111.closing_debit), dec("0"));
        assert_eq!(dec(&a4111.closing_credit), dec("0"));
        // 5121: single receipt → closing debit 1190, corespondent 4111.
        let a5121 = acc("5121");
        assert_eq!(dec(&a5121.closing_debit), dec("1190"));
        assert_eq!(a5121.entries[0].contra, "4111");
        assert_eq!(a5121.entries[0].balance_side, "D");
    }

    // ── Revenue split 701/704/707/709 (Phase 2.3) ────────────────────────────
    #[tokio::test]
    async fn revenue_split_routes_service_to_704_and_reduction_to_709() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number, \
             issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, status, \
             payment_means_code, created_at, updated_at) \
             VALUES ('inv','co','ct','inv',1,'inv','2025-01-15','2025-02-15','RON','900','171','1071','VALIDATED','42',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Service line 1000/190 (→704) + a granted reduction -100/-19 (→709).
        sqlx::query(
            "INSERT INTO invoice_line_items (id, invoice_id, position, name, quantity, unit, \
             unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, revenue_kind) \
             VALUES ('l1','inv','1','Consultanță','1','buc','1000','19','S','1000','190','1190','service'), \
                    ('l2','inv','2','Discount','1','buc','-100','19','S','-100','-19','-119','reduction')",
        )
        .execute(&pool)
        .await
        .unwrap();
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (_, c704) = account_balance(&pool, "co", "704").await;
        let (d709, _) = account_balance(&pool, "co", "709").await;
        let (_, c707) = account_balance(&pool, "co", "707").await;
        assert_eq!(c704, dec("1000"), "service revenue → 704");
        assert_eq!(d709, dec("100"), "granted reduction → 709 (debit)");
        assert_eq!(c707, dec("0"), "nothing on 707 (no goods line)");

        // reconcile must net the reduction's 4427 debit: GL collected = 190 − 19 = 171 = D300.
        let rec = reconcile(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();
        assert_eq!(dec(&rec.vat_collected_gl), dec("171"));
        assert_eq!(dec(&rec.vat_collected_d300), dec("171"));
        assert!(
            !rec.discrepancies.iter().any(|d| d.contains("colectat")),
            "no false TVA-colectată discrepancy on a VAT-bearing reduction"
        );
    }

    // ── FX gain/loss 665/765 at settlement (Phase 2.3) ───────────────────────
    #[tokio::test]
    async fn fx_gain_on_foreign_currency_receipt() {
        let pool = setup_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "co", "ct", "CUI999").await;
        // EUR invoice at rate 5.0: 100 EUR exempt (Z) → 4111 = 500 RON.
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number, \
             issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, \
             total_amount, status, payment_means_code, created_at, updated_at) \
             VALUES ('inv','co','ct','inv',1,'inv','2025-01-10','2025-02-10','EUR',5.0,'100','0','100','VALIDATED','42',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoice_line_items (id, invoice_id, position, name, quantity, unit, \
             unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES ('l','inv','1','Export','1','buc','100','0','Z','100','0','100')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Collect 100 EUR at rate 5.1 → cash 510 RON; receivable was 500 → FX gain 10.
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method, exchange_rate) \
             VALUES ('p1','inv','co','100','EUR','2025-01-20','transfer',5.1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        generate_gl_entries(&pool, "co", "2025-01-01", "2025-01-31")
            .await
            .unwrap();

        let (d5124, _) = account_balance(&pool, "co", "5124").await;
        let (_, c765) = account_balance(&pool, "co", "765").await;
        let (d4111, c4111) = account_balance(&pool, "co", "4111").await;
        assert_eq!(
            d5124,
            dec("510"),
            "EUR cash booked at the payment rate on 5124"
        );
        assert_eq!(c765, dec("10"), "favourable FX diff → 765");
        assert_eq!(
            d4111 - c4111,
            Decimal::ZERO,
            "receivable fully relieved at invoice rate"
        );
    }
}
