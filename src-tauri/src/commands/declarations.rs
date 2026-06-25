//! Declarații fiscale — D300 Decont TVA (vânzări + achiziții).
//!
//! D300 este decontul de TVA lunar/trimestrial depus la ANAF.
//! Această implementare acoperă:
//! - **Vânzări** (TVA colectată): din facturile cu status VALIDATED sau STORNED.
//!   Setul fiscal autorizat este `status IN ('VALIDATED','STORNED')` — identic cu
//!   rapoartele TVA, jurnalele, D394 și SAF-T, pentru reconciliere completă.
//!   Facturile STORNED sunt originalele ștornate (efectul lor fiscal rămâne în
//!   perioada emiterii); nota de credit negativă are status VALIDATED.
//! - **Achiziții** (TVA deductibilă): din received_invoice_vat_lines (Wave B).
//!   Facturile primite fără defalcare TVA (net_amount IS NULL) sunt raportate
//!   separat prin `purchase_unparsed_count`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::d100::{compute_d100 as compute_d100_fn, D100Input, D100Result};
use crate::anaf_decl::d101::{compute_d101 as compute_d101_fn, D101Input, D101Result};
use crate::anaf_decl::d112::{compute_payroll as compute_payroll_fn, PayrollInput, PayrollResult};
use crate::anaf_decl::d300::D300Submission;
use crate::anaf_decl::version::resolve;
use crate::anaf_decl::xml_esc as xml_escape;
use crate::anaf_decl::DeclKind;
use crate::db::companies;

/// Calcul salariu (nucleul D112): brut → net + contribuții + cost angajator, cu ratele 2026.
#[tauri::command]
pub async fn compute_payroll(input: PayrollInput) -> AppResult<PayrollResult> {
    Ok(compute_payroll_fn(&input))
}

/// Rezultatul comenzii D100 = rândul de obligație (micro/profit) + obligațiile INFORMATIVE de impozit pe
/// dividende cu scadența în trimestru. `#[serde(flatten)]` păstrează câmpurile `D100Result` la nivel
/// superior (compatibil cu frontend-ul existent) și adaugă `dividendObligations`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D100CommandResult {
    #[serde(flatten)]
    pub d100: D100Result,
    /// Obligații de impozit pe dividende cu scadența în trimestrul afișat (informativ — D100 nu emite
    /// XML; validarea ANAF se face prin PDF inteligent + SPV). Gol dacă nu există dividende cu scadența
    /// în perioadă.
    pub dividend_obligations: Vec<crate::db::dividends::DividendObligation>,
}

/// D100 (obligații de plată) — rândul trimestrial: micro poziția 5 (1% × venituri) sau profit
/// poziția 2 (16% × rezultat), din P&L-ul perioadei; suma de plată = datorată − plăți anterioare.
/// Returnează ȘI obligațiile de impozit pe dividende cu scadența în trimestru (informativ — vezi
/// `D100CommandResult`).
#[tauri::command]
pub async fn compute_d100(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    quarter: u32,
    year: i32,
    prior_payments: String,
) -> AppResult<D100CommandResult> {
    let company = companies::get(&state.db, &company_id).await?;
    let pnl = crate::db::gl::profit_and_loss(
        &state.db,
        &company_id,
        &company.tax_regime,
        &period_from,
        &period_to,
    )
    .await?;
    // `prior_payments` is user-supplied (free-text). A silent `.unwrap_or(ZERO)` would zero an
    // unparseable value (e.g. a ro-RO "1.234,56"), understating prior payments → an overstated
    // "sumă de plată". Reject it so the user corrects the input instead of filing a wrong figure.
    // (revenue/result come from profit_and_loss → fmt_dec, a canonical Decimal literal that always
    // re-parses, so their `.parse()` cannot fail.)
    let prior = {
        let raw = prior_payments.trim();
        if raw.is_empty() {
            Decimal::ZERO
        } else {
            let d = Decimal::from_str(raw).map_err(|_| {
                AppError::Validation(
                    "Plăți anterioare invalide — folosiți formatul 1234.56.".into(),
                )
            })?;
            // Negative prior payments would ADD to the tax due (compute_d100_fn clamps only the
            // final result, not this input) — reject, symmetric with the assets cost guard.
            if d.is_sign_negative() {
                return Err(AppError::Validation(
                    "Plățile anterioare nu pot fi negative.".into(),
                ));
            }
            d
        }
    };
    let input = D100Input {
        quarter,
        year,
        revenue: pnl.total_revenue.parse().unwrap_or(Decimal::ZERO),
        result: pnl.gross_result.parse().unwrap_or(Decimal::ZERO),
        prior_payments: prior,
    };
    let d100 = compute_d100_fn(&company.tax_regime, &input);

    // Obligațiile INFORMATIVE de impozit pe dividende cu scadența în trimestrul afișat. D100 nu emite
    // XML, dar impozitul pe dividende SE declară prin D100 (lunar, 25 a lunii următoare plății), așa că
    // semnalăm contribuabilului obligațiile scadente în cele 3 luni ale trimestrului. `clamp(1,4)` evită
    // un underflow pe `quarter-1` dacă vine 0 (frontend-ul derivă quarter din dată, deci 1-4).
    let q = quarter.clamp(1, 4);
    let months: Vec<String> = (1..=3)
        .map(|i| format!("{year:04}-{:02}", (q - 1) * 3 + i))
        .collect();
    let dividend_obligations =
        crate::db::dividends::dividend_obligations_in_months(&state.db, &company_id, &months)
            .await?;

    Ok(D100CommandResult {
        d100,
        dividend_obligations,
    })
}
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

/// D101 (impozit pe profit anual) worksheet: takes the base (rezultat brut + cifră de afaceri) from
/// the period P&L and applies the caller-supplied fiscal adjustments (art. 19 Cod fiscal).
#[tauri::command]
pub async fn compute_d101(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    input: D101Input,
) -> AppResult<D101Result> {
    let company = companies::get(&state.db, &company_id).await?;
    let pnl = crate::db::gl::profit_and_loss(
        &state.db,
        &company_id,
        &company.tax_regime,
        &period_from,
        &period_to,
    )
    .await?;
    let mut input = input;
    input.accounting_result = pnl.gross_result.parse().unwrap_or(Decimal::ZERO);
    input.turnover = pnl.operating_revenue.parse().unwrap_or(Decimal::ZERO);
    Ok(compute_d101_fn(&input))
}

// ── DUK gate types ────────────────────────────────────────────────────────────

/// Result of an official export attempt with the DUK gate.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialExportResult {
    /// The written file path, or empty if blocked by DUK.
    pub path: String,
    pub written: bool,
    /// Whether a DUK runtime was available to validate.
    pub duk_available: bool,
    /// Whether DUK reported clean (only meaningful when duk_available).
    pub duk_passed: bool,
    pub issues: Vec<crate::anaf_decl::preflight::PreflightIssue>,
}

/// DRY gate decision: returns true (write allowed) unless DUK is available,
/// reported failure, and the user has NOT requested an override.
pub fn duk_gate_allows_write(available: bool, passed: bool, override_: bool) -> bool {
    !(available && !passed && !override_)
}

/// Rezultatul re-validării unui șir XML cu DUK (din editorul XML din aplicație).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlDukValidation {
    /// A fost disponibil validatorul (jar + runtime Java)? Dacă nu, nu blocăm editarea.
    pub available: bool,
    /// A trecut validarea (relevant doar când `available`).
    pub passed: bool,
    pub issues: Vec<crate::anaf_decl::preflight::PreflightIssue>,
}

/// Validează un șir XML ARBITRAR cu validatorul OFICIAL ANAF (DUK) pentru tipul declarației — folosit
/// de editorul XML din aplicație („re-validează cu DUK"). Acceptă D300/D394/D406/D112/D205. Scrie XML-ul
/// într-un fișier temporar, rulează `run_duk` (`-v <TIP>`) și întoarce verdictul + erorile. Dacă jar-ul
/// validatorului lipsește din build (ex. D205 pe unele build-uri) sau runtime-ul Java nu e disponibil,
/// întoarce `available=false` (NU eroare), ca să nu blocheze vizualizarea/editarea XML-ului.
#[tauri::command]
pub async fn validate_declaration_xml(
    app: tauri::AppHandle,
    decl_kind: String,
    xml: String,
) -> AppResult<XmlDukValidation> {
    use crate::anaf_decl::DeclKind;
    let kind = match decl_kind.as_str() {
        "D300" => DeclKind::D300,
        "D394" => DeclKind::D394,
        "D406" => DeclKind::D406,
        "D112" => DeclKind::D112,
        "D205" => DeclKind::D205,
        "D301" => DeclKind::D301,
        "D700" => DeclKind::D700,
        "D710" => DeclKind::D710,
        "D100" => DeclKind::D100,
        "D101" => DeclKind::D101,
        other => {
            return Err(AppError::Validation(format!(
                "Tip de declarație necunoscut pentru validare DUK: {other}"
            )))
        }
    };
    // Jar-ul validatorului poate lipsi (ex. D205 pe unele build-uri) — verificăm înainte, fiindcă
    // `run_duk` ar întoarce o eroare „validator neinstalat" în loc de un verdict. Folosim aceeași
    // rezolvare ca `BundledProvider` (Tauri păstrează prefixul `resources/` în pachet).
    let jar = {
        use tauri::Manager;
        let root =
            crate::anaf_decl::duk::bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        root.join(format!("duk/lib/{}Validator.jar", kind.as_duk_type()))
    };
    if !jar.is_file() {
        return Ok(XmlDukValidation {
            available: false,
            passed: false,
            issues: Vec::new(),
        });
    }
    let tmp = std::env::temp_dir().join(format!("duk_revalidate_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp XML: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let outcome = crate::anaf_decl::duk::run_duk(&provider, kind, &tmp);
    let _ = std::fs::remove_file(&tmp);
    Ok(match outcome? {
        Some(o) => XmlDukValidation {
            available: true,
            passed: o.passed,
            issues: o.errors,
        },
        None => XmlDukValidation {
            available: false,
            passed: false,
            issues: Vec::new(),
        },
    })
}

// ── Structs ───────────────────────────────────────────────────────────────────

/// Un grup de TVA colectat (cotă + categorie).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D300Group {
    /// Cota TVA (e.g. "19.00", "9.00", "5.00", "0.00").
    pub vat_rate: String,
    /// Categoria TVA (BIZ-12: "S", "Z", "E", "AE", "K", "G", "O").
    pub vat_category: String,
    /// Baza impozabilă (subtotal net), aranjată cu 2 zecimale.
    pub base: String,
    /// TVA colectat, aranjat cu 2 zecimale.
    pub vat: String,
    /// Tipul achiziției intra-UE (numai pentru category="K"): "goods" sau "services".
    /// None pentru orice alt grup. Determină rândul D300: goods→R5/R18, services→R7/R20.
    pub intra_eu_kind: Option<String>,
}

/// Raportul D300 — TVA colectat (vânzări) + TVA deductibil (achiziții).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D300Report {
    /// CUI-ul companiei emitente.
    pub company_cui: String,
    /// Data de început a perioadei (YYYY-MM-DD).
    pub period_from: String,
    /// Data de sfârșit a perioadei (YYYY-MM-DD).
    pub period_to: String,
    /// Grupuri TVA colectat sortate descrescător după cotă.
    pub groups: Vec<D300Group>,
    /// Total baze impozabile (RON), 2 zecimale.
    pub total_base: String,
    /// Total TVA colectat (RON), 2 zecimale.
    pub total_vat: String,
    /// Numărul de facturi VALIDATED + STORNED incluse (setul fiscal autorizat).
    pub invoice_count: i64,
    // ── Wave B: achiziții ────────────────────────────────────────────────────
    /// Grupuri TVA deductibil (achiziții), din received_invoice_vat_lines.
    pub purchase_groups: Vec<D300Group>,
    /// Total baze impozabile achiziții (RON), 2 zecimale.
    pub total_deductible_base: String,
    /// Total TVA deductibil (RON), 2 zecimale.
    pub total_deductible_vat: String,
    /// Numărul de facturi primite (status != REJECTED) în perioadă.
    pub purchase_invoice_count: i64,
    /// Facturi primite fără defalcare TVA (net_amount IS NULL) — date parțiale.
    pub purchase_unparsed_count: i64,
    /// TVA netă de plată = TVA colectată − TVA deductibilă (negativă = de recuperat).
    pub net_vat: String,

    // ── Wave 8: regularizări cote vechi ──────────────────────────────────────
    /// Auto-computed Σ baza din vânzările S la cote vechi 19%/5% → R16_1.
    pub reg_colectata_baza: String,
    /// Auto-computed Σ TVA din vânzările S la cote vechi 19%/5% → R16_2.
    pub reg_colectata_tva: String,
    /// Auto-computed Σ baza din achizițiile S la cote vechi 19%/9%/5% → R30_1.
    pub reg_dedusa_baza: String,
    /// Auto-computed Σ TVA din achizițiile S la cote vechi 19%/9%/5% → R30_2.
    pub reg_dedusa_tva: String,

    /// Informational TVA-neexigibilă memo balances (D300 rows A/A1/B/B1), in lei.
    #[serde(default)]
    pub cash_vat_memo: CashVatMemo,
}

/// Closing 4428 (TVA neexigibilă) balances for the informational D300 memo rows A/A1/B/B1, in lei.
/// A = output VAT not yet chargeable (cash-VAT sales, art. 282); B = input VAT not yet deducted
/// (art. 297(2)/(3)); A1/B1 = the slice from the reporting period + the prior 5 months.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashVatMemo {
    pub a_base: i64,
    pub a_vat: i64,
    pub a1_base: i64,
    pub a1_vat: i64,
    pub b_base: i64,
    pub b_vat: i64,
    pub b1_base: i64,
    pub b1_vat: i64,
}

// ── Commands ──────────────────────────────────────────────────────────────────

// ── Shared D300 VAT core (used by compute_d300 + gl::reconcile) ──────────────

/// Convert a RON `Decimal` to integer bani (round half-away-from-zero, matching
/// `cash_vat::round_div`). Inputs reaching this are already ≤2 decimals (post `amount_to_ron`),
/// so the strategy only matters as a guard against a future 3-decimal caller.
fn ron_to_bani(d: Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::RoundingStrategy;
    (d * Decimal::from(100))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

/// Fetch a company's cash-VAT regime flag + effective window (`cash_vat`, start, end).
/// Missing company → `(false, None, None)`.
async fn fetch_cash_vat_flags(
    pool: &sqlx::SqlitePool,
    company_id: &str,
) -> AppResult<(bool, Option<String>, Option<String>)> {
    let row = sqlx::query(
        "SELECT cash_vat, cash_vat_start, cash_vat_end FROM companies WHERE id = ?1 LIMIT 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(match row {
        Some(r) => (
            r.try_get::<bool, _>("cash_vat").unwrap_or(false),
            r.try_get::<Option<String>, _>("cash_vat_start")
                .unwrap_or(None),
            r.try_get::<Option<String>, _>("cash_vat_end")
                .unwrap_or(None),
        ),
        None => (false, None, None),
    })
}

/// Cash-VAT (TVA la încasare) collected groups: for a company on the regime, the deferred
/// standard-rate ("S") output VAT attributed to the **collection** period instead of the
/// issue date (Cod fiscal art. 282 alin. (3)-(5); OPANAF 174/2026 F300 rd.9/10/11, old rate
/// → rd.16). For each payment with `paid_at` in `[period_from, period_to]` against a deferred
/// sales invoice (status VALIDATED/STORNED, issued within the regime window, with ≥1 "S"
/// line), releases base+VAT proportionally — cumulative across ALL the invoice's payments so
/// prior-period partials never re-release — grouped by the line's ORIGINAL rate (so old-rate
/// collections still reach the rd.16 regularizări via the existing prefill loop). Excluded
/// categories and out-of-window "S" lines are NOT here; they keep the issue-date path.
///
/// Returns map `(rate_key, "S")` → `(rate, base_ron, vat_ron)`. Credit notes / storno
/// (non-positive gross) are inert here — proportional reversal is slice 5 (GL).
async fn cash_vat_collected_groups(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    cash_vat_start: Option<&str>,
    cash_vat_end: Option<&str>,
    period_from: &str,
    period_to: &str,
) -> AppResult<BTreeMap<(i64, String), (Decimal, Decimal, Decimal)>> {
    use crate::anaf_decl::cash_vat::{allocate_collection, RateBucket};
    use rust_decimal::prelude::ToPrimitive;

    let mut out: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

    // Invoices with a collection in [period], status ok, issued within the regime window.
    let invoice_rows = sqlx::query(
        "SELECT DISTINCT i.id, COALESCE(i.currency,'RON') AS currency, i.exchange_rate \
         FROM payments p JOIN invoices i ON i.id = p.invoice_id \
         WHERE p.company_id = ?1 \
           AND substr(p.paid_at,1,10) >= ?2 AND substr(p.paid_at,1,10) <= ?3 \
           AND i.status = 'VALIDATED' \
           AND CAST(i.total_amount AS REAL) > 0 \
           AND (?4 IS NULL OR i.issue_date >= ?4) \
           AND (?5 IS NULL OR i.issue_date <= ?5)",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .bind(cash_vat_start)
    .bind(cash_vat_end)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    for inv in &invoice_rows {
        let invoice_id: String = inv.try_get("id").unwrap_or_default();
        let currency: String = inv
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            inv.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        // Build the invoice's "S" rate buckets + full gross (ALL categories) in RON bani.
        let line_rows = sqlx::query(
            "SELECT vat_category, vat_rate, subtotal_amount, vat_amount \
             FROM invoice_line_items WHERE invoice_id = ?1",
        )
        .bind(&invoice_id)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

        let mut gross_bani: i64 = 0;
        // rate_key → (rate, base_bani, vat_bani)
        let mut bucket_acc: BTreeMap<i64, (Decimal, i64, i64)> = BTreeMap::new();
        for l in &line_rows {
            let category: String = l
                .try_get("vat_category")
                .unwrap_or_else(|_| "S".to_string());
            let rate_s: String = l.try_get("vat_rate").unwrap_or_default();
            let base_s: String = l.try_get("subtotal_amount").unwrap_or_default();
            let vat_s: String = l.try_get("vat_amount").unwrap_or_default();

            let base_ron = amount_to_ron(
                Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            );
            let vat_ron = amount_to_ron(
                Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            );
            // Denominator = the collectible/payable; reverse-charge (AE) / intra-EU (K) VAT is
            // self-assessed, not settled with the partner, so it is excluded (no-op when those
            // lines carry VAT=0, as self-issued sales do).
            let is_reverse_charge = matches!(category.trim(), "AE" | "K");
            gross_bani += ron_to_bani(base_ron);
            if !is_reverse_charge {
                gross_bani += ron_to_bani(vat_ron);
            }

            if category.trim() == "S" {
                let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
                let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
                let e = bucket_acc.entry(rate_key).or_insert((rate, 0, 0));
                e.1 += ron_to_bani(base_ron);
                e.2 += ron_to_bani(vat_ron);
            }
        }

        // No deferred "S" lines, or a credit note / storno (non-positive gross): skip.
        if bucket_acc.is_empty() || gross_bani <= 0 {
            continue;
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

        // Walk ALL payments up to period end chronologically; release only the in-period ones.
        let pay_rows = sqlx::query(
            "SELECT amount, paid_at \
             FROM payments WHERE invoice_id = ?1 AND substr(paid_at,1,10) <= ?2 \
             ORDER BY paid_at, id",
        )
        .bind(&invoice_id)
        .bind(period_to)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

        let mut paid_before: i64 = 0;
        for p in &pay_rows {
            let amt_s: String = p.try_get("amount").unwrap_or_default();
            let paid_at: String = p.try_get("paid_at").unwrap_or_default();
            // Payments settle in the invoice's currency (db/payments.rs defaults to it);
            // convert with the invoice currency + rate so the collected/gross ratio holds.
            let pay_bani = ron_to_bani(amount_to_ron(
                Decimal::from_str(&amt_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            ));
            if pay_bani <= 0 {
                continue;
            }
            // Date-only comparison (defensive against a time component on paid_at).
            let pd = if paid_at.len() >= 10 {
                &paid_at[..10]
            } else {
                paid_at.as_str()
            };
            if pd >= period_from && pd <= period_to {
                for rb in allocate_collection(gross_bani, &buckets, paid_before, pay_bani) {
                    let rate = *rate_of.get(&rb.rate_key).unwrap_or(&Decimal::ZERO);
                    let e = out.entry((rb.rate_key, "S".to_string())).or_insert((
                        rate,
                        Decimal::ZERO,
                        Decimal::ZERO,
                    ));
                    e.1 += Decimal::from(rb.base_bani) / Decimal::from(100);
                    e.2 += Decimal::from(rb.vat_bani) / Decimal::from(100);
                }
            }
            paid_before += pay_bani;
        }
    }

    Ok(out)
}

/// Cash-VAT STORNO collected-VAT correction for `[period_from, period_to]`, per rate_key → Σ
/// `amount_to_4428`. The issue-date collected query reverses a credit note's FULL "S" VAT (−R), but
/// under art. 282 alin. (10) only the already-collected part (`amount_to_4427`) may reduce collected
/// VAT — the still-deferred part (`amount_to_4428`, never reported as collected) must not. Callers
/// ADD this back, leaving each cross-period cash-VAT storno's net collected contribution at
/// −`amount_to_4427`. Uses the SAME `cash_vat_storno_split` as the GL postings ⇒ D300 collected ties
/// to GL net-4427 on reconcile. Empty for same-period / non-deferred storni (issue-date path already
/// correct there). Only cross-period cash-VAT storni of still-deferred originals contribute.
async fn cash_vat_storno_collected_correction(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<BTreeMap<i64, Decimal>> {
    let storni = sqlx::query(
        "SELECT id, storno_of_invoice_id, COALESCE(currency,'RON') AS currency, exchange_rate, \
                issue_date \
         FROM invoices \
         WHERE company_id = ?1 AND storno_of_invoice_id IS NOT NULL \
           AND status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?2 AND issue_date <= ?3 \
           AND CAST(total_amount AS REAL) < 0",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let mut out: BTreeMap<i64, Decimal> = BTreeMap::new();
    for s in &storni {
        let sid: String = s.try_get("id").unwrap_or_default();
        let oid: String = match s.try_get::<Option<String>, _>("storno_of_invoice_id") {
            Ok(Some(v)) => v,
            _ => continue,
        };
        let sissue: String = s.try_get("issue_date").unwrap_or_default();
        let sc: String = s.try_get("currency").unwrap_or_else(|_| "RON".to_string());
        let sfx = parse_rate(s.try_get::<Option<f64>, _>("exchange_rate").unwrap_or(None));
        let split = crate::db::gl::cash_vat_storno_split(
            pool,
            company_id,
            &sid,
            &sissue,
            &sc,
            sfx,
            &oid,
            period_from,
            period_to,
        )
        .await?;
        for (k, (_to4427, to4428)) in &split {
            if !to4428.is_zero() {
                *out.entry(*k).or_insert(Decimal::ZERO) += *to4428;
            }
        }
    }
    Ok(out)
}

/// True when buyer-side cash-VAT routing could change anything for this company: either the
/// buyer itself applies cash VAT, or it has at least one supplier (contact) flagged as
/// applying cash VAT (RPATVAÎ). When false, the deductible side is byte-identical to before.
async fn buyer_side_active(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    buyer_cash_vat: bool,
) -> AppResult<bool> {
    if buyer_cash_vat {
        return Ok(true);
    }
    let has: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM contacts WHERE company_id = ?1 AND cash_vat = 1)",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(has != 0)
}

/// SQL predicate (parameters `?4=buyer_cash_vat`, `?5=start`, `?6=end`) selecting a received
/// invoice whose input VAT is DEFERRED to payment: the buyer applies cash VAT in window
/// (art. 297(3)) OR the supplier applies cash VAT (art. 297(2), RPATVAÎ contact matched by CUI,
/// RO-prefix/case-insensitive). Used both to gate cash_vat_deductible_groups and to exclude
/// the same lines from the issue-date deductible sum.
const DEFERRED_RECEIVED_PREDICATE: &str = "( \
       (?4 = 1 AND (?5 IS NULL OR ri.issue_date >= ?5) AND (?6 IS NULL OR ri.issue_date <= ?6)) \
       OR EXISTS (SELECT 1 FROM contacts c WHERE c.company_id = ri.company_id \
                  AND REPLACE(UPPER(TRIM(c.cui)),'RO','') = REPLACE(UPPER(TRIM(ri.issuer_cui)),'RO','') \
                  AND c.cash_vat = 1) \
     )";

/// Cash-VAT (TVA la încasare) DEDUCTIBLE groups — buyer-side mirror of
/// `cash_vat_collected_groups`. For a deferred received invoice the standard-rate ("S") input
/// VAT becomes deductible only as the supplier is PAID (art. 297(2)/(3)), so it is attributed
/// to the PAYMENT period (D300 rd.24/25; old rate → rd.33). Sums the released base+VAT over
/// received_invoice_payments (paid_at ∈ period), proportional (allocate_collection), grouped by
/// original rate. Returns map `(rate_key, "S")` → `(rate, base_ron, vat_ron)`.
async fn cash_vat_deductible_groups(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    buyer_cash_vat: bool,
    cash_vat_start: Option<&str>,
    cash_vat_end: Option<&str>,
    period_from: &str,
    period_to: &str,
) -> AppResult<BTreeMap<(i64, String), (Decimal, Decimal, Decimal)>> {
    use crate::anaf_decl::cash_vat::{allocate_collection, RateBucket};
    use rust_decimal::prelude::ToPrimitive;

    let mut out: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

    let sql = format!(
        "SELECT DISTINCT ri.id, COALESCE(ri.currency,'RON') AS currency, ri.exchange_rate \
         FROM received_invoice_payments rp \
         JOIN received_invoices ri ON ri.id = rp.received_invoice_id \
         WHERE rp.company_id = ?1 \
           AND substr(rp.paid_at,1,10) >= ?2 AND substr(rp.paid_at,1,10) <= ?3 \
           AND ri.status != 'REJECTED' \
           AND {DEFERRED_RECEIVED_PREDICATE}"
    );
    let invoice_rows = sqlx::query(&sql)
        .bind(company_id)
        .bind(period_from)
        .bind(period_to)
        .bind(buyer_cash_vat as i64)
        .bind(cash_vat_start)
        .bind(cash_vat_end)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

    for inv in &invoice_rows {
        let invoice_id: String = inv.try_get("id").unwrap_or_default();
        let currency: String = inv
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            inv.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        let line_rows = sqlx::query(
            "SELECT vat_category, vat_rate, base_amount, vat_amount \
             FROM received_invoice_vat_lines WHERE received_invoice_id = ?1",
        )
        .bind(&invoice_id)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

        let mut gross_bani: i64 = 0;
        let mut bucket_acc: BTreeMap<i64, (Decimal, i64, i64)> = BTreeMap::new();
        for l in &line_rows {
            let category: String = l
                .try_get("vat_category")
                .unwrap_or_else(|_| "S".to_string());
            let rate_s: String = l.try_get("vat_rate").unwrap_or_default();
            let base_s: String = l.try_get("base_amount").unwrap_or_default();
            let vat_s: String = l.try_get("vat_amount").unwrap_or_default();
            let base_ron = amount_to_ron(
                Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            );
            let vat_ron = amount_to_ron(
                Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            );
            // Denominator = the PAYABLE; reverse-charge (AE) / intra-EU (K) input VAT is
            // self-assessed and not paid to the supplier, so it is excluded — otherwise a
            // fully-paid mixed invoice would never release all the deferred S input VAT.
            let is_reverse_charge = matches!(category.trim(), "AE" | "K");
            gross_bani += ron_to_bani(base_ron);
            if !is_reverse_charge {
                gross_bani += ron_to_bani(vat_ron);
            }
            if category.trim() == "S" {
                let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
                let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
                let e = bucket_acc.entry(rate_key).or_insert((rate, 0, 0));
                e.1 += ron_to_bani(base_ron);
                e.2 += ron_to_bani(vat_ron);
            }
        }
        if bucket_acc.is_empty() || gross_bani <= 0 {
            continue;
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

        let pay_rows = sqlx::query(
            "SELECT amount, paid_at FROM received_invoice_payments \
             WHERE received_invoice_id = ?1 AND substr(paid_at,1,10) <= ?2 ORDER BY paid_at, id",
        )
        .bind(&invoice_id)
        .bind(period_to)
        .fetch_all(pool)
        .await
        .map_err(AppError::Database)?;

        let mut paid_before: i64 = 0;
        for p in &pay_rows {
            let amt_s: String = p.try_get("amount").unwrap_or_default();
            let paid_at: String = p.try_get("paid_at").unwrap_or_default();
            let pay_bani = ron_to_bani(amount_to_ron(
                Decimal::from_str(&amt_s).unwrap_or(Decimal::ZERO),
                &currency,
                fx,
            ));
            if pay_bani <= 0 {
                continue;
            }
            let pd = if paid_at.len() >= 10 {
                &paid_at[..10]
            } else {
                paid_at.as_str()
            };
            if pd >= period_from && pd <= period_to {
                for rb in allocate_collection(gross_bani, &buckets, paid_before, pay_bani) {
                    let rate = *rate_of.get(&rb.rate_key).unwrap_or(&Decimal::ZERO);
                    let e = out.entry((rb.rate_key, "S".to_string())).or_insert((
                        rate,
                        Decimal::ZERO,
                        Decimal::ZERO,
                    ));
                    e.1 += Decimal::from(rb.base_bani) / Decimal::from(100);
                    e.2 += Decimal::from(rb.vat_bani) / Decimal::from(100);
                }
            }
            paid_before += pay_bani;
        }
    }

    Ok(out)
}

/// Computează totalul TVA colectat + deductibil din sursele primare pentru o
/// perioadă, fără niciun override.  Aceasta este sursa unică de adevăr la care
/// trebuie să se raporteze atât `compute_d300` cât și `reconcile` din GL.
///
/// Returnează `(collected_ron, deductible_ron)` rotunjite la 2 zecimale.
pub(crate) async fn d300_vat_totals(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> crate::error::AppResult<(Decimal, Decimal)> {
    // Cash-VAT regime: when on, standard-rate ("S") output VAT is exigible on COLLECTION, so
    // it is excluded from the issue-date sum below and re-added by collection date via
    // cash_vat_collected_groups — kept byte-consistent with compute_d300.
    let (cash_vat, cash_vat_start, cash_vat_end) = fetch_cash_vat_flags(pool, company_id).await?;

    // ── TVA colectată: Σvat_amount din liniile facturilor emise (VALIDATED+STORNED) ──
    let mut collected_sql = String::from(
        "SELECT l.vat_amount, COALESCE(i.currency,'RON') as currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3",
    );
    if cash_vat {
        collected_sql.push_str(
            " AND NOT (TRIM(l.vat_category) = 'S' \
               AND i.status = 'VALIDATED' \
               AND CAST(i.total_amount AS REAL) > 0 \
               AND (?4 IS NULL OR i.issue_date >= ?4) \
               AND (?5 IS NULL OR i.issue_date <= ?5))",
        );
    }
    let mut sales_q = sqlx::query(&collected_sql)
        .bind(company_id)
        .bind(period_from)
        .bind(period_to);
    if cash_vat {
        sales_q = sales_q
            .bind(cash_vat_start.clone())
            .bind(cash_vat_end.clone());
    }
    let sales_rows = sales_q
        .fetch_all(pool)
        .await
        .map_err(crate::error::AppError::Database)?;

    let mut collected = Decimal::ZERO;
    for row in &sales_rows {
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        collected += amount_to_ron(
            Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
    }

    // Cash-VAT: add the deferred "S" output VAT made exigible by collections in the period.
    if cash_vat {
        let deferred = cash_vat_collected_groups(
            pool,
            company_id,
            cash_vat_start.as_deref(),
            cash_vat_end.as_deref(),
            period_from,
            period_to,
        )
        .await?;
        for (_, _base, vat) in deferred.values() {
            collected += *vat;
        }
        // art. 282: undo the issue-date over-reversal of a cross-period storno's deferred part.
        let storno_corr =
            cash_vat_storno_collected_correction(pool, company_id, period_from, period_to).await?;
        for vat in storno_corr.values() {
            collected += *vat;
        }
    }

    // Buyer-side cash VAT: when the buyer applies cash VAT OR has a cash-VAT supplier, the
    // deferred "S" input VAT is excluded here and re-added by supplier-payment date below.
    let buyer_active = buyer_side_active(pool, company_id, cash_vat).await?;

    // ── TVA deductibilă: Σvat_amount din received_invoice_vat_lines ──
    let mut deductible_sql = String::from(
        "SELECT vl.vat_amount, vl.vat_category, COALESCE(ri.currency,'RON') as currency, ri.exchange_rate \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED'",
    );
    if buyer_active {
        deductible_sql.push_str(&format!(
            " AND NOT (TRIM(vl.vat_category) = 'S' AND {DEFERRED_RECEIVED_PREDICATE})"
        ));
    }
    let mut pq = sqlx::query(&deductible_sql)
        .bind(company_id)
        .bind(period_from)
        .bind(period_to);
    if buyer_active {
        pq = pq
            .bind(cash_vat as i64)
            .bind(cash_vat_start.clone())
            .bind(cash_vat_end.clone());
    }
    let purch_rows = pq
        .fetch_all(pool)
        .await
        .map_err(crate::error::AppError::Database)?;

    let mut deductible = Decimal::ZERO;
    for row in &purch_rows {
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let category: String = row.try_get("vat_category").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        deductible += vat_ron;
        // Taxare inversă (AE intern / K intracomunitar): cumpărătorul autolichidează
        // TVA-ul colectat (registrul GL înregistrează C 4427 pe lângă D 4426).
        // Reflectăm și pe latura colectată ca reconcilierea GL ↔ D300 să se închidă
        // pentru achizițiile art.331 / intracomunitare.
        if category == "AE" || category == "K" {
            collected += vat_ron;
        }
    }

    // Buyer-side cash VAT: add the deferred "S" input VAT made deductible by supplier payments
    // in the period (art. 297(2)/(3); D300 rd.24/25).
    if buyer_active {
        let deferred_ded = cash_vat_deductible_groups(
            pool,
            company_id,
            cash_vat,
            cash_vat_start.as_deref(),
            cash_vat_end.as_deref(),
            period_from,
            period_to,
        )
        .await?;
        for (_, _base, vat) in deferred_ded.values() {
            deductible += *vat;
        }
    }

    Ok((
        collected.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero),
        deductible.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero),
    ))
}

// ── Cash-VAT plafon monitor (slice 8) ───────────────────────────────────────

/// Cash-VAT (TVA la încasare) plafon status for a company at a reference date.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlafonStatus {
    /// Whether the company currently has the cash-VAT regime enabled.
    pub on_cash_vat: bool,
    /// Cumulative current-year cifra de afaceri (net, RON, 2 decimals).
    pub ca_ron: String,
    /// Applicable plafon in lei (OUG 8/2026).
    pub plafon_lei: i64,
    /// True when the cumulative CA strictly exceeds the plafon (mandatory exit triggered).
    pub exceeded: bool,
    /// The "YYYY-MM" month the plafon was first breached, if any.
    pub breach_month: Option<String>,
    /// Art. 324 alin. (14) exit-notificare deadline (form 700; 20th of the month after breach).
    pub notificare_deadline: Option<String>,
    /// Last date cash VAT still applies to new invoices (end of the fiscal period following
    /// the breach month); normal VAT applies from the next day. In-flight 4428 still finishes.
    pub cash_vat_stops_after: Option<String>,
}

/// Last calendar day of `(year, month)`.
fn last_day_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()
}

/// Monitor the cash-VAT eligibility plafon: cumulative current-year cifra de afaceri (net of
/// taxable + exempt supplies, issue-date basis) vs the OUG 8/2026 plafon, and — if breached —
/// the art. 324 alin. (14) exit-notificare deadline and the date cash VAT stops applying.
///
/// Note: the CA cannot exclude occasional fixed-asset / intangible disposals (the app does not
/// classify them), so it is a slight over-estimate vs the strict art. 282(3) base.
#[tauri::command]
pub async fn cash_vat_plafon_status(
    state: State<'_, AppState>,
    company_id: String,
    as_of: String,
) -> AppResult<PlafonStatus> {
    compute_plafon_status(&state.db, &company_id, &as_of).await
}

/// Intrastat threshold monitor (1.000.000 lei per flow, Ord. INS 1604/2025).
#[tauri::command]
pub async fn intrastat_status(
    state: State<'_, AppState>,
    company_id: String,
    as_of: String,
) -> AppResult<IntrastatStatus> {
    compute_intrastat_status(&state.db, &company_id, &as_of).await
}

/// 2026 Intrastat value threshold per flow (Ord. INS 1604/2025): 1.000.000 lei for both expedieri
/// (dispatches) and introduceri (arrivals). Above it on a flow, monthly Intrastat is mandatory.
const INTRASTAT_THRESHOLD_RON: i64 = 1_000_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntrastatFlowStatus {
    pub ytd_ron: String,
    pub pct: i64,
    /// "ok" | "approaching" (≥80%) | "exceeded" (>100%).
    pub level: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntrastatStatus {
    pub threshold_ron: String,
    pub dispatches: IntrastatFlowStatus,
    pub arrivals: IntrastatFlowStatus,
}

fn intrastat_flow_status(ytd: Decimal, threshold: Decimal) -> IntrastatFlowStatus {
    let pct = if threshold.is_zero() {
        0
    } else {
        use rust_decimal::prelude::ToPrimitive;
        (ytd / threshold * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap_or(0)
    };
    let level = if ytd > threshold {
        "exceeded"
    } else if pct >= 80 {
        "approaching"
    } else {
        "ok"
    };
    IntrastatFlowStatus {
        ytd_ron: format!("{:.2}", ytd),
        pct,
        level: level.to_string(),
    }
}

/// Intrastat threshold monitor: YTD intra-EU GOODS value per flow vs the 1.000.000-lei threshold.
/// Dispatches = outbound intra-EU goods (invoice lines, vat_category 'K'); arrivals = inbound
/// intra-EU goods acquisitions (received_invoice_vat_lines 'K' + intra_eu_kind 'goods').
pub(crate) async fn compute_intrastat_status(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    as_of: &str,
) -> AppResult<IntrastatStatus> {
    let year = as_of.get(0..4).unwrap_or("").to_string();
    let year_start = format!("{year}-01-01");

    // DISPATCHES — outbound intra-EU goods invoice lines.
    let disp_rows = sqlx::query(
        "SELECT li.subtotal_amount AS amt, COALESCE(i.currency,'RON') AS currency, \
                i.exchange_rate AS fx \
         FROM invoice_line_items li JOIN invoices i ON li.invoice_id = i.id \
         WHERE i.company_id = ?1 AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 AND i.issue_date <= ?3 \
           AND li.vat_category = 'K' AND COALESCE(li.revenue_kind,'goods') IN ('goods','product')",
    )
    .bind(company_id)
    .bind(&year_start)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    let mut dispatches = Decimal::ZERO;
    for row in &disp_rows {
        let amt: String = row.try_get("amt").unwrap_or_default();
        let currency: String = row.try_get("currency").unwrap_or_else(|_| "RON".into());
        let fx = parse_rate(row.try_get::<Option<f64>, _>("fx").unwrap_or(None));
        dispatches += amount_to_ron(
            Decimal::from_str(&amt).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
    }

    // ARRIVALS — inbound intra-EU goods acquisitions.
    let arr_rows = sqlx::query(
        "SELECT vl.base_amount AS amt, COALESCE(ri.currency,'RON') AS currency, \
                ri.exchange_rate AS fx \
         FROM received_invoice_vat_lines vl JOIN received_invoices ri ON vl.received_invoice_id = ri.id \
         WHERE ri.company_id = ?1 AND ri.status != 'REJECTED' \
           AND ri.issue_date >= ?2 AND ri.issue_date <= ?3 \
           AND vl.vat_category = 'K' AND COALESCE(ri.intra_eu_kind,'goods') = 'goods'",
    )
    .bind(company_id)
    .bind(&year_start)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    let mut arrivals = Decimal::ZERO;
    for row in &arr_rows {
        let amt: String = row.try_get("amt").unwrap_or_default();
        let currency: String = row.try_get("currency").unwrap_or_else(|_| "RON".into());
        let fx = parse_rate(row.try_get::<Option<f64>, _>("fx").unwrap_or(None));
        arrivals += amount_to_ron(
            Decimal::from_str(&amt).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
    }

    let threshold = Decimal::from(INTRASTAT_THRESHOLD_RON);
    Ok(IntrastatStatus {
        threshold_ron: format!("{:.2}", threshold),
        dispatches: intrastat_flow_status(dispatches, threshold),
        arrivals: intrastat_flow_status(arrivals, threshold),
    })
}

pub(crate) async fn compute_plafon_status(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    as_of: &str,
) -> AppResult<PlafonStatus> {
    use rust_decimal::prelude::ToPrimitive;

    let on_cash_vat = fetch_cash_vat_flags(pool, company_id).await?.0;

    // Current-year window [YYYY-01-01, as_of].
    let year = as_of.get(0..4).unwrap_or("").to_string();
    let year_start = format!("{year}-01-01");

    // Net (subtotal) per issue-month for the authorised fiscal set (VALIDATED + STORNED).
    let rows = sqlx::query(
        "SELECT substr(issue_date,1,7) AS ym, subtotal_amount, \
                COALESCE(currency,'RON') AS currency, exchange_rate \
         FROM invoices \
         WHERE company_id = ?1 \
           AND status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?2 \
           AND issue_date <= ?3",
    )
    .bind(company_id)
    .bind(&year_start)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Accumulate net (RON) per month.
    let mut by_month: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut ca = Decimal::ZERO;
    for row in &rows {
        let ym: String = row.try_get("ym").unwrap_or_default();
        let net_s: String = row.try_get("subtotal_amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let net = amount_to_ron(
            Decimal::from_str(&net_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        *by_month.entry(ym).or_insert(Decimal::ZERO) += net;
        ca += net;
    }

    // Ascending (month, net_lei) for the breach scan. Commercial rounding (MidpointAwayFromZero)
    // — the ANAF whole-lei convention; banker's rounding could miss a real plafon breach.
    let monthly_lei: Vec<(String, i64)> = by_month
        .iter()
        .map(|(m, n)| {
            (
                m.clone(),
                n.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_i64()
                    .unwrap_or(0),
            )
        })
        .collect();

    let plafon = crate::anaf_decl::cash_vat::plafon_lei(as_of);
    let breach_month = crate::anaf_decl::cash_vat::plafon_breach_month(&monthly_lei, plafon);

    // Exit deadlines: notificare by the 20th of the month AFTER the breach; cash VAT keeps
    // applying through the end of that same following fiscal period (art. 282 alin. (5)).
    let (notificare_deadline, cash_vat_stops_after) = match &breach_month {
        Some(bm) => {
            let y: i32 = bm.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let m: u32 = bm.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(0);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            let deadline = format!("{ny:04}-{nm:02}-20");
            let stops = last_day_of_month(ny, nm)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            (Some(deadline), Some(stops))
        }
        None => (None, None),
    };

    Ok(PlafonStatus {
        on_cash_vat,
        ca_ron: ca
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        plafon_lei: plafon,
        exceeded: breach_month.is_some(),
        breach_month,
        notificare_deadline,
        cash_vat_stops_after,
    })
}

/// Calculează decontul D300 (TVA colectat — vânzări + TVA deductibil — achiziții)
/// pentru o companie și o perioadă.
///
/// **Vânzări**: facturile cu status VALIDATED sau STORNED (setul fiscal autorizat
/// BIZ-11/storno-fix), identic cu rapoartele TVA, jurnalele, D394 și SAF-T,
/// astfel încât D300 reconciliază cu celelalte declarații.
/// Closing TVA-neexigibilă (4428) memo balances for the informational D300 rows A/A1/B/B1.
/// Splits the 4428 GL entries by side — output (sales: `journal_type='SALES'`, or a `'PAYMENT'`
/// journal carrying a customer = a sales collection) vs input (purchases) — nets the still-deferred
/// VAT as of `period_to`, and derives the base from each entry's rate (`tax_percentage`). A1/B1 keep
/// only the slice from the reporting period + the prior 5 months. Purely informational: never feeds
/// any total (OPANAF 174/2026).
#[allow(clippy::type_complexity)]
async fn cash_vat_memo_balances(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<CashVatMemo> {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // A1/B1 window: reporting period + the prior 5 months (monthly fiscal period).
    let aging_from = {
        let y: i32 = period_from
            .get(0..4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(2026);
        let m: i32 = period_from
            .get(5..7)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let t = y * 12 + (m - 1) - 5;
        format!("{:04}-{:02}-01", t / 12, t % 12 + 1)
    };

    let rows: Vec<(
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT j.journal_type, j.customer_id, j.supplier_id, j.transaction_date, \
                e.debit, e.credit, e.tax_percentage \
         FROM gl_entry e JOIN gl_journal j ON j.id = e.journal_pk \
         WHERE j.company_id = ?1 AND e.account_code = '4428' AND j.transaction_date <= ?2",
    )
    .bind(company_id)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    let (mut a_vat, mut a_base, mut a1_vat, mut a1_base) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let (mut b_vat, mut b_base, mut b1_vat, mut b1_base) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);

    for (jt, cust, supp, date, debit, credit, rate_s) in &rows {
        let rate = rate_s
            .as_deref()
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or(Decimal::ZERO);
        if rate <= Decimal::ZERO {
            continue; // cash-VAT is standard-rate only; no rate → no derivable base
        }
        let d = Decimal::from_str(debit).unwrap_or(Decimal::ZERO);
        let c = Decimal::from_str(credit).unwrap_or(Decimal::ZERO);
        let is_output = jt == "SALES" || (jt == "PAYMENT" && cust.is_some());
        let is_input = jt == "PURCHASE" || (jt == "PAYMENT" && supp.is_some());
        // Deferred output VAT accumulates as a 4428 credit (released = debit); input is the reverse.
        let signed_vat = if is_output {
            c - d
        } else if is_input {
            d - c
        } else {
            continue;
        };
        let base = signed_vat * Decimal::from(100) / rate; // base = VAT / (rate%/100)
        let in_aging = date.as_str() >= aging_from.as_str();
        if is_output {
            a_vat += signed_vat;
            a_base += base;
            if in_aging {
                a1_vat += signed_vat;
                a1_base += base;
            }
        } else {
            b_vat += signed_vat;
            b_base += base;
            if in_aging {
                b1_vat += signed_vat;
                b1_base += base;
            }
        }
    }

    let lei = |x: Decimal| crate::anaf_decl::round_lei(x).max(0);
    Ok(CashVatMemo {
        a_base: lei(a_base),
        a_vat: lei(a_vat),
        a1_base: lei(a1_base),
        a1_vat: lei(a1_vat),
        b_base: lei(b_base),
        b_vat: lei(b_vat),
        b1_base: lei(b1_base),
        b1_vat: lei(b1_vat),
    })
}

/// **Achiziții**: facturile primite cu status != REJECTED, defalcate din
/// `received_invoice_vat_lines`. Facturile fără defalcare sunt contorizate în
/// `purchase_unparsed_count` și nu contribuie la totaluri.
/// Gruparea se face după (cotă, categorie) — BIZ-12.
#[tauri::command]
pub async fn compute_d300(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<D300Report> {
    use rust_decimal::prelude::ToPrimitive;

    crate::commands::require_valid_date("Data de început", &period_from)?;
    crate::commands::require_valid_date("Data de sfârșit", &period_to)?;
    let pool = &state.db;

    // Fetch CUI-ul companiei.
    let company_row = sqlx::query("SELECT cui FROM companies WHERE id = ?1 LIMIT 1")
        .bind(&company_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::NotFound)?;

    let company_cui: String = company_row
        .try_get("cui")
        .unwrap_or_else(|_| company_id.clone());

    // Cash-VAT regime: when on, standard-rate ("S") output VAT is exigible on COLLECTION.
    // Such lines are excluded from the issue-date grouping below and re-added by collection
    // date via cash_vat_collected_groups; excluded categories + non-cash-VAT are unchanged.
    let (cash_vat, cash_vat_start, cash_vat_end) = fetch_cash_vat_flags(pool, &company_id).await?;

    // Numărul total de facturi din setul fiscal autorizat în perioadă (pentru header).
    // Setul fiscal: VALIDATED + STORNED — același set ca rapoartele TVA, jurnalele,
    // D394 și SAF-T, pentru reconciliere completă (storno-fix).
    let count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM invoices \
         WHERE status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?1 \
           AND issue_date <= ?2 \
           AND company_id = ?3",
    )
    .bind(&period_from)
    .bind(&period_to)
    .bind(&company_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let invoice_count: i64 = count_row.try_get("cnt").unwrap_or(0);

    // Fetch liniile de factură pentru grupare TVA — refolosind query-ul din reports.rs.
    // BIZ-12: grupăm după (vat_rate, vat_category) — cote identice cu categorii diferite
    // (e.g. 0% Scutit "E" vs. 0% Zero-rated "Z") rămân rânduri separate.
    // Wave 4: fetch i.currency + i.exchange_rate for RON normalisation.
    // Storno-fix: setul fiscal autorizat este VALIDATED + STORNED, identic cu
    // rapoartele TVA, D394 și SAF-T, astfel încât D300 reconciliază cu celelalte.
    let mut sales_sql = String::from(
        "SELECT l.vat_rate, l.vat_category, l.subtotal_amount, l.vat_amount, \
                COALESCE(i.currency, 'RON') AS currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND i.company_id = ?3",
    );
    if cash_vat {
        // Deferred "S" lines (issued in-window) move to the collection-date path.
        sales_sql.push_str(
            " AND NOT (TRIM(l.vat_category) = 'S' \
               AND i.status = 'VALIDATED' \
               AND CAST(i.total_amount AS REAL) > 0 \
               AND (?4 IS NULL OR i.issue_date >= ?4) \
               AND (?5 IS NULL OR i.issue_date <= ?5))",
        );
    }
    let mut sales_q = sqlx::query(&sales_sql)
        .bind(&period_from)
        .bind(&period_to)
        .bind(&company_id);
    if cash_vat {
        sales_q = sales_q
            .bind(cash_vat_start.clone())
            .bind(cash_vat_end.clone());
    }
    let line_rows = sales_q.fetch_all(pool).await.map_err(AppError::Database)?;

    // Acumulăm în BTreeMap<(rate_key_i64, category), (rate_dec, base_sum, vat_sum)>
    // — același pattern ca în reports.rs::generate_vat_report.
    let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

    for row in &line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| String::from("S"));
        let base_s: String = row.try_get("subtotal_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );

        let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);

        // Convert per-line amounts to RON before accumulating (RON rows unchanged).
        let base_ron = amount_to_ron(
            Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );

        let e = groups
            .entry((rate_key, category))
            .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
        e.1 += base_ron;
        e.2 += vat_ron;
    }

    // Cash-VAT: splice in the deferred "S" base+VAT made exigible by collections in the
    // period (grouped by original rate). Old-rate buckets (19/5%) flow through the existing
    // reg_colectata (rd.16) loop below exactly like issue-date old-rate S sales.
    if cash_vat {
        let deferred = cash_vat_collected_groups(
            pool,
            &company_id,
            cash_vat_start.as_deref(),
            cash_vat_end.as_deref(),
            &period_from,
            &period_to,
        )
        .await?;
        for ((rate_key, cat), (rate, base, vat)) in deferred {
            let e = groups
                .entry((rate_key, cat))
                .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
            e.1 += base;
            e.2 += vat;
        }
        // art. 282 storno correction: add back the deferred VAT (+ proportional base, so the rd row
        // stays vat ≈ base×cotă for DUK) over-reversed at issue date for cross-period cash-VAT storni.
        let storno_corr =
            cash_vat_storno_collected_correction(pool, &company_id, &period_from, &period_to)
                .await?;
        for (rate_key, vat_corr) in storno_corr {
            let rate = Decimal::from(rate_key) / Decimal::from(100);
            let base_corr = if rate_key != 0 {
                vat_corr * Decimal::from(10000) / Decimal::from(rate_key)
            } else {
                Decimal::ZERO
            };
            let e = groups.entry((rate_key, "S".to_string())).or_insert((
                rate,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            e.1 += base_corr;
            e.2 += vat_corr;
        }
    }

    // Calculăm totalurile și construim Vec<D300Group> descrescător după cotă.
    let mut total_base = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;

    // BTreeMap e crescător → rev() pentru descrescător după cotă (ca în reports.rs).
    let groups_vec: Vec<D300Group> = groups
        .into_iter()
        .rev()
        .map(|((_rate_key, category), (rate, base_sum, vat_sum))| {
            total_base += base_sum;
            total_vat += vat_sum;
            D300Group {
                vat_rate: rate
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat_category: category,
                base: base_sum
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat: vat_sum
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                intra_eu_kind: None, // sales groups never have an intra_eu_kind
            }
        })
        .collect();

    // ── Wave B: achiziții — received_invoice_vat_lines ────────────────────────

    // Numărul de facturi primite în perioadă (status != REJECTED).
    let purchase_count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED'",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let purchase_invoice_count: i64 = purchase_count_row.try_get("cnt").unwrap_or(0);

    // Numărul de facturi primite fără defalcare TVA (net_amount IS NULL).
    let unparsed_count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED' \
           AND net_amount IS NULL",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let purchase_unparsed_count: i64 = unparsed_count_row.try_get("cnt").unwrap_or(0);

    // Fetch liniile VAT din facturile primite (JOIN pentru filtru perioadă + companie).
    // Wave 4: fetch ri.currency + ri.exchange_rate for RON normalisation.
    // Wave 7: fetch ri.intra_eu_kind so K acquisitions can be split goods/services.
    // Buyer-side cash VAT: deferred "S" input VAT moves to the supplier-payment-date path.
    let buyer_active = buyer_side_active(pool, &company_id, cash_vat).await?;
    let mut purchase_sql = String::from(
        "SELECT vl.vat_rate, vl.vat_category, vl.base_amount, vl.vat_amount, \
                COALESCE(ri.currency, 'RON') AS currency, ri.exchange_rate, \
                ri.intra_eu_kind \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED'",
    );
    if buyer_active {
        purchase_sql.push_str(&format!(
            " AND NOT (TRIM(vl.vat_category) = 'S' AND {DEFERRED_RECEIVED_PREDICATE})"
        ));
    }
    let mut pq = sqlx::query(&purchase_sql)
        .bind(&company_id)
        .bind(&period_from)
        .bind(&period_to);
    if buyer_active {
        pq = pq
            .bind(cash_vat as i64)
            .bind(cash_vat_start.clone())
            .bind(cash_vat_end.clone());
    }
    let purchase_line_rows = pq.fetch_all(pool).await.map_err(AppError::Database)?;

    // Acumulăm per (rate_key, category, kind) — kind este non-empty numai pentru K.
    // Astfel K-goods și K-services acumulează separat; toate celelalte categorii
    // folosesc kind="" (nu contează pentru rândul D300 al lor).
    let mut purchase_groups: BTreeMap<(i64, String, String), (Decimal, Decimal, Decimal)> =
        BTreeMap::new();

    for row in &purchase_line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| String::from("S"));
        let base_s: String = row.try_get("base_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        // intra_eu_kind: meaningful only for K; default "goods" (migration default).
        let intra_eu_kind: String = row
            .try_get("intra_eu_kind")
            .unwrap_or_else(|_| "goods".to_string());

        let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);

        // kind key: only meaningful for K; empty string for all other categories
        // so they accumulate as before (no behavioural change outside K).
        let kind_key = if category == "K" {
            intra_eu_kind.clone()
        } else {
            String::new()
        };

        // Convert per-line amounts to RON before accumulating (RON rows unchanged).
        let base_ron = amount_to_ron(
            Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );

        let e = purchase_groups
            .entry((rate_key, category, kind_key))
            .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
        e.1 += base_ron;
        e.2 += vat_ron;
    }

    // Cash-VAT buyer side: splice in the deferred "S" input VAT made deductible by supplier
    // payments in the period (kind="" — S is never intra-EU). Old-rate buckets feed the
    // existing reg_dedusa (rd.33) loop exactly like issue-date old-rate S purchases.
    if buyer_active {
        let deferred_ded = cash_vat_deductible_groups(
            pool,
            &company_id,
            cash_vat,
            cash_vat_start.as_deref(),
            cash_vat_end.as_deref(),
            &period_from,
            &period_to,
        )
        .await?;
        for ((rate_key, cat), (rate, base, vat)) in deferred_ded {
            let e = purchase_groups
                .entry((rate_key, cat, String::new()))
                .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
            e.1 += base;
            e.2 += vat;
        }
    }

    let mut total_deductible_base = Decimal::ZERO;
    let mut total_deductible_vat = Decimal::ZERO;

    let purchase_groups_vec: Vec<D300Group> = purchase_groups
        .into_iter()
        .rev()
        .map(|((_rate_key, category, kind), (rate, base_sum, vat_sum))| {
            total_deductible_base += base_sum;
            total_deductible_vat += vat_sum;
            // intra_eu_kind: Some("goods"|"services") for K groups; None elsewhere.
            let intra_eu_kind = if category == "K" {
                Some(if kind.is_empty() {
                    "goods".to_string()
                } else {
                    kind
                })
            } else {
                None
            };
            D300Group {
                vat_rate: rate
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat_category: category,
                base: base_sum
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat: vat_sum
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                intra_eu_kind,
            }
        })
        .collect();

    // TVA netă de plată = colectată − deductibilă (negativă = de recuperat).
    let net_vat = total_vat - total_deductible_vat;

    // ── Wave 8: regularizări cote vechi ──────────────────────────────────────
    // Σ vânzări S la cote 19%/5% → auto-prefill R16 (regularizări colectată)
    let mut reg_coll_base = Decimal::ZERO;
    let mut reg_coll_tva = Decimal::ZERO;
    for g in &groups_vec {
        if g.vat_category == "S" {
            let rate_d = Decimal::from_str(&g.vat_rate).unwrap_or(Decimal::ZERO);
            let rate_pct = if rate_d > Decimal::ONE {
                rate_d
                    .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_i64()
                    .unwrap_or(-1)
            } else {
                (rate_d * Decimal::from(100))
                    .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_i64()
                    .unwrap_or(-1)
            };
            if matches!(rate_pct, 19 | 5) {
                reg_coll_base += Decimal::from_str(&g.base).unwrap_or(Decimal::ZERO);
                reg_coll_tva += Decimal::from_str(&g.vat).unwrap_or(Decimal::ZERO);
            }
        }
    }
    // Σ achiziții S la cote 19%/9%/5% → auto-prefill R30 (regularizări dedusă)
    let mut reg_ded_base = Decimal::ZERO;
    let mut reg_ded_tva = Decimal::ZERO;
    for g in &purchase_groups_vec {
        if g.vat_category == "S" {
            let rate_d = Decimal::from_str(&g.vat_rate).unwrap_or(Decimal::ZERO);
            let rate_pct = if rate_d > Decimal::ONE {
                rate_d
                    .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_i64()
                    .unwrap_or(-1)
            } else {
                (rate_d * Decimal::from(100))
                    .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_i64()
                    .unwrap_or(-1)
            };
            if matches!(rate_pct, 19 | 9 | 5) {
                reg_ded_base += Decimal::from_str(&g.base).unwrap_or(Decimal::ZERO);
                reg_ded_tva += Decimal::from_str(&g.vat).unwrap_or(Decimal::ZERO);
            }
        }
    }

    let cash_vat_memo = cash_vat_memo_balances(pool, &company_id, &period_from, &period_to).await?;

    Ok(D300Report {
        company_cui,
        period_from,
        period_to,
        groups: groups_vec,
        total_base: total_base
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        total_vat: total_vat
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        invoice_count,
        purchase_groups: purchase_groups_vec,
        total_deductible_base: total_deductible_base
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        total_deductible_vat: total_deductible_vat
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        purchase_invoice_count,
        purchase_unparsed_count,
        net_vat: net_vat
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        reg_colectata_baza: reg_coll_base
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        reg_colectata_tva: reg_coll_tva
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        reg_dedusa_baza: reg_ded_base
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        reg_dedusa_tva: reg_ded_tva
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string(),
        cash_vat_memo,
    })
}

/// Generează fișierul XML D300 și îl scrie la calea specificată.
/// Returnează calea fișierului salvat.
///
/// Formatul XML este un extract structurat al decontului D300 pentru vânzări + achiziții.
/// Header: CUI, perioadă, tip declarație. Body: grupuri TVA colectat + deductibil + TVA netă.
/// NOTE: Nu este formularul complet ANAF D300 cu schema oficială — depunerea
/// electronică necesită integrare cu sistemul ANAF e-Formulare.
///
/// R4: `manual_deductible_vat` — when provided, overrides the computed
/// `total_deductible_vat` and recalculates `net_vat` so the exported XML
/// matches what the user sees on screen after a manual override.
/// When `None`, the server-computed value is used (backward-compatible).
#[tauri::command]
pub async fn export_d300(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    dest_path: String,
    manual_deductible_vat: Option<String>,
) -> AppResult<String> {
    // Validate the user-chosen destination path (same guard as every other export
    // command: absolute, no UNC, no `..` traversal, allowed extension).
    let dest_path = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();

    // Calculăm mai întâi raportul.
    let mut report = compute_d300(state, company_id, period_from, period_to).await?;

    // R4: apply the manual override if provided.
    if let Some(ref override_str) = manual_deductible_vat {
        if let Ok(override_dec) = Decimal::from_str(override_str.trim()) {
            let total_vat = Decimal::from_str(&report.total_vat).unwrap_or(Decimal::ZERO);
            let net_vat = total_vat - override_dec;
            report.total_deductible_vat = override_dec
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string();
            report.net_vat = net_vat
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string();
        }
    }

    let dest = dest_path.clone();
    // Construim XML-ul în spawn_blocking (I/O + string building) — pattern din saft.rs.
    tokio::task::spawn_blocking(move || build_and_write_xml(report, dest))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

// ── XML builder ───────────────────────────────────────────────────────────────

fn build_and_write_xml(report: D300Report, dest_path: String) -> AppResult<String> {
    let generated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut xml = String::with_capacity(8192);

    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!-- D300 Decont TVA — Extras vânzări + achiziții generat de Clarito -->\n");
    xml.push_str("<!-- Schema oficială ANAF D300 necesită depunere prin e-Formulare.      -->\n");
    xml.push_str("<D300>\n");

    // ── Header ────────────────────────────────────────────────────────────────
    xml.push_str("  <Header>\n");
    xml.push_str("    <TipDeclaratie>D300</TipDeclaratie>\n");
    xml.push_str(&format!(
        "    <CUI>{}</CUI>\n",
        xml_escape(&report.company_cui)
    ));
    xml.push_str(&format!(
        "    <PerioadaDeLa>{}</PerioadaDeLa>\n",
        xml_escape(&report.period_from)
    ));
    xml.push_str(&format!(
        "    <PerioadaPanaLa>{}</PerioadaPanaLa>\n",
        xml_escape(&report.period_to)
    ));
    xml.push_str(&format!(
        "    <GeneratLa>{}</GeneratLa>\n",
        xml_escape(&generated_at)
    ));
    xml.push_str(&format!(
        "    <NrFacturiValidate>{}</NrFacturiValidate>\n",
        report.invoice_count
    ));
    xml.push_str("  </Header>\n");

    // ── VanzariTVAColectat (livrări) ──────────────────────────────────────────
    xml.push_str("  <VanzariTVAColectat>\n");
    xml.push_str("    <!-- Grupuri TVA sortate descrescător după cotă -->\n");

    for group in &report.groups {
        xml.push_str("    <Grupa>\n");
        xml.push_str(&format!(
            "      <CotaTVA>{}</CotaTVA>\n",
            xml_escape(&group.vat_rate)
        ));
        xml.push_str(&format!(
            "      <CategorieTVA>{}</CategorieTVA>\n",
            xml_escape(&group.vat_category)
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&group.base)
        ));
        xml.push_str(&format!(
            "      <TVAColectat>{}</TVAColectat>\n",
            xml_escape(&group.vat)
        ));
        xml.push_str("    </Grupa>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVAColectat>{}</TotalTVAColectat>\n",
        xml_escape(&report.total_vat)
    ));
    xml.push_str("  </VanzariTVAColectat>\n");

    // ── AchizitiiTVADeductibil ────────────────────────────────────────────────
    xml.push_str("  <AchizitiiTVADeductibil>\n");
    xml.push_str("    <!-- TVA deductibilă din received_invoice_vat_lines (Wave B) -->\n");
    if report.purchase_unparsed_count > 0 {
        xml.push_str(&format!(
            "    <!-- ATENȚIE: {} facturi primite nu au defalcare TVA (net_amount IS NULL) — cifrele de mai jos sunt parțiale. -->\n",
            report.purchase_unparsed_count
        ));
    }

    for group in &report.purchase_groups {
        xml.push_str("    <Grupa>\n");
        xml.push_str(&format!(
            "      <CotaTVA>{}</CotaTVA>\n",
            xml_escape(&group.vat_rate)
        ));
        xml.push_str(&format!(
            "      <CategorieTVA>{}</CategorieTVA>\n",
            xml_escape(&group.vat_category)
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&group.base)
        ));
        xml.push_str(&format!(
            "      <TVADeductibil>{}</TVADeductibil>\n",
            xml_escape(&group.vat)
        ));
        xml.push_str("    </Grupa>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_deductible_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVADeductibil>{}</TotalTVADeductibil>\n",
        xml_escape(&report.total_deductible_vat)
    ));
    xml.push_str("  </AchizitiiTVADeductibil>\n");

    // ── TVA netă de plată / de recuperat ─────────────────────────────────────
    xml.push_str(&format!(
        "  <!-- TVADePlata = TVA colectată − TVA deductibilă; negativ înseamnă de recuperat -->\n  <TVADePlata>{}</TVADePlata>\n",
        xml_escape(&report.net_vat)
    ));

    xml.push_str("</D300>\n");

    // Validate the caller-supplied destination (absolute, no '..', no UNC, whitelist ext) — the
    // IPC endpoint accepts an arbitrary string.
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;
    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;

    Ok(dest_path)
}

/// Generează fișierul XML D300 oficial ANAF (schema v12) și îl scrie la calea specificată.
///
/// Aceasta este comanda de export **oficial** (schema-conformant, validat cu XSD).
/// Diferă de `export_d300` (working-paper preview) prin:
/// - Emite un `<declaratie300>` cu namespace-ul ANAF exact (`mfp:anaf:dgti:d300:declaratie:v12`)
/// - Toate datele sunt atribute, sumele sunt rotunjite la lei întregi
/// - Maparea rândurilor respectă structura_D300_v12 oficială
///
/// Parametrul `submission` conține câmpurile completate de utilizator (declarant, CAEN, bancă etc.)
/// care nu sunt derivabile din datele fiscale.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_d300_official(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    submission: D300Submission,
    dest_path: String,
    skip_duk_override: bool,
) -> AppResult<OfficialExportResult> {
    use crate::anaf_decl::d300::generator::generate_d300_xml;
    use crate::anaf_decl::d300::rows::map_to_rows;

    // Validate destination path (.xml whitelisted by validate_export_path)
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();

    // Parse period_from to resolve the schema version and extract luna/an.
    let period = NaiveDate::parse_from_str(&period_from, "%Y-%m-%d").map_err(|_| {
        AppError::Validation(format!(
            "period_from '{period_from}' nu este în formatul YYYY-MM-DD."
        ))
    })?;

    // Resolve schema version for the reported period.
    let ver = resolve(DeclKind::D300, period)?;

    // Fetch the company record (needed for cui/den/adresa).
    let company = companies::get(&state.db, &company_id).await?;
    // Clonăm pool-ul înainte de a muta `state` în compute_d300 — necesar pentru înregistrarea depunerii.
    let pool = state.db.clone();

    // Compute the fiscal aggregates.
    let report = compute_d300(state, company_id, period_from, period_to).await?;

    // Map to D300Rows (all amounts rounded to lei, totals computed).
    let rows = map_to_rows(&report, &submission, &company, period)?;

    // Generate XML.
    let xml = generate_d300_xml(&rows, &ver)?;

    // Layer D: validate with the bundled DUK before writing. Graceful: no runtime → proceed.
    let tmp =
        std::env::temp_dir().join(format!("d300_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let duk = crate::anaf_decl::duk::run_duk(&provider, DeclKind::D300, &tmp)?;
    let _ = std::fs::remove_file(&tmp);
    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
    if !duk_gate_allows_write(duk_available, duk_passed, skip_duk_override) {
        return Ok(OfficialExportResult {
            path: String::new(),
            written: false,
            duk_available,
            duk_passed,
            issues,
        });
    }

    // Write to disk.
    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    // Înregistrează depunerea în istoric (best-effort — erorile sunt înghițite).
    // `period` (NaiveDate) derivat din period_from înainte de a fi mutat în compute_d300.
    use chrono::Datelike as _;
    let filing_period = format!("{}-{:02}", period.year(), period.month());
    let _ = crate::db::declaration_filings::record(
        &pool,
        crate::db::declaration_filings::FilingInput {
            company_id: company.id.clone(),
            kind: "D300".into(),
            period: filing_period,
            is_rectificative: false,
            file_path: Some(dest.clone()),
        },
    )
    .await;
    Ok(OfficialExportResult {
        path: dest,
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}

/// Construiește XML-ul D300 fără a-l scrie pe disc și fără gate-ul DUK — pentru previzualizare/editare
/// în vizualizatorul XML din aplicație. Parcurge EXACT aceiași pași de build ca `export_d300_official`
/// (resolve ver → company → compute_d300 → map_to_rows → generate_d300_xml), așa că re-validarea cu
/// DUK (`validate_declaration_xml`) e relevantă. Niciun fișier temporar, niciun `run_duk`, nicio scriere.
#[tauri::command]
pub async fn preview_d300_xml(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    submission: D300Submission,
) -> AppResult<String> {
    use crate::anaf_decl::d300::generator::generate_d300_xml;
    use crate::anaf_decl::d300::rows::map_to_rows;

    // Parse period_from to resolve the schema version and extract luna/an.
    let period = NaiveDate::parse_from_str(&period_from, "%Y-%m-%d").map_err(|_| {
        AppError::Validation(format!(
            "period_from '{period_from}' nu este în formatul YYYY-MM-DD."
        ))
    })?;

    // Resolve schema version for the reported period.
    let ver = resolve(DeclKind::D300, period)?;

    // Fetch the company record (needed for cui/den/adresa) — înainte de a muta `state` în compute_d300.
    let company = companies::get(&state.db, &company_id).await?;

    // Compute the fiscal aggregates.
    let report = compute_d300(state, company_id, period_from, period_to).await?;

    // Map to D300Rows (all amounts rounded to lei, totals computed).
    let rows = map_to_rows(&report, &submission, &company, period)?;

    // Generate XML (fără validare DUK, fără scriere — doar întoarce șirul).
    generate_d300_xml(&rows, &ver)
}

/// Pre-export validation — runs pure-Rust checks and returns friendly Romanian
/// messages for the most common DUKIntegrator-fatal issues.
///
/// `kind` is one of: `"D300"`, `"D394"`, `"D406"` (or `"SAFT"` as alias).
/// Anything unrecognised defaults to `D300`.
#[tauri::command]
pub async fn preflight_declaration(
    state: State<'_, AppState>,
    company_id: String,
    kind: String,
    period_from: String,
    period_to: String,
) -> AppResult<Vec<crate::anaf_decl::preflight::PreflightIssue>> {
    let decl_kind = match kind.to_uppercase().as_str() {
        "D394" => DeclKind::D394,
        "D406" | "SAFT" => DeclKind::D406,
        _ => DeclKind::D300, // "D300" or anything unrecognised
    };
    crate::anaf_decl::preflight::preflight(
        &state.db,
        &company_id,
        decl_kind,
        &period_from,
        &period_to,
    )
    .await
}

// ── RO e-TVA reconciliation (pre-filing self-check) ─────────────────────────────

/// Reconcile the app-computed D300 against the ANAF "decont precompletat" (P300ETVA) values the
/// caller imports from SPV. 2026: the conformance notification is abolished (OUG 89/2025 +
/// OUG 13/2026) — this is an INFORMATIVE self-check, not a notification-response flow. The
/// precompletat JSON has no published XSD and is fetched via a dedicated SPV endpoint (auth
/// required); live retrieval/JSON-mapping is out of scope, so the caller supplies the values.
#[tauri::command]
pub async fn reconcile_etva(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    precompletat: crate::anaf_decl::etva::EtvaPrecompletat,
) -> crate::error::AppResult<crate::anaf_decl::etva::EtvaReconciliation> {
    use crate::anaf_decl::etva::{reconcile_line, EtvaReconciliation};
    let pool = &state.db;
    let (collected, deductible) =
        d300_vat_totals(pool, &company_id, &period_from, &period_to).await?;
    let (cash_vat, _, _) = fetch_cash_vat_flags(pool, &company_id).await?;
    let note = if cash_vat {
        Some(
            "TVA la încasare: divergența față de precompletat (construit pe datele e-Factura, \
             nu pe încasare) este așteptată."
                .to_string(),
        )
    } else {
        None
    };
    let pc_collected =
        Decimal::from_str(precompletat.collected_vat.trim()).unwrap_or(Decimal::ZERO);
    let pc_deductible =
        Decimal::from_str(precompletat.deductible_vat.trim()).unwrap_or(Decimal::ZERO);
    let lines = vec![
        reconcile_line("TVA colectată", collected, pc_collected, note.clone()),
        reconcile_line("TVA deductibilă", deductible, pc_deductible, note.clone()),
        reconcile_line(
            "TVA de plată / de recuperat",
            collected - deductible,
            pc_collected - pc_deductible,
            note,
        ),
    ];
    let any_significant = lines.iter().any(|l| l.significant);
    Ok(EtvaReconciliation {
        period_from,
        period_to,
        lines,
        any_significant,
        cash_vat,
    })
}

/// One JSON file extracted from the e-TVA precompletat zip.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtvaPrecompletatFile {
    pub name: String,
    pub json: String,
}

/// Fetch the RO e-TVA decont precompletat (P300ETVA) from ANAF for a period and return its JSON
/// files. Dedicated service GET {base}/decont/ws/v1/info?cui&an&luna (OAuth2). The exact JSON key
/// names are not publicly documented (only the PDF row model rd.1–36 is), so the raw JSON is
/// returned for the user to read/map into the reconciliation inputs. Live-only (needs ANAF auth).
#[tauri::command]
pub async fn etva_fetch_precompletat(
    state: State<'_, AppState>,
    company_id: String,
    an: i32,
    luna: u32,
    test_mode: bool,
) -> crate::error::AppResult<Vec<EtvaPrecompletatFile>> {
    use crate::anaf::client::{AnafClient, ERR_UNAUTHORIZED};
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    let token =
        crate::commands::anaf::get_valid_token(&company_id, pool, &state.token_refresh_lock)
            .await?;
    let client = AnafClient::new(test_mode);

    let mut res = client
        .fetch_etva_decont(&token, &company.cui, an, luna)
        .await;
    if let Err(ref e) = res {
        if e == ERR_UNAUTHORIZED {
            if let Ok(new_tok) = crate::background::refresh_token_after_401(
                &company_id,
                pool,
                &state.token_refresh_lock,
                &token,
            )
            .await
            {
                res = client
                    .fetch_etva_decont(&new_tok, &company.cui, an, luna)
                    .await;
            }
        }
    }
    let zip = res.map_err(AppError::Other)?;
    let files = crate::anaf_decl::etva::extract_etva_jsons(&zip).map_err(AppError::Other)?;
    Ok(files
        .into_iter()
        .map(|(name, json)| EtvaPrecompletatFile { name, json })
        .collect())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // ── DUK gate logic ────────────────────────────────────────────────────────

    /// duk_gate_allows_write: write is blocked only when DUK is available,
    /// failed, and the user has NOT requested an override.
    #[test]
    fn gate_blocks_when_available_and_failed_without_override() {
        // available=true, passed=false, override=false → blocked
        assert!(!duk_gate_allows_write(true, false, false));
    }

    #[test]
    fn gate_allows_when_duk_not_available() {
        // available=false → always allow (graceful fallback)
        assert!(duk_gate_allows_write(false, false, false));
        assert!(duk_gate_allows_write(false, true, false));
        assert!(duk_gate_allows_write(false, false, true));
    }

    #[test]
    fn gate_allows_when_duk_passed() {
        // available=true, passed=true → allow
        assert!(duk_gate_allows_write(true, true, false));
        assert!(duk_gate_allows_write(true, true, true));
    }

    #[test]
    fn gate_allows_when_override_set() {
        // available=true, passed=false, override=true → allow (user override)
        assert!(duk_gate_allows_write(true, false, true));
    }

    /// Verifică că gruparea după (cotă, categorie) produce rânduri distincte —
    /// același comportament ca BIZ-12 din reports.rs.
    #[test]
    fn d300_groups_split_by_rate_and_category() {
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

        // 19% Standard
        let rate_19 = (Decimal::from(19) * Decimal::from(100))
            .round()
            .to_string()
            .parse::<i64>()
            .unwrap_or(1900);
        let e = groups.entry((rate_19, "S".to_string())).or_insert((
            Decimal::from_str("0.19").unwrap(),
            Decimal::ZERO,
            Decimal::ZERO,
        ));
        e.1 += Decimal::from_str("1000.00").unwrap();
        e.2 += Decimal::from_str("190.00").unwrap();

        // 0% Scutit (E) și 0% Zero-rated (Z) — trebuie să rămână separate
        let rate_0 = 0_i64;
        for (cat, base, vat) in [("E", "200.00", "0.00"), ("Z", "100.00", "0.00")] {
            let e = groups.entry((rate_0, cat.to_string())).or_insert((
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            e.1 += Decimal::from_str(base).unwrap();
            e.2 += Decimal::from_str(vat).unwrap();
        }

        assert_eq!(
            groups.len(),
            3,
            "19%S, 0%E și 0%Z trebuie să fie 3 grupuri distincte"
        );
        assert_eq!(
            groups[&(rate_19, "S".to_string())].1,
            Decimal::from_str("1000.00").unwrap()
        );
        assert_eq!(
            groups[&(rate_0, "E".to_string())].1,
            Decimal::from_str("200.00").unwrap()
        );
        assert_eq!(
            groups[&(rate_0, "Z".to_string())].1,
            Decimal::from_str("100.00").unwrap()
        );
    }

    /// Verifică acumularea exactă Decimal (fără drift float).
    #[test]
    fn d300_decimal_accumulation_exact() {
        let amounts = ["1000.00", "200.50", "350.75"];
        let total: Decimal = amounts.iter().map(|s| Decimal::from_str(s).unwrap()).sum();
        assert_eq!(total, Decimal::from_str("1551.25").unwrap());
    }

    /// Verifică că xml_escape scapă corect caracterele speciale.
    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(xml_escape("RO & SRL <test>"), "RO &amp; SRL &lt;test&gt;");
        assert_eq!(xml_escape("19.00"), "19.00");
        assert_eq!(xml_escape(""), "");
    }

    /// Verifică că build_and_write_xml produce un XML valid cu elementele cerute.
    #[test]
    fn build_xml_contains_required_elements() {
        let report = D300Report {
            company_cui: "RO12345678".to_string(),
            period_from: "2024-01-01".to_string(),
            period_to: "2024-01-31".to_string(),
            groups: vec![D300Group {
                vat_rate: "19.00".to_string(),
                vat_category: "S".to_string(),
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
                intra_eu_kind: None,
            }],
            total_base: "1000.00".to_string(),
            total_vat: "190.00".to_string(),
            invoice_count: 5,
            purchase_groups: vec![D300Group {
                vat_rate: "19.00".to_string(),
                vat_category: "S".to_string(),
                base: "500.00".to_string(),
                vat: "95.00".to_string(),
                intra_eu_kind: None,
            }],
            total_deductible_base: "500.00".to_string(),
            total_deductible_vat: "95.00".to_string(),
            purchase_invoice_count: 3,
            purchase_unparsed_count: 0,
            net_vat: "95.00".to_string(),
            reg_colectata_baza: "0.00".to_string(),
            reg_colectata_tva: "0.00".to_string(),
            reg_dedusa_baza: "0.00".to_string(),
            reg_dedusa_tva: "0.00".to_string(),
            cash_vat_memo: Default::default(),
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d300.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<D300>"));
        assert!(content.contains("<TipDeclaratie>D300</TipDeclaratie>"));
        assert!(content.contains("<CUI>RO12345678</CUI>"));
        assert!(content.contains("<CotaTVA>19.00</CotaTVA>"));
        assert!(content.contains("<TVAColectat>190.00</TVAColectat>"));
        assert!(content.contains("<TotalTVAColectat>190.00</TotalTVAColectat>"));
        assert!(content.contains("<NrFacturiValidate>5</NrFacturiValidate>"));
        assert!(content.contains("<AchizitiiTVADeductibil>"));
        assert!(content.contains("<TVADeductibil>95.00</TVADeductibil>"));
        assert!(content.contains("<TotalTVADeductibil>95.00</TotalTVADeductibil>"));
        assert!(content.contains("<TVADePlata>95.00</TVADePlata>"));

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că nota de facturi neparsate apare în XML când purchase_unparsed_count > 0.
    #[test]
    fn build_xml_includes_unparsed_note_when_needed() {
        let report = D300Report {
            company_cui: "RO11111111".to_string(),
            period_from: "2024-02-01".to_string(),
            period_to: "2024-02-29".to_string(),
            groups: vec![],
            total_base: "0.00".to_string(),
            total_vat: "0.00".to_string(),
            invoice_count: 0,
            purchase_groups: vec![],
            total_deductible_base: "0.00".to_string(),
            total_deductible_vat: "0.00".to_string(),
            purchase_invoice_count: 5,
            purchase_unparsed_count: 3,
            net_vat: "0.00".to_string(),
            reg_colectata_baza: "0.00".to_string(),
            reg_colectata_tva: "0.00".to_string(),
            reg_dedusa_baza: "0.00".to_string(),
            reg_dedusa_tva: "0.00".to_string(),
            cash_vat_memo: Default::default(),
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d300_unparsed.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("3 facturi primite nu au defalcare TVA"),
            "XML trebuie să conțină nota pentru facturi neparsate"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// R4: Verify that a manual deductible override changes total_deductible_vat
    /// and net_vat in the exported XML.
    #[test]
    fn r4_manual_deductible_override_changes_xml() {
        // Build a base report where computed deductible = 95.00.
        let mut report = D300Report {
            company_cui: "RO12345678".to_string(),
            period_from: "2024-01-01".to_string(),
            period_to: "2024-01-31".to_string(),
            groups: vec![D300Group {
                vat_rate: "19.00".to_string(),
                vat_category: "S".to_string(),
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
                intra_eu_kind: None,
            }],
            total_base: "1000.00".to_string(),
            total_vat: "190.00".to_string(),
            invoice_count: 5,
            purchase_groups: vec![],
            total_deductible_base: "500.00".to_string(),
            total_deductible_vat: "95.00".to_string(), // computed
            purchase_invoice_count: 3,
            purchase_unparsed_count: 2,
            net_vat: "95.00".to_string(), // 190 − 95
            reg_colectata_baza: "0.00".to_string(),
            reg_colectata_tva: "0.00".to_string(),
            reg_dedusa_baza: "0.00".to_string(),
            reg_dedusa_tva: "0.00".to_string(),
            cash_vat_memo: Default::default(),
        };

        // Simulate the R4 override logic from export_d300 with override = 120.00.
        let override_str = "120.00";
        if let Ok(override_dec) = Decimal::from_str(override_str.trim()) {
            let total_vat = Decimal::from_str(&report.total_vat).unwrap_or(Decimal::ZERO);
            let net_vat = total_vat - override_dec;
            report.total_deductible_vat = override_dec.round_dp(2).to_string();
            report.net_vat = net_vat.round_dp(2).to_string();
        }

        assert_eq!(
            report.total_deductible_vat, "120.00",
            "Override must replace computed deductible"
        );
        assert_eq!(
            report.net_vat, "70.00",
            "net_vat must be 190.00 - 120.00 = 70.00"
        );

        // Verify the XML contains the overridden values.
        let dir = std::env::temp_dir();
        let path = dir.join("test_d300_r4_override.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(
            result.is_ok(),
            "build_and_write_xml must succeed: {result:?}"
        );

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("<TotalTVADeductibil>120.00</TotalTVADeductibil>"),
            "XML must contain overridden deductible 120.00, got: {}",
            &content[content.find("<AchizitiiTVADeductibil>").unwrap_or(0)..]
        );
        assert!(
            content.contains("<TVADePlata>70.00</TVADePlata>"),
            "XML must contain net_vat 70.00 after override"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// R4: When no override is provided (None), the computed values are used unchanged.
    #[test]
    fn r4_no_override_uses_computed_values() {
        let mut report = D300Report {
            company_cui: "RO12345678".to_string(),
            period_from: "2024-01-01".to_string(),
            period_to: "2024-01-31".to_string(),
            groups: vec![],
            total_base: "0.00".to_string(),
            total_vat: "190.00".to_string(),
            invoice_count: 0,
            purchase_groups: vec![],
            total_deductible_base: "0.00".to_string(),
            total_deductible_vat: "95.00".to_string(),
            purchase_invoice_count: 0,
            purchase_unparsed_count: 0,
            net_vat: "95.00".to_string(),
            reg_colectata_baza: "0.00".to_string(),
            reg_colectata_tva: "0.00".to_string(),
            reg_dedusa_baza: "0.00".to_string(),
            reg_dedusa_tva: "0.00".to_string(),
            cash_vat_memo: Default::default(),
        };

        // Simulate None override — no changes applied.
        let manual_deductible_vat: Option<String> = None;
        if let Some(ref override_str) = manual_deductible_vat {
            if let Ok(override_dec) = Decimal::from_str(override_str.trim()) {
                let total_vat = Decimal::from_str(&report.total_vat).unwrap_or(Decimal::ZERO);
                let net_vat = total_vat - override_dec;
                report.total_deductible_vat = override_dec.round_dp(2).to_string();
                report.net_vat = net_vat.round_dp(2).to_string();
            }
        }

        assert_eq!(
            report.total_deductible_vat, "95.00",
            "Without override, computed deductible must be unchanged"
        );
        assert_eq!(report.net_vat, "95.00");
    }

    // ── Wave 4: FX normalisation ──────────────────────────────────────────────

    /// Wave 4: EUR sales line (base=1000, vat=190, rate=5.0) → 5000/950 RON.
    #[test]
    fn d300_sales_eur_line_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_ron = amount_to_ron(
            Decimal::from_str("1000.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str("190.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        assert_eq!(
            base_ron,
            Decimal::from_str("5000.00").unwrap(),
            "EUR 1000 * 5.0 must equal RON 5000"
        );
        assert_eq!(
            vat_ron,
            Decimal::from_str("950.00").unwrap(),
            "EUR 190 * 5.0 must equal RON 950"
        );
    }

    /// Wave 4: RON sales line is unchanged (amount_to_ron identity for RON).
    #[test]
    fn d300_sales_ron_line_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        assert_eq!(
            amount_to_ron(base, "RON", parse_rate(Some(5.0))),
            base,
            "RON base must be unchanged"
        );
        assert_eq!(
            amount_to_ron(vat, "RON", parse_rate(Some(5.0))),
            vat,
            "RON vat must be unchanged"
        );
    }

    /// Wave 4: EUR purchase line (base=1000, vat=190, rate=5.0) → 5000/950 RON.
    #[test]
    fn d300_purchase_eur_line_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_ron = amount_to_ron(
            Decimal::from_str("1000.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str("190.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        assert_eq!(base_ron, Decimal::from_str("5000.00").unwrap());
        assert_eq!(vat_ron, Decimal::from_str("950.00").unwrap());
    }

    /// Storno-fix: a STORNED original must be included in D300 sales totals —
    /// mirrors the reconciliation with reports.rs/D394/SAF-T (status IN VALIDATED,STORNED).
    ///
    /// Simulates the accumulation logic that runs after the SQL query:
    /// a STORNED invoice line (positive base+vat) is accumulated just like a
    /// VALIDATED line, contributing to the D300 fiscal total.
    #[test]
    fn d300_storned_original_counted_in_sales_total() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};
        use rust_decimal::prelude::ToPrimitive;
        use std::collections::BTreeMap;

        // Simulate two invoices fetched by the "IN ('VALIDATED','STORNED')" query:
        //   1. A VALIDATED invoice: base=1000 RON, vat=190 RON
        //   2. A STORNED original: base=500 RON, vat=95 RON
        //      (its credit note is a separate VALIDATED invoice in the DB —
        //       storno-fix pattern from reports.rs)
        let lines = [
            ("1000.00", "190.00", "RON", None::<f64>), // VALIDATED
            ("500.00", "95.00", "RON", None::<f64>),   // STORNED original
        ];

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();
        for (base_s, vat_s, currency, raw_rate) in &lines {
            let rate_dec = Decimal::from_str("0.19").unwrap();
            let rate_key = (rate_dec * Decimal::from(100))
                .round()
                .to_i64()
                .unwrap_or(0);
            let fx = parse_rate(*raw_rate);
            let base_ron = amount_to_ron(Decimal::from_str(base_s).unwrap(), currency, fx);
            let vat_ron = amount_to_ron(Decimal::from_str(vat_s).unwrap(), currency, fx);
            let e = groups.entry((rate_key, "S".to_string())).or_insert((
                rate_dec,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            e.1 += base_ron;
            e.2 += vat_ron;
        }

        let g = &groups[&(19, "S".to_string())];
        assert_eq!(
            g.1,
            Decimal::from_str("1500.00").unwrap(),
            "D300 must include STORNED original: base should be 1000+500=1500 RON"
        );
        assert_eq!(
            g.2,
            Decimal::from_str("285.00").unwrap(),
            "D300 must include STORNED original: vat should be 190+95=285 RON"
        );
    }

    /// Wave 4: RON purchase line is unchanged.
    #[test]
    fn d300_purchase_ron_line_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        assert_eq!(amount_to_ron(base, "RON", parse_rate(Some(5.0))), base);
        assert_eq!(amount_to_ron(vat, "RON", parse_rate(Some(5.0))), vat);
    }

    /// Wave 4: Mixed EUR+RON accumulation produces correct RON aggregate for D300 sales.
    #[test]
    fn d300_sales_mixed_eur_ron_accumulation() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};
        use rust_decimal::prelude::ToPrimitive;

        // EUR invoice: base=1000, vat=190, rate=5.0 → 5000/950 RON
        // RON invoice: base=1000, vat=190 → 1000/190 RON
        // Total: base=6000, vat=1140
        let lines = [
            ("1000.00", "190.00", "EUR", Some(5.0_f64)),
            ("1000.00", "190.00", "RON", None),
        ];

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();
        for (base_s, vat_s, currency, raw_rate) in &lines {
            let rate_dec = Decimal::from_str("0.19").unwrap();
            let rate_key = (rate_dec * Decimal::from(100))
                .round()
                .to_i64()
                .unwrap_or(0);
            let fx = parse_rate(*raw_rate);
            let base_ron = amount_to_ron(Decimal::from_str(base_s).unwrap(), currency, fx);
            let vat_ron = amount_to_ron(Decimal::from_str(vat_s).unwrap(), currency, fx);
            let e = groups.entry((rate_key, "S".to_string())).or_insert((
                rate_dec,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            e.1 += base_ron;
            e.2 += vat_ron;
        }

        let g = &groups[&(19, "S".to_string())];
        assert_eq!(
            g.1,
            Decimal::from_str("6000.00").unwrap(),
            "Total base must be 5000+1000=6000 RON"
        );
        assert_eq!(
            g.2,
            Decimal::from_str("1140.00").unwrap(),
            "Total vat must be 950+190=1140 RON"
        );
    }

    /// Verifică că net_vat = colectată − deductibilă, inclusiv cazul negativ (TVA de recuperat).
    #[test]
    fn d300_net_vat_can_be_negative() {
        let collected = Decimal::from_str("100.00").unwrap();
        let deductible = Decimal::from_str("150.00").unwrap();
        let net = collected - deductible;
        assert_eq!(net, Decimal::from_str("-50.00").unwrap());
        assert!(
            net.is_sign_negative(),
            "TVA de recuperat trebuie să fie negativă"
        );
    }

    /// Verifică gruparea achiziții pe (rată, categorie) — același pattern ca vânzări.
    #[test]
    fn d300_purchase_groups_split_by_rate_and_category() {
        use rust_decimal::prelude::ToPrimitive;
        use std::collections::BTreeMap;

        let mut purchase_groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> =
            BTreeMap::new();

        // Simulăm 2 linii la 19% S și una la 9% S.
        for (rate_str, cat, base_str, vat_str) in [
            ("0.19", "S", "1000.00", "190.00"),
            ("0.19", "S", "500.00", "95.00"),
            ("0.09", "S", "200.00", "18.00"),
        ] {
            let rate = Decimal::from_str(rate_str).unwrap();
            let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
            let e = purchase_groups
                .entry((rate_key, cat.to_string()))
                .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
            e.1 += Decimal::from_str(base_str).unwrap();
            e.2 += Decimal::from_str(vat_str).unwrap();
        }

        assert_eq!(purchase_groups.len(), 2, "Trebuie 2 grupuri: 19%S și 9%S");

        let rate_19_key = (Decimal::from_str("0.19").unwrap() * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap();
        let rate_9_key = (Decimal::from_str("0.09").unwrap() * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap();

        let g19 = &purchase_groups[&(rate_19_key, "S".to_string())];
        assert_eq!(g19.1, Decimal::from_str("1500.00").unwrap());
        assert_eq!(g19.2, Decimal::from_str("285.00").unwrap());

        let g9 = &purchase_groups[&(rate_9_key, "S".to_string())];
        assert_eq!(g9.1, Decimal::from_str("200.00").unwrap());
        assert_eq!(g9.2, Decimal::from_str("18.00").unwrap());
    }
}

// ── Cash-VAT (TVA la încasare) collection-date routing — DB integration tests ──
#[cfg(test)]
mod cash_vat_routing_tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// Insert a raw 4428 (TVA neexigibilă) GL entry for the memo-balance test.
    #[allow(clippy::too_many_arguments)]
    async fn insert_4428(
        pool: &SqlitePool,
        company: &str,
        jtype: &str,
        date: &str,
        debit: &str,
        credit: &str,
        rate: &str,
    ) {
        let jid = crate::db::models::new_id();
        sqlx::query(
            "INSERT INTO gl_journal (id, company_id, journal_id, journal_type, transaction_id, \
             transaction_date, description, source_type, source_id, customer_id, supplier_id) \
             VALUES (?1,?2,'X',?3,?4,?5,'t',?3,?4,NULL,NULL)",
        )
        .bind(&jid)
        .bind(company)
        .bind(jtype)
        .bind(crate::db::models::new_id())
        .bind(date)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit, \
             tax_type, tax_code, tax_percentage) VALUES (?1,?2,1,'4428',?3,?4,'300','000000',?5)",
        )
        .bind(crate::db::models::new_id())
        .bind(&jid)
        .bind(debit)
        .bind(credit)
        .bind(rate)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cash_vat_memo_balances_split_and_aging() {
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        // Output (sales) deferred VAT: Jun-2026 (in aging) 210 @21% → base 1000; Dec-2025 (out) 105 → 500.
        insert_4428(
            &pool,
            "co",
            "SALES",
            "2026-06-15",
            "0.00",
            "210.00",
            "21.00",
        )
        .await;
        insert_4428(
            &pool,
            "co",
            "SALES",
            "2025-12-15",
            "0.00",
            "105.00",
            "21.00",
        )
        .await;
        // Input (purchase) deferred VAT: Jun-2026, 21 @21% → base 100.
        insert_4428(
            &pool,
            "co",
            "PURCHASE",
            "2026-06-10",
            "21.00",
            "0.00",
            "21.00",
        )
        .await;

        let m = cash_vat_memo_balances(&pool, "co", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        assert_eq!(m.a_vat, 315); // 210 + 105 (closing, all)
        assert_eq!(m.a_base, 1500); // 1000 + 500
        assert_eq!(m.a1_vat, 210); // only Jun-2026 (Dec-2025 is outside the 6-month window)
        assert_eq!(m.a1_base, 1000);
        assert_eq!(m.b_vat, 21);
        assert_eq!(m.b_base, 100);
        assert_eq!(m.b1_vat, 21);
    }

    async fn seed_company(pool: &SqlitePool, id: &str, cash_vat: bool, start: Option<&str>) {
        sqlx::query(
            "INSERT INTO companies \
             (id, cui, legal_name, address, city, county, country, cash_vat, cash_vat_start) \
             VALUES (?1,'12345678','Test SRL','Str 1','Bucuresti','B','RO',?2,?3)",
        )
        .bind(id)
        .bind(cash_vat as i64)
        .bind(start)
        .execute(pool)
        .await
        .expect("insert company");
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name) \
             VALUES (?1,?2,'CUSTOMER','99','Client')",
        )
        .bind(format!("c-{id}"))
        .bind(id)
        .execute(pool)
        .await
        .expect("insert contact");
    }

    // Single 21% "S" line; gross = net + vat.
    async fn seed_invoice(
        pool: &SqlitePool,
        company: &str,
        inv: &str,
        issue: &str,
        net: &str,
        vat: &str,
        gross: &str,
    ) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, issue_date, due_date, \
              currency, subtotal_amount, vat_amount, total_amount, status, payment_means_code, \
              created_at, updated_at) \
             VALUES (?1,?2,?3,?1,1,?1,?4,?4,'RON',?5,?6,?7,'VALIDATED','42',1,1)",
        )
        .bind(inv)
        .bind(company)
        .bind(format!("c-{company}"))
        .bind(issue)
        .bind(net)
        .bind(vat)
        .bind(gross)
        .execute(pool)
        .await
        .expect("insert invoice");
        sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, vat_rate, \
              vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES (?1,?2,'1','P','1','buc',?3,'21','S',?3,?4,?5)",
        )
        .bind(format!("l-{inv}"))
        .bind(inv)
        .bind(net)
        .bind(vat)
        .bind(gross)
        .execute(pool)
        .await
        .expect("insert line");
    }

    async fn seed_payment(
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

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[tokio::test]
    async fn cash_vat_defers_collected_to_payment_period() {
        // 21% invoice (10000 + 2100), issued March, half-paid March, half-paid April.
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "inv1", "2026-03-10", "10000", "2100", "12100").await;
        seed_payment(&pool, "co", "inv1", "p1", "6050", "2026-03-20").await;
        seed_payment(&pool, "co", "inv1", "p2", "6050", "2026-04-15").await;

        let (mar, _) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let (apr, _) = d300_vat_totals(&pool, "co", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(mar, dec("1050"), "March = half the VAT (half collected)");
        assert_eq!(apr, dec("1050"), "April = remaining half (true-up)");
        assert_eq!(mar + apr, dec("2100"), "Σ over periods = invoice VAT");
    }

    #[tokio::test]
    async fn non_cash_vat_company_routes_by_issue_date() {
        // Same invoice/payments, but the company is NOT on cash VAT → unchanged accrual.
        let pool = pool().await;
        seed_company(&pool, "co", false, None).await;
        seed_invoice(&pool, "co", "inv1", "2026-03-10", "10000", "2100", "12100").await;
        seed_payment(&pool, "co", "inv1", "p1", "6050", "2026-03-20").await;
        seed_payment(&pool, "co", "inv1", "p2", "6050", "2026-04-15").await;

        let (mar, _) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let (apr, _) = d300_vat_totals(&pool, "co", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(mar, dec("2100"), "accrual: full VAT in the issue month");
        assert_eq!(apr, Decimal::ZERO, "accrual: nothing in April");
    }

    #[tokio::test]
    async fn cash_vat_pre_window_invoice_stays_accrual() {
        // Invoice issued BEFORE the regime start must keep invoice-date exigibility even
        // though it is collected after adoption (it was already declared at issue date).
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-03-01")).await;
        seed_invoice(&pool, "co", "inv0", "2026-02-10", "10000", "2100", "12100").await;
        seed_payment(&pool, "co", "inv0", "p1", "12100", "2026-03-20").await;

        let (feb, _) = d300_vat_totals(&pool, "co", "2026-02-01", "2026-02-28")
            .await
            .unwrap();
        let (mar, _) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(
            feb,
            dec("2100"),
            "pre-window invoice: full VAT at issue date"
        );
        assert_eq!(
            mar,
            Decimal::ZERO,
            "pre-window invoice: nothing deferred to March"
        );
    }

    #[tokio::test]
    async fn cash_vat_storno_credit_note_reverses_collected() {
        // A cash-VAT "S" sale collected in full, then storno'd via a negative credit note
        // the same month, must net the collected VAT back to zero. The credit note (negative
        // total) stays on the issue-date path so the reversal is not dropped; the positive
        // sale is deferred and re-added on collection. (Proportional-to-collection reversal
        // is slice 5; here the accrual-style reversal must at least be correct.)
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "inv1", "2026-03-10", "10000", "2100", "12100").await;
        seed_payment(&pool, "co", "inv1", "p1", "12100", "2026-03-15").await;
        // Negative credit note (storno) — VALIDATED, negative amounts, no collection.
        seed_invoice(
            &pool,
            "co",
            "cn1",
            "2026-03-25",
            "-10000",
            "-2100",
            "-12100",
        )
        .await;

        let (mar, _) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(
            mar,
            Decimal::ZERO,
            "storno must reverse the collected VAT, not be dropped"
        );
    }

    #[tokio::test]
    async fn cash_vat_storned_original_not_deferred() {
        // A STORNED original is NOT deferred — it follows the same issue-date path as a
        // non-cash-VAT invoice (so GL, which treats STORNED as is_storno, stays consistent).
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "inv1", "2026-03-10", "10000", "2100", "12100").await;
        sqlx::query("UPDATE invoices SET status='STORNED' WHERE id='inv1'")
            .execute(&pool)
            .await
            .unwrap();
        seed_payment(&pool, "co", "inv1", "p1", "12100", "2026-04-15").await;

        let (mar, _) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let (apr, _) = d300_vat_totals(&pool, "co", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(
            mar,
            dec("2100"),
            "STORNED original counted at its issue date"
        );
        assert_eq!(
            apr,
            Decimal::ZERO,
            "STORNED original is not deferred to collection"
        );
    }

    #[tokio::test]
    async fn plafon_status_flags_breach_and_exit_dates() {
        // Cumulative 2026 net crosses 5.000.000 lei in March → exit notificare by 20.04.2026,
        // cash VAT stops after 30.04.2026.
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "i1", "2026-01-15", "2000000", "0", "2000000").await;
        seed_invoice(&pool, "co", "i2", "2026-02-15", "2000000", "0", "2000000").await;
        seed_invoice(&pool, "co", "i3", "2026-03-15", "1500000", "0", "1500000").await;

        let s = compute_plafon_status(&pool, "co", "2026-03-31")
            .await
            .unwrap();
        assert!(s.on_cash_vat);
        assert_eq!(s.plafon_lei, 5_000_000);
        assert_eq!(s.ca_ron, "5500000");
        assert!(s.exceeded);
        assert_eq!(s.breach_month.as_deref(), Some("2026-03"));
        assert_eq!(s.notificare_deadline.as_deref(), Some("2026-04-20"));
        assert_eq!(s.cash_vat_stops_after.as_deref(), Some("2026-04-30"));
    }

    #[tokio::test]
    async fn plafon_status_under_threshold_is_clean() {
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "i1", "2026-02-15", "1000000", "0", "1000000").await;

        let s = compute_plafon_status(&pool, "co", "2026-06-30")
            .await
            .unwrap();
        assert!(!s.exceeded);
        assert_eq!(s.breach_month, None);
        assert_eq!(s.notificare_deadline, None);
        assert_eq!(s.ca_ron, "1000000");
    }

    #[tokio::test]
    async fn intrastat_status_empty_is_ok() {
        // Exercises the dispatch + arrival SQL (catches column-name errors); no intra-EU goods → ok.
        let pool = pool().await;
        seed_company(&pool, "co", false, None).await;
        let s = compute_intrastat_status(&pool, "co", "2026-06-30")
            .await
            .unwrap();
        assert_eq!(s.threshold_ron, "1000000.00");
        assert_eq!(s.dispatches.level, "ok");
        assert_eq!(s.arrivals.level, "ok");
        assert_eq!(s.dispatches.ytd_ron, "0.00");
    }

    #[tokio::test]
    async fn plafon_breach_in_december_rolls_to_next_year() {
        // Breach in Dec 2026 → notificare 2027-01-20, cash VAT stops after 2027-01-31.
        let pool = pool().await;
        seed_company(&pool, "co", true, Some("2026-01-01")).await;
        seed_invoice(&pool, "co", "i1", "2026-12-10", "6000000", "0", "6000000").await;

        let s = compute_plafon_status(&pool, "co", "2026-12-31")
            .await
            .unwrap();
        assert_eq!(s.breach_month.as_deref(), Some("2026-12"));
        assert_eq!(s.notificare_deadline.as_deref(), Some("2027-01-20"));
        assert_eq!(s.cash_vat_stops_after.as_deref(), Some("2027-01-31"));
    }

    // ── Buyer-side (slice 7c) seeders + tests ────────────────────────────────
    async fn seed_cash_vat_supplier(pool: &SqlitePool, company: &str, cui: &str) {
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name, cash_vat) \
             VALUES (?1,?2,'SUPPLIER',?3,'Furnizor TI',1)",
        )
        .bind(format!("sup-{cui}"))
        .bind(company)
        .bind(cui)
        .execute(pool)
        .await
        .expect("insert supplier");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_received(
        pool: &SqlitePool,
        company: &str,
        rid: &str,
        issuer_cui: &str,
        issue: &str,
        net: &str,
        vat: &str,
        gross: &str,
    ) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status) \
             VALUES (?1,?2,?1,?3,'Furnizor',?4,'RON',?5,'/x.xml','REVIEWED')",
        )
        .bind(rid)
        .bind(company)
        .bind(issuer_cui)
        .bind(gross)
        .bind(issue)
        .execute(pool)
        .await
        .expect("insert received");
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines \
             (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES (?1,?2,'21','S',?3,?4)",
        )
        .bind(format!("vl-{rid}"))
        .bind(rid)
        .bind(net)
        .bind(vat)
        .execute(pool)
        .await
        .expect("insert vat line");
    }

    async fn seed_received_payment(
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
    async fn buyer_side_supplier_triggered_defers_deductible() {
        // Buyer NOT on cash VAT, but the supplier (matched by CUI) is → deduction defers to
        // the supplier-payment date (art. 297(2)).
        let pool = pool().await;
        seed_company(&pool, "co", false, None).await; // buyer not on cash VAT
        seed_cash_vat_supplier(&pool, "co", "RO99").await;
        seed_received(
            &pool,
            "co",
            "ri1",
            "RO99",
            "2026-03-10",
            "10000",
            "2100",
            "12100",
        )
        .await;
        seed_received_payment(&pool, "co", "ri1", "rp1", "6050", "2026-03-20").await;
        seed_received_payment(&pool, "co", "ri1", "rp2", "6050", "2026-04-15").await;

        let (_, mar) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let (_, apr) = d300_vat_totals(&pool, "co", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(mar, dec("1050"), "March = half the input VAT (half paid)");
        assert_eq!(apr, dec("1050"), "April = remaining half (true-up)");
        assert_eq!(
            mar + apr,
            dec("2100"),
            "Σ deductible over periods = invoice VAT"
        );
    }

    #[tokio::test]
    async fn buyer_side_inactive_keeps_issue_date_deduction() {
        // No cash-VAT supplier, buyer not on cash VAT → deduct at the invoice date as before.
        let pool = pool().await;
        seed_company(&pool, "co", false, None).await;
        seed_received(
            &pool,
            "co",
            "ri1",
            "RO77",
            "2026-03-10",
            "10000",
            "2100",
            "12100",
        )
        .await;
        seed_received_payment(&pool, "co", "ri1", "rp1", "12100", "2026-04-15").await;

        let (_, mar) = d300_vat_totals(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        let (_, apr) = d300_vat_totals(&pool, "co", "2026-04-01", "2026-04-30")
            .await
            .unwrap();
        assert_eq!(mar, dec("2100"), "normal: full deduction at invoice date");
        assert_eq!(apr, Decimal::ZERO, "nothing deferred to the payment month");
    }
}

#[cfg(test)]
mod test_ron_to_bani_overflow {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_overflow_scenarios() {
        // i64::MAX = 9,223,372,036,854,775,807
        // Safe RON threshold = i64::MAX / 100 = 92,233,720,368,547.75807

        // Test: Small value (should work)
        let small = Decimal::from_str("1000.50").unwrap();
        let result = ron_to_bani(small);
        assert_eq!(result, 100050, "Small value should convert correctly");

        // Test: The value from the finding (922,337,203,685,477.58)
        // This is actually well below the safe threshold!
        let finding_val = Decimal::from_str("922337203685477.58").unwrap();
        let result = ron_to_bani(finding_val);
        // This will NOT overflow; result should be 92233720368547758
        println!("Finding value result: {}", result);
        assert!(result > 0, "Finding value should not silently overflow");

        // Test: Actual overflow (over 92,233,720,368,547.75807 RON)
        let overflow_val = Decimal::from_str("92233720368548.00").unwrap();
        let result = ron_to_bani(overflow_val);
        println!(
            "Overflow value (92,233,720,368,548.00 RON) result: {}",
            result
        );
        // unwrap_or(0) will return 0 on overflow

        // Test: Extreme but realistic N(15,2) max (9,999,999,999,999.99)
        let n15_max = Decimal::from_str("9999999999999.99").unwrap();
        let result = ron_to_bani(n15_max);
        assert!(result > 0, "N(15,2) max should convert correctly");
        println!("N(15,2) max result: {}", result);
    }
}

// ─── Istoricul depunerilor ────────────────────────────────────────────────────

/// Listează depunerile înregistrate pentru o firmă (cele mai recente primele).
#[tauri::command]
pub async fn list_declaration_filings(
    state: State<'_, crate::state::AppState>,
    company_id: String,
) -> crate::error::AppResult<Vec<crate::db::declaration_filings::Filing>> {
    crate::db::declaration_filings::list(&state.db, &company_id).await
}

/// Șterge o depunere din istoric (company-scoped: nu poate șterge rândul altei firme).
#[tauri::command]
pub async fn delete_declaration_filing(
    state: State<'_, crate::state::AppState>,
    id: String,
    company_id: String,
) -> crate::error::AppResult<()> {
    crate::db::declaration_filings::delete(&state.db, &id, &company_id).await
}
