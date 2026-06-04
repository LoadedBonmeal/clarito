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

use crate::anaf_decl::d300::D300Submission;
use crate::anaf_decl::version::resolve;
use crate::anaf_decl::DeclKind;
use crate::db::companies;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

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
}

// ── Commands ──────────────────────────────────────────────────────────────────

// ── Shared D300 VAT core (used by compute_d300 + gl::reconcile) ──────────────

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
    // ── TVA colectată: Σvat_amount din liniile facturilor emise (VALIDATED+STORNED) ──
    let sales_rows = sqlx::query(
        "SELECT l.vat_amount, COALESCE(i.currency,'RON') as currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
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

    // ── TVA deductibilă: Σvat_amount din received_invoice_vat_lines ──
    let purch_rows = sqlx::query(
        "SELECT vl.vat_amount, vl.vat_category, COALESCE(ri.currency,'RON') as currency, ri.exchange_rate \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
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

    Ok((collected.round_dp(2), deductible.round_dp(2)))
}

/// Calculează decontul D300 (TVA colectat — vânzări + TVA deductibil — achiziții)
/// pentru o companie și o perioadă.
///
/// **Vânzări**: facturile cu status VALIDATED sau STORNED (setul fiscal autorizat
/// BIZ-11/storno-fix), identic cu rapoartele TVA, jurnalele, D394 și SAF-T,
/// astfel încât D300 reconciliază cu celelalte declarații.
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
    let line_rows = sqlx::query(
        "SELECT l.vat_rate, l.vat_category, l.subtotal_amount, l.vat_amount, \
                COALESCE(i.currency, 'RON') AS currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND i.company_id = ?3",
    )
    .bind(&period_from)
    .bind(&period_to)
    .bind(&company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

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
                vat_rate: rate.round_dp(2).to_string(),
                vat_category: category,
                base: base_sum.round_dp(2).to_string(),
                vat: vat_sum.round_dp(2).to_string(),
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
    let purchase_line_rows = sqlx::query(
        "SELECT vl.vat_rate, vl.vat_category, vl.base_amount, vl.vat_amount, \
                COALESCE(ri.currency, 'RON') AS currency, ri.exchange_rate, \
                ri.intra_eu_kind \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED'",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

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
                vat_rate: rate.round_dp(2).to_string(),
                vat_category: category,
                base: base_sum.round_dp(2).to_string(),
                vat: vat_sum.round_dp(2).to_string(),
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
                rate_d.round_dp(0).to_i64().unwrap_or(-1)
            } else {
                (rate_d * Decimal::from(100))
                    .round_dp(0)
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
                rate_d.round_dp(0).to_i64().unwrap_or(-1)
            } else {
                (rate_d * Decimal::from(100))
                    .round_dp(0)
                    .to_i64()
                    .unwrap_or(-1)
            };
            if matches!(rate_pct, 19 | 9 | 5) {
                reg_ded_base += Decimal::from_str(&g.base).unwrap_or(Decimal::ZERO);
                reg_ded_tva += Decimal::from_str(&g.vat).unwrap_or(Decimal::ZERO);
            }
        }
    }

    Ok(D300Report {
        company_cui,
        period_from,
        period_to,
        groups: groups_vec,
        total_base: total_base.round_dp(2).to_string(),
        total_vat: total_vat.round_dp(2).to_string(),
        invoice_count,
        purchase_groups: purchase_groups_vec,
        total_deductible_base: total_deductible_base.round_dp(2).to_string(),
        total_deductible_vat: total_deductible_vat.round_dp(2).to_string(),
        purchase_invoice_count,
        purchase_unparsed_count,
        net_vat: net_vat.round_dp(2).to_string(),
        reg_colectata_baza: reg_coll_base.round_dp(2).to_string(),
        reg_colectata_tva: reg_coll_tva.round_dp(2).to_string(),
        reg_dedusa_baza: reg_ded_base.round_dp(2).to_string(),
        reg_dedusa_tva: reg_ded_tva.round_dp(2).to_string(),
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
            report.total_deductible_vat = override_dec.round_dp(2).to_string();
            report.net_vat = net_vat.round_dp(2).to_string();
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

    std::fs::write(&dest_path, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;

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
    Ok(OfficialExportResult {
        path: dest,
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
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

/// Escapes XML special characters in a string value.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
