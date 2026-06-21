//! Reevaluare valutară lunară (OMFP 1802/2014 art. 304(3) + art. 322).
//!
//! ## Baza legală
//!
//! Art. 304(3): elementele monetare exprimate în valută se evaluează la cursul de schimb
//! comunicat de BNR valabil la data închiderii exercițiului financiar.
//! Art. 322: la finele fiecărei luni, creanțele și datoriile în valută se evaluează la
//! cursul BNR din **ultima zi lucrătoare** a lunii.
//!
//! ## Baza reevaluării (art. 322 alin. 3)
//!
//! Reevaluarea lunară se face față de **valoarea Lei din luna anterioară** (nu față de
//! cursul de booking inițial). Dacă luna anterioară nu există, se folosește cursul de
//! booking (exchange_rate din factură). Ignorarea acestei reguli face ca eroarea să se
//! cumuleze luni de-a rândul.
//!
//! ## Conturi utilizate (consistente cu 665/765 din post_payment)
//!
//! - Creanță (4111) curs ↑ → D 4111 / C 765 (favorabil)
//! - Creanță (4111) curs ↓ → D 665  / C 4111 (nefavorabil)
//! - Datorie (401)  curs ↑ → D 665  / C 401  (nefavorabil)
//! - Datorie (401)  curs ↓ → D 401  / C 765  (favorabil)
//!
//! Notă analitică: 6651/7651 pot fi folosite ca analitice; în comentarii unde relevant.
//!
//! ## Idempotență
//!
//! Re-rularea pentru aceeași (company_id, period) șterge + reinserează rândurile din
//! `fx_revaluation` și înlocuiește nota GL via `post_manual_journal` (source_type='FX_REVAL').
//! `generate_gl_entries` nu șterge 'FX_REVAL' — nota supraviețuiește regenerărilor.
//!
//! ## Deferred
//!
//! - Reevaluarea trezoreriei (5124/5314): necesită un tabel de solduri bancare cu valută.
//! - Ultima zi bancară BNR: dacă feed-ul nu are cursul exact pe acea zi, `parse_bnr_rate`
//!   alege cel mai recent Cube cu date ≤ target — comportament corect pentru zile fără publicare.
//! - Facturi cu status DRAFT: excluse din reevaluare (numai VALIDATED/STORNED).

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::commands::bnr::parse_bnr_rate;
use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::invoices::round2;
use crate::db::models::new_id;
use crate::error::{AppError, AppResult};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Rotunjire la 4 zecimale (cursuri BNR).
fn round4(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(4, RoundingStrategy::MidpointAwayFromZero)
}

/// Parsează un TEXT Decimal din DB; returnează ZERO la eroare cu log.
fn parse_dec(label: &str, s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or_else(|e| {
        tracing::warn!("fx_revaluation: parse_dec({label}) failed for {:?}: {e}", s);
        Decimal::ZERO
    })
}

/// Calculează ultima zi bancară din luna `period` ("YYYY-MM") — primul weekday
/// ≤ ultima zi calendaristică. Returnează "YYYY-MM-DD".
pub(crate) fn last_banking_day(period: &str) -> AppResult<String> {
    // Parsăm "YYYY-MM"
    let parts: Vec<&str> = period.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(AppError::Validation(format!(
            "Perioadă invalidă «{period}» — formatul așteptat este YYYY-MM."
        )));
    }
    let year: u32 = parts[0]
        .parse()
        .map_err(|_| AppError::Validation(format!("Anul din perioadă invalid: «{}»", parts[0])))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| AppError::Validation(format!("Luna din perioadă invalidă: «{}»", parts[1])))?;
    if !matches!(month, 1..=12) || !matches!(year, 1900..=2100) {
        return Err(AppError::Validation(format!(
            "Perioadă în afara intervalului valid: «{period}»"
        )));
    }

    // Ultima zi calendaristică a lunii (next-month-day-1 minus 1 day).
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1u32)
    } else {
        (year, month + 1)
    };

    // Calculăm ultima zi a lunii iterând de la 28 în sus.
    // Simplu și sigur fără dependență de chrono.
    let last_day = {
        let days_in_month = [0u32, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let base = days_in_month[month as usize];
        // Bisect
        if month == 2 {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29u32
            } else {
                28u32
            }
        } else {
            base
        }
    };
    let _ = next_year; // suppress unused
    let _ = next_month;

    // Mergem de la last_day înapoi până găsim un zi lucrătoare (Mon-Fri).
    // Ziua săptămânii via algoritmul Tomohiko Sakamoto (0=Duminică … 6=Sâmbătă).
    fn weekday(y: u32, m: u32, d: u32) -> u32 {
        // Sakamoto's algorithm
        let t: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if m < 3 { y - 1 } else { y };
        (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d) % 7
    }

    let mut day = last_day;
    loop {
        let wd = weekday(year, month, day);
        // 0=Sun, 6=Sat
        if wd != 0 && wd != 6 {
            break;
        }
        if day == 1 {
            return Err(AppError::Other(format!(
                "Nu s-a găsit nicio zi bancară în luna {period}"
            )));
        }
        day -= 1;
    }

    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

// ─── Models ───────────────────────────────────────────────────────────────────

/// O linie de reevaluare per factură (returnată de `list_fx_revaluations`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxRevaluationRow {
    pub id: String,
    pub company_id: String,
    pub period: String,
    pub invoice_id: String,
    pub invoice_kind: String,
    pub currency: String,
    pub foreign_outstanding: String,
    pub month_end_rate: String,
    pub prior_rate: String,
    pub revalued_lei: String,
    pub prior_lei: String,
    pub diff_lei: String,
    pub created_at: i64,
}

/// Rezultatul rulării `compute_fx_revaluation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxRevaluationResult {
    /// Perioada reevaluată ("YYYY-MM").
    pub period: String,
    /// Număr de facturi reevaluate cu diff ≠ 0.
    pub rows_posted: usize,
    /// Diferențe totale favorabile (lei) — C 765.
    pub total_favorable: String,
    /// Diferențe totale nefavorabile (lei) — D 665.
    pub total_unfavorable: String,
    /// Diferența netă (revalued_lei − prior_lei), pozitivă = venit net.
    pub net_diff: String,
    /// Nota GL postată (source_id = "FX_REVAL-{period}").
    pub gl_source_id: String,
    /// Ultima zi bancară folosită pentru curs.
    pub month_end_date: String,
}

// ─── Core logic ───────────────────────────────────────────────────────────────

/// Returnează cursul anterior de reevaluare pentru o factură.
/// = ultima `month_end_rate` din `fx_revaluation` pentru această factură cu period < `current_period`,
/// sau `booking_rate` dacă nu există nicio reevaluare anterioară.
async fn get_prior_rate(
    pool: &SqlitePool,
    company_id: &str,
    invoice_id: &str,
    invoice_kind: &str,
    current_period: &str,
    booking_rate: Decimal,
) -> Decimal {
    let r: Option<String> = sqlx::query_scalar(
        "SELECT month_end_rate FROM fx_revaluation \
         WHERE company_id = ?1 AND invoice_id = ?2 AND invoice_kind = ?3 \
           AND period < ?4 \
         ORDER BY period DESC LIMIT 1",
    )
    .bind(company_id)
    .bind(invoice_id)
    .bind(invoice_kind)
    .bind(current_period)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    r.as_deref()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or(booking_rate)
}

/// Calculează și postează reevaluarea valutară pentru perioada `period` ("YYYY-MM").
///
/// 1. Rezolvă ultima zi bancară; fetch cursuri BNR pentru fiecare valută.
/// 2. Iterează facturile emise/primite ne-RON cu sold deschis > 0.01.
/// 3. Calculează `prior_rate` din ultima reevaluare sau booking rate.
/// 4. Postează nota GL (source_type='FX_REVAL', source_id="FX_REVAL-{period}").
/// 5. Upsert rânduri în `fx_revaluation`.
///
/// Idempotentă: re-rularea înlocuiește nota + rândurile existente.
pub async fn compute_fx_revaluation(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
    // BNR XML-uri injectate în teste (None = fetch real din rețea — nu testat)
    bnr_xml_override: Option<&str>,
) -> AppResult<FxRevaluationResult> {
    // ── 1. Ultima zi bancară + cursuri BNR ────────────────────────────────────
    let month_end_date = last_banking_day(period)?;

    // Toate valutele non-RON din facturile deschise ale companiei.
    let currencies: Vec<String> = {
        // Facturi emise
        let issued: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT UPPER(TRIM(currency)) \
             FROM invoices \
             WHERE company_id = ?1 \
               AND status IN ('VALIDATED','STORNED') \
               AND currency IS NOT NULL \
               AND UPPER(TRIM(currency)) != 'RON' \
               AND UPPER(TRIM(currency)) != ''",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;
        // Facturi primite
        let received: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT UPPER(TRIM(currency)) \
             FROM received_invoices \
             WHERE company_id = ?1 \
               AND currency IS NOT NULL \
               AND UPPER(TRIM(currency)) != 'RON' \
               AND UPPER(TRIM(currency)) != ''",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        let mut all: Vec<String> = issued;
        for c in received {
            if !all.contains(&c) {
                all.push(c);
            }
        }
        all
    };

    // Fetch cursuri BNR (Decimal, 4 zecimale) per valută.
    use std::collections::HashMap;
    let mut rates: HashMap<String, Decimal> = HashMap::new();
    for cur in &currencies {
        let rate = fetch_bnr_rate_decimal(cur, &month_end_date, bnr_xml_override).await?;
        rates.insert(cur.clone(), rate);
    }

    // ── 2. Facturi emise cu sold deschis ─────────────────────────────────────
    // Perioada emisiei ≤ ultima zi a perioadei de reevaluare (factura trebuie să existe).
    let period_end = format!("{}-31", period); // lexicographic upper bound
    let issued_rows = sqlx::query(
        "SELECT i.id, UPPER(TRIM(i.currency)) as currency, \
                CAST(i.exchange_rate AS TEXT) as exchange_rate, \
                i.total_amount \
         FROM invoices i \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED','STORNED') \
           AND UPPER(TRIM(i.currency)) != 'RON' \
           AND i.currency IS NOT NULL \
           AND i.issue_date <= ?2",
    )
    .bind(company_id)
    .bind(&period_end)
    .fetch_all(pool)
    .await?;

    // ── 3. Facturi primite cu sold deschis ────────────────────────────────────
    let received_rows = sqlx::query(
        "SELECT r.id, UPPER(TRIM(r.currency)) as currency, \
                CAST(r.exchange_rate AS TEXT) as exchange_rate, \
                r.total_amount \
         FROM received_invoices r \
         WHERE r.company_id = ?1 \
           AND UPPER(TRIM(r.currency)) != 'RON' \
           AND r.currency IS NOT NULL \
           AND r.issue_date <= ?2",
    )
    .bind(company_id)
    .bind(&period_end)
    .fetch_all(pool)
    .await?;

    // Suma plăților per factură emisă (în valuta facturii — payments.amount).
    // Notă: payments.amount este în valuta facturii (nu RON), confirmat de payments.rs.
    let issued_paid: HashMap<String, Decimal> = {
        let rows = sqlx::query(
            "SELECT p.invoice_id, p.amount \
             FROM payments p \
             INNER JOIN invoices i ON i.id = p.invoice_id \
             WHERE i.company_id = ?1 \
               AND i.status IN ('VALIDATED','STORNED') \
               AND UPPER(TRIM(i.currency)) != 'RON'",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;
        let mut m: HashMap<String, Decimal> = HashMap::new();
        for r in rows {
            let inv_id: String = r.try_get("invoice_id").unwrap_or_default();
            let amt_s: String = r.try_get("amount").unwrap_or_else(|_| "0".to_string());
            let amt = parse_dec("issued_paid.amount", &amt_s);
            *m.entry(inv_id).or_default() += amt;
        }
        m
    };

    // Suma plăților per factură primită.
    // Tabelul se numește `received_invoice_payments` (migration 0027).
    let received_paid: HashMap<String, Decimal> = {
        let rows = sqlx::query(
            "SELECT p.received_invoice_id, p.amount \
             FROM received_invoice_payments p \
             INNER JOIN received_invoices r ON r.id = p.received_invoice_id \
             WHERE r.company_id = ?1 \
               AND UPPER(TRIM(r.currency)) != 'RON'",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;
        let mut m: HashMap<String, Decimal> = HashMap::new();
        for r in rows {
            let inv_id: String = r.try_get("received_invoice_id").unwrap_or_default();
            let amt_s: String = r.try_get("amount").unwrap_or_else(|_| "0".to_string());
            let amt = parse_dec("received_paid.amount", &amt_s);
            *m.entry(inv_id).or_default() += amt;
        }
        m
    };

    // ── 4. Compute diff per invoice ───────────────────────────────────────────

    struct RevalLine {
        invoice_id: String,
        invoice_kind: &'static str,
        currency: String,
        foreign_outstanding: Decimal,
        month_end_rate: Decimal,
        prior_rate: Decimal,
        revalued_lei: Decimal,
        prior_lei: Decimal,
        diff_lei: Decimal,
    }

    let mut lines: Vec<RevalLine> = Vec::new();

    // Facturi emise
    for row in &issued_rows {
        let inv_id: String = row.try_get("id").unwrap_or_default();
        let currency: String = row.try_get("currency").unwrap_or_default();

        let month_end_rate = match rates.get(&currency) {
            Some(&r) => r,
            None => continue, // valuta fără curs BNR — skip
        };

        let total_s: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let foreign_total = parse_dec("issued.total_amount", &total_s);

        let paid = issued_paid.get(&inv_id).copied().unwrap_or(Decimal::ZERO);
        let foreign_outstanding = round2(foreign_total - paid);

        // Skip dacă soldul e ≤ 0.01
        if foreign_outstanding <= Decimal::new(1, 2) {
            continue;
        }

        // Booking rate (exchange_rate REAL → TEXT via SQL CAST)
        let booking_rate_s: String = row
            .try_get("exchange_rate")
            .unwrap_or_else(|_| "0".to_string());
        let booking_rate = parse_dec("issued.exchange_rate", &booking_rate_s);
        // Dacă booking_rate = 0 (RON de fapt), skip
        if booking_rate <= Decimal::ZERO {
            continue;
        }

        let prior_rate =
            get_prior_rate(pool, company_id, &inv_id, "ISSUED", period, booking_rate).await;
        let prior_lei = round2(foreign_outstanding * prior_rate);
        let revalued_lei = round2(foreign_outstanding * month_end_rate);
        let diff_lei = revalued_lei - prior_lei;

        // Skip dacă diferența e neglijabilă (< 0.01 RON)
        if diff_lei.abs() < Decimal::new(1, 2) {
            continue;
        }

        lines.push(RevalLine {
            invoice_id: inv_id,
            invoice_kind: "ISSUED",
            currency,
            foreign_outstanding,
            month_end_rate,
            prior_rate,
            revalued_lei,
            prior_lei,
            diff_lei,
        });
    }

    // Facturi primite
    for row in &received_rows {
        let inv_id: String = row.try_get("id").unwrap_or_default();
        let currency: String = row.try_get("currency").unwrap_or_default();

        let month_end_rate = match rates.get(&currency) {
            Some(&r) => r,
            None => continue,
        };

        let total_s: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let foreign_total = parse_dec("received.total_amount", &total_s);

        let paid = received_paid.get(&inv_id).copied().unwrap_or(Decimal::ZERO);
        let foreign_outstanding = round2(foreign_total - paid);

        if foreign_outstanding <= Decimal::new(1, 2) {
            continue;
        }

        let booking_rate_s: String = row
            .try_get("exchange_rate")
            .unwrap_or_else(|_| "0".to_string());
        let booking_rate = parse_dec("received.exchange_rate", &booking_rate_s);
        if booking_rate <= Decimal::ZERO {
            continue;
        }

        let prior_rate =
            get_prior_rate(pool, company_id, &inv_id, "RECEIVED", period, booking_rate).await;
        let prior_lei = round2(foreign_outstanding * prior_rate);
        let revalued_lei = round2(foreign_outstanding * month_end_rate);
        let diff_lei = revalued_lei - prior_lei;

        if diff_lei.abs() < Decimal::new(1, 2) {
            continue;
        }

        lines.push(RevalLine {
            invoice_id: inv_id,
            invoice_kind: "RECEIVED",
            currency,
            foreign_outstanding,
            month_end_rate,
            prior_rate,
            revalued_lei,
            prior_lei,
            diff_lei,
        });
    }

    // ── 5. Construiește nota GL ────────────────────────────────────────────────
    //
    // Netting per (account, currency) nu este obligatoriu — postăm NET per sens:
    // net 4111 adj, net 401 adj, net 665, net 765.
    // Notă: post_manual_journal verifică echilibrul; dacă liniile sunt goale, nu postăm.

    // Acumulatori pentru notă GL
    let mut d_4111 = Decimal::ZERO; // D 4111 (creanță curs ↑)
    let mut c_4111 = Decimal::ZERO; // C 4111 (creanță curs ↓) — CREDIT
    let mut d_401 = Decimal::ZERO; // D 401  (datorie curs ↓)
    let mut c_401 = Decimal::ZERO; // C 401  (datorie curs ↑) — CREDIT
    let mut d_665 = Decimal::ZERO; // D 665  cheltuială
    let mut c_765 = Decimal::ZERO; // C 765  venit

    let mut total_favorable = Decimal::ZERO;
    let mut total_unfavorable = Decimal::ZERO;

    for line in &lines {
        match (line.invoice_kind, line.diff_lei > Decimal::ZERO) {
            // Creanță (4111), curs ↑ → diff > 0 → D 4111 / C 765 (favorabil)
            ("ISSUED", true) => {
                d_4111 += line.diff_lei;
                c_765 += line.diff_lei;
                total_favorable += line.diff_lei;
            }
            // Creanță (4111), curs ↓ → diff < 0 → D 665 / C 4111 (nefavorabil)
            ("ISSUED", false) => {
                let abs = line.diff_lei.abs();
                d_665 += abs;
                c_4111 += abs;
                total_unfavorable += abs;
            }
            // Datorie (401), curs ↑ → diff > 0 → D 665 / C 401 (nefavorabil)
            ("RECEIVED", true) => {
                d_665 += line.diff_lei;
                c_401 += line.diff_lei;
                total_unfavorable += line.diff_lei;
            }
            // Datorie (401), curs ↓ → diff < 0 → D 401 / C 765 (favorabil)
            ("RECEIVED", false) => {
                let abs = line.diff_lei.abs();
                d_401 += abs;
                c_765 += abs;
                total_favorable += abs;
            }
            _ => {}
        }
    }

    let rows_posted = lines.len();
    let net_diff = total_favorable - total_unfavorable;
    let gl_source_id = format!("FX_REVAL-{period}");
    let gl_date = month_end_date.clone();
    let gl_desc = format!("Reevaluare valutară {period} — OMFP 1802/2014 art. 322");

    if rows_posted > 0 {
        // Construiește liniile notei GL (doar cele cu sumă > 0)
        let mut gl_lines: Vec<(&'static str, Decimal, Decimal)> = Vec::new();

        if d_4111 > Decimal::ZERO {
            gl_lines.push(("4111", d_4111, Decimal::ZERO));
        }
        if c_4111 > Decimal::ZERO {
            gl_lines.push(("4111", Decimal::ZERO, c_4111));
        }
        if d_401 > Decimal::ZERO {
            gl_lines.push(("401", d_401, Decimal::ZERO));
        }
        if c_401 > Decimal::ZERO {
            gl_lines.push(("401", Decimal::ZERO, c_401));
        }
        if d_665 > Decimal::ZERO {
            gl_lines.push(("665", d_665, Decimal::ZERO));
        }
        if c_765 > Decimal::ZERO {
            gl_lines.push(("765", Decimal::ZERO, c_765));
        }

        post_manual_journal(
            pool,
            &ManualJournal {
                company_id,
                journal_id: "RV",
                journal_type: "FX_REVAL",
                source_type: "FX_REVAL",
                source_id: &gl_source_id,
                date: &gl_date,
                description: &gl_desc,
                partner_cui: None,
            },
            &gl_lines,
        )
        .await?;
    }

    // ── 6. Upsert fx_revaluation rows ────────────────────────────────────────
    // Ștergem rândurile existente pentru această perioadă (idempotent), reinserăm.
    sqlx::query("DELETE FROM fx_revaluation WHERE company_id = ?1 AND period = ?2")
        .bind(company_id)
        .bind(period)
        .execute(pool)
        .await?;

    for line in &lines {
        let row_id = new_id();
        sqlx::query(
            "INSERT INTO fx_revaluation \
             (id, company_id, period, invoice_id, invoice_kind, currency, \
              foreign_outstanding, month_end_rate, prior_rate, \
              revalued_lei, prior_lei, diff_lei) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(&row_id)
        .bind(company_id)
        .bind(period)
        .bind(&line.invoice_id)
        .bind(line.invoice_kind)
        .bind(&line.currency)
        .bind(line.foreign_outstanding.to_string())
        .bind(line.month_end_rate.to_string())
        .bind(line.prior_rate.to_string())
        .bind(line.revalued_lei.to_string())
        .bind(line.prior_lei.to_string())
        .bind(line.diff_lei.to_string())
        .execute(pool)
        .await?;
    }

    Ok(FxRevaluationResult {
        period: period.to_string(),
        rows_posted,
        total_favorable: total_favorable.to_string(),
        total_unfavorable: total_unfavorable.to_string(),
        net_diff: net_diff.to_string(),
        gl_source_id,
        month_end_date,
    })
}

/// Listează rândurile de reevaluare pentru o perioadă.
pub async fn list_fx_revaluations(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<Vec<FxRevaluationRow>> {
    let rows = sqlx::query(
        "SELECT id, company_id, period, invoice_id, invoice_kind, currency, \
                foreign_outstanding, month_end_rate, prior_rate, \
                revalued_lei, prior_lei, diff_lei, created_at \
         FROM fx_revaluation \
         WHERE company_id = ?1 AND period = ?2 \
         ORDER BY invoice_kind, currency, invoice_id",
    )
    .bind(company_id)
    .bind(period)
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for r in rows {
        result.push(FxRevaluationRow {
            id: r.try_get("id").unwrap_or_default(),
            company_id: r.try_get("company_id").unwrap_or_default(),
            period: r.try_get("period").unwrap_or_default(),
            invoice_id: r.try_get("invoice_id").unwrap_or_default(),
            invoice_kind: r.try_get("invoice_kind").unwrap_or_default(),
            currency: r.try_get("currency").unwrap_or_default(),
            foreign_outstanding: r.try_get("foreign_outstanding").unwrap_or_default(),
            month_end_rate: r.try_get("month_end_rate").unwrap_or_default(),
            prior_rate: r.try_get("prior_rate").unwrap_or_default(),
            revalued_lei: r.try_get("revalued_lei").unwrap_or_default(),
            prior_lei: r.try_get("prior_lei").unwrap_or_default(),
            diff_lei: r.try_get("diff_lei").unwrap_or_default(),
            created_at: r.try_get("created_at").unwrap_or(0),
        });
    }
    Ok(result)
}

// ─── BNR rate fetch (Decimal, testabil) ──────────────────────────────────────

/// Fetch cursul BNR ca Decimal. Injectare XML pentru teste (bnr_xml_override).
pub(crate) async fn fetch_bnr_rate_decimal(
    currency: &str,
    date: &str,
    xml_override: Option<&str>,
) -> AppResult<Decimal> {
    if currency.eq_ignore_ascii_case("RON") {
        return Ok(Decimal::ONE);
    }

    if let Some(xml) = xml_override {
        return parse_bnr_rate(xml, currency, date).ok_or_else(|| {
            AppError::Validation(format!(
                "Cursul BNR pentru {currency} la {date} nu a fost găsit în XML-ul furnizat"
            ))
        });
    }

    // Producție: fetch real (doi pași ca în fetch_bnr_rate)
    use reqwest::Client;
    use std::time::Duration;
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Other(format!("Eroare client HTTP BNR: {e}")))?;

    // Pasul 1: feed zilnic
    let daily_url = "https://www.bnr.ro/nbrfxrates.xml";
    if let Ok(xml) = fetch_xml_str(&client, daily_url).await {
        if let Some(r) = parse_bnr_rate(&xml, currency, date) {
            return Ok(round4(r));
        }
    }

    // Pasul 2: fișier anual
    let year_str = date.get(..4).ok_or_else(|| {
        AppError::Validation(format!("Data '{date}' nu este în format YYYY-MM-DD"))
    })?;
    if !year_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(format!(
            "Anul '{year_str}' din data '{date}' nu este valid"
        )));
    }
    let year_url = format!("https://www.bnr.ro/files/xml/years/nbrfx{year_str}.xml");
    let xml = fetch_xml_str(&client, &year_url).await?;
    parse_bnr_rate(&xml, currency, date).ok_or_else(|| {
        AppError::Validation(format!(
            "Cursul BNR pentru {currency} la {date} nu a fost găsit"
        ))
    })
}

async fn fetch_xml_str(client: &reqwest::Client, url: &str) -> AppResult<String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("Eroare rețea BNR ({url}): {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "BNR HTTP {} pentru {url}",
            resp.status()
        )));
    }
    const MAX: u64 = 25 * 1024 * 1024;
    if resp.content_length().is_some_and(|l| l > MAX) {
        return Err(AppError::Other("Răspuns BNR prea mare (>25 MiB)".into()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("Eroare citire BNR: {e}")))?;
    if bytes.len() as u64 > MAX {
        return Err(AppError::Other("Răspuns BNR prea mare (>25 MiB)".into()));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|e| AppError::Other(format!("Răspuns BNR non-UTF-8: {e}")))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::gl::generate_gl_entries;
    use rust_decimal_macros::dec;
    use sqlx::SqlitePool;

    // ── Pool helper ────────────────────────────────────────────────────────────

    async fn make_pool() -> SqlitePool {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let p = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::migrate!("./migrations").run(&p).await.unwrap();
        p
    }

    async fn insert_company(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'RO1','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_contact(pool: &SqlitePool, id: &str, company_id: &str) {
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES (?1,?2,'CUSTOMER','Client SRL')",
        )
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Inserează factură emisă valutară.
    #[allow(clippy::too_many_arguments)]
    async fn insert_issued(
        pool: &SqlitePool,
        id: &str,
        company_id: &str,
        contact_id: &str,
        currency: &str,
        exchange_rate: f64,
        total: &str,
        issue_date: &str,
    ) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, exchange_rate, \
              subtotal_amount, vat_amount, total_amount, status, payment_means_code, \
              created_at, updated_at) \
             VALUES (?1,?2,?3,'F',1,'F/1',?4,'2026-12-31',?5,?6,'0','0',?7,'VALIDATED','30',1,1)",
        )
        .bind(id)
        .bind(company_id)
        .bind(contact_id)
        .bind(issue_date)
        .bind(currency)
        .bind(exchange_rate)
        .bind(total)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Inserează factură primită valutară.
    async fn insert_received(
        pool: &SqlitePool,
        id: &str,
        company_id: &str,
        currency: &str,
        exchange_rate: f64,
        total: &str,
        issue_date: &str,
    ) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, net_amount, vat_amount, currency, exchange_rate, \
              issue_date, xml_path, status, intra_eu_kind, downloaded_at, created_at) \
             VALUES (?1,?2,?1,'RO999','Furnizor SRL', \
                     ?3,'0','0',?4,?5,?6,'/x.xml','REGISTERED','goods',1,1)",
        )
        .bind(id)
        .bind(company_id)
        .bind(total)
        .bind(currency)
        .bind(exchange_rate)
        .bind(issue_date)
        .execute(pool)
        .await
        .unwrap();
    }

    // ── Test: last_banking_day ─────────────────────────────────────────────────

    #[test]
    fn last_banking_day_avoids_weekend() {
        // 2026-01-31 este sâmbătă → ultima zi bancară = vineri 30
        assert_eq!(last_banking_day("2026-01").unwrap(), "2026-01-30");
        // 2026-02-28 este sâmbătă → ultima zi bancară = vineri 27
        assert_eq!(last_banking_day("2026-02").unwrap(), "2026-02-27");
        // 2026-03-31 este marți → chiar ea
        assert_eq!(last_banking_day("2026-03").unwrap(), "2026-03-31");
        // 2026-04-30 este joi → chiar ea
        assert_eq!(last_banking_day("2026-04").unwrap(), "2026-04-30");
        // 2026-05-31 este duminică → vineri 29
        assert_eq!(last_banking_day("2026-05").unwrap(), "2026-05-29");
    }

    #[test]
    fn last_banking_day_invalid_period_errors() {
        assert!(last_banking_day("2026").is_err());
        assert!(last_banking_day("abc").is_err());
        assert!(last_banking_day("2026-13").is_err());
    }

    // ── GOLDEN TEST: lanț multi-lună (art. 322 base) ──────────────────────────
    //
    // Factură: 1000 EUR @ booking rate 4.97 (= 4970 RON).
    // Luna 1 (2026-01): curs 4.97 → diff = 1000×(4.97-4.97) = 0 (skip, < 0.01).
    //
    // Repunem cu booking 4.97, curs ian = 5.00:
    // diff_1 = 1000×(5.00-4.97) = +30 → D 4111 / C 765
    // prior_rate_2 = 5.00 (nu 4.97!)
    //
    // Luna 2 (2026-02): curs 5.00 → diff = 1000×(5.00-5.00) = 0 (skip).
    // Repunem cu ian 5.00, feb 4.95:
    // diff_2 = 1000×(4.95-5.00) = -50 → D 665 / C 4111 (baza = 5.00, nu 4.97!)
    //
    // Testul direct verifică prior_rate chain.

    #[tokio::test]
    async fn multi_month_chain_art322_base() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // Factură 1000 EUR @ booking 4.97
        insert_issued(
            &pool,
            "inv1",
            "co",
            "ct",
            "EUR",
            4.97,
            "1000.00",
            "2025-12-15",
        )
        .await;

        // BNR XML cu ian 5.00 și feb 4.95
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
    <Cube date="2026-02-27">
      <Rate currency="EUR">4.9500</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        // ── Luna 1: 2026-01 ────────────────────────────────────────────────────
        // last_banking_day("2026-01") = "2026-01-30" → curs = 5.00
        let r1 = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();
        assert_eq!(r1.rows_posted, 1, "luna 1: o factură reevaluată");
        // diff = 1000 × (5.00 − 4.97) = 30
        assert_eq!(
            Decimal::from_str(&r1.total_favorable).unwrap(),
            dec!(30.00),
            "luna 1: total favorabil = 30"
        );
        assert_eq!(
            Decimal::from_str(&r1.total_unfavorable).unwrap(),
            Decimal::ZERO,
            "luna 1: nicio pierdere"
        );

        // Verificăm prior_rate salvat = 5.00
        let rows1 = list_fx_revaluations(&pool, "co", "2026-01").await.unwrap();
        assert_eq!(rows1.len(), 1);
        assert_eq!(
            Decimal::from_str(&rows1[0].prior_rate).unwrap(),
            dec!(4.97),
            "luna 1: prior_rate = booking 4.97"
        );
        assert_eq!(
            Decimal::from_str(&rows1[0].month_end_rate).unwrap(),
            dec!(5.0000),
            "luna 1: month_end_rate = 5.00"
        );
        assert_eq!(Decimal::from_str(&rows1[0].diff_lei).unwrap(), dec!(30.00));

        // ── Luna 2: 2026-02 ────────────────────────────────────────────────────
        // last_banking_day("2026-02") = "2026-02-27" → curs = 4.95
        // BAZA este 5.00 (din luna 1), NU 4.97!
        let r2 = compute_fx_revaluation(&pool, "co", "2026-02", Some(xml))
            .await
            .unwrap();
        assert_eq!(r2.rows_posted, 1, "luna 2: o factură reevaluată");
        // diff = 1000 × (4.95 − 5.00) = −50
        assert_eq!(
            Decimal::from_str(&r2.total_unfavorable).unwrap(),
            dec!(50.00),
            "luna 2: pierdere 50 — baza CORECTĂ 5.00 nu 4.97"
        );
        assert_eq!(
            Decimal::from_str(&r2.total_favorable).unwrap(),
            Decimal::ZERO,
            "luna 2: niciun venit"
        );

        let rows2 = list_fx_revaluations(&pool, "co", "2026-02").await.unwrap();
        assert_eq!(rows2.len(), 1);
        assert_eq!(
            Decimal::from_str(&rows2[0].prior_rate).unwrap(),
            dec!(5.0000),
            "luna 2: prior_rate = 5.00 (din luna 1, NU booking 4.97!)"
        );
        assert_eq!(Decimal::from_str(&rows2[0].diff_lei).unwrap(), dec!(-50.00));

        // ── Verificare nota GL luna 1 ──────────────────────────────────────────
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='FX_REVAL' AND source_id='FX_REVAL-2026-01'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 1, "nota GL luna 1 postată");

        // ── Verificare echilibru nota GL luna 2 ────────────────────────────────
        let (sum_d, sum_c): (String, String) = sqlx::query_as(
            "SELECT CAST(SUM(e.debit) AS TEXT), CAST(SUM(e.credit) AS TEXT) \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id='co' AND j.source_type='FX_REVAL' AND j.source_id='FX_REVAL-2026-02'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let d = Decimal::from_str(&sum_d).unwrap();
        let c = Decimal::from_str(&sum_c).unwrap();
        assert_eq!(d, c, "nota GL luna 2 echilibrată");
        assert_eq!(d, dec!(50), "D 665 = 50");
    }

    // ── Test: semne creanță vs datorie ────────────────────────────────────────

    #[tokio::test]
    async fn receivable_vs_payable_signs() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // Creanță 1000 EUR @ 4.90, curs reevaluare 5.10 (↑) → favorabil D 4111 / C 765
        insert_issued(
            &pool,
            "inv_cr",
            "co",
            "ct",
            "EUR",
            4.90,
            "1000.00",
            "2025-11-01",
        )
        .await;
        // Datorie 500 EUR @ 4.90, curs reevaluare 5.10 (↑) → nefavorabil D 665 / C 401
        insert_received(&pool, "inv_dt", "co", "EUR", 4.90, "500.00", "2025-11-01").await;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.1000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        let r = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();

        // Creanță: 1000×(5.10−4.90) = +200 favorabil
        // Datorie: 500×(5.10−4.90) = +100 nefavorabil (datoria crește)
        // net = 200 − 100 = 100

        assert_eq!(
            Decimal::from_str(&r.total_favorable).unwrap(),
            dec!(200.00),
            "creanță ↑ = favorabil 200"
        );
        assert_eq!(
            Decimal::from_str(&r.total_unfavorable).unwrap(),
            dec!(100.00),
            "datorie ↑ = nefavorabil 100"
        );

        // Verificăm conturile GL
        let entries: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, CAST(e.debit AS TEXT), CAST(e.credit AS TEXT) \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='FX_REVAL' AND j.source_id='FX_REVAL-2026-01'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let sum_by_acc = |code: &str, side: &str| -> Decimal {
            entries
                .iter()
                .filter(|(c, _, _)| c == code)
                .map(|(_, d, cr)| {
                    if side == "d" {
                        Decimal::from_str(d).unwrap_or(Decimal::ZERO)
                    } else {
                        Decimal::from_str(cr).unwrap_or(Decimal::ZERO)
                    }
                })
                .fold(Decimal::ZERO, |a, b| a + b)
        };

        assert_eq!(
            sum_by_acc("4111", "d"),
            dec!(200.00),
            "D 4111 = 200 (creanță curs ↑)"
        );
        assert_eq!(sum_by_acc("765", "c"), dec!(200.00), "C 765 = 200");
        assert_eq!(
            sum_by_acc("665", "d"),
            dec!(100.00),
            "D 665 = 100 (datorie curs ↑)"
        );
        assert_eq!(sum_by_acc("401", "c"), dec!(100.00), "C 401 = 100");

        // Testăm și curs ↓ pentru datorie (D 401 / C 765)
        let pool2 = make_pool().await;
        insert_company(&pool2, "co").await;
        insert_contact(&pool2, "ct", "co").await;
        insert_received(&pool2, "inv_dt2", "co", "EUR", 5.10, "500.00", "2025-11-01").await;
        let xml_down = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">4.9000</Rate>
    </Cube>
  </Body>
</DataSet>"#;
        let r2 = compute_fx_revaluation(&pool2, "co", "2026-01", Some(xml_down))
            .await
            .unwrap();
        // Datorie: 500×(4.90−5.10) = −100 → D 401 / C 765 (favorabil)
        assert_eq!(
            Decimal::from_str(&r2.total_favorable).unwrap(),
            dec!(100.00),
            "datorie curs ↓ = favorabil 100"
        );
        let entries2: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, CAST(e.debit AS TEXT), CAST(e.credit AS TEXT) \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='FX_REVAL' AND j.source_id='FX_REVAL-2026-01'",
        )
        .fetch_all(&pool2)
        .await
        .unwrap();
        let sum2 = |code: &str, side: &str| -> Decimal {
            entries2
                .iter()
                .filter(|(c, _, _)| c == code)
                .map(|(_, d, cr)| {
                    if side == "d" {
                        Decimal::from_str(d).unwrap_or(Decimal::ZERO)
                    } else {
                        Decimal::from_str(cr).unwrap_or(Decimal::ZERO)
                    }
                })
                .fold(Decimal::ZERO, |a, b| a + b)
        };
        assert_eq!(
            sum2("401", "d"),
            dec!(100.00),
            "D 401 = 100 (datorie curs ↓)"
        );
        assert_eq!(sum2("765", "c"), dec!(100.00), "C 765 = 100");
    }

    // ── Test: echilibru GL (Σd == Σc) ─────────────────────────────────────────

    #[tokio::test]
    async fn gl_note_is_balanced() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // Mix: creanță curs ↑ + datorie curs ↑
        insert_issued(
            &pool,
            "i1",
            "co",
            "ct",
            "EUR",
            4.80,
            "1000.00",
            "2025-10-01",
        )
        .await;
        insert_received(&pool, "r1", "co", "EUR", 4.80, "300.00", "2025-10-01").await;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-03-31">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        compute_fx_revaluation(&pool, "co", "2026-03", Some(xml))
            .await
            .unwrap();

        let (sum_d, sum_c): (String, String) = sqlx::query_as(
            "SELECT CAST(SUM(e.debit) AS TEXT), CAST(SUM(e.credit) AS TEXT) \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.source_type='FX_REVAL' AND j.source_id='FX_REVAL-2026-03'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let d = Decimal::from_str(&sum_d).unwrap();
        let c = Decimal::from_str(&sum_c).unwrap();
        assert_eq!(d, c, "nota GL echilibrată (Σd={d}, Σc={c})");
    }

    // ── Test: plată parțială — numai soldul deschis e reevaluat ──────────────

    #[tokio::test]
    async fn partial_payment_only_outstanding_revalued() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // Factură 1000 EUR @ 4.97
        insert_issued(
            &pool,
            "inv1",
            "co",
            "ct",
            "EUR",
            4.97,
            "1000.00",
            "2025-12-01",
        )
        .await;

        // Plată parțială 400 EUR
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method, created_at) \
             VALUES ('p1','inv1','co','400.00','EUR','2026-01-05','transfer',1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        let r = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();

        // Outstanding = 1000 − 400 = 600 EUR
        // diff = 600 × (5.00 − 4.97) = 18
        assert_eq!(r.rows_posted, 1);
        assert_eq!(
            Decimal::from_str(&r.total_favorable).unwrap(),
            dec!(18.00),
            "numai soldul de 600 EUR reevaluat, nu 1000"
        );

        let rows = list_fx_revaluations(&pool, "co", "2026-01").await.unwrap();
        assert_eq!(
            Decimal::from_str(&rows[0].foreign_outstanding).unwrap(),
            dec!(600.00)
        );
    }

    // ── Test: idempotență + supraviețuire generate_gl_entries ────────────────

    #[tokio::test]
    async fn idempotency_and_gl_regen_safety() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;
        insert_issued(
            &pool,
            "inv1",
            "co",
            "ct",
            "EUR",
            4.97,
            "1000.00",
            "2025-12-01",
        )
        .await;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        // Prima rulare
        let r1 = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();
        assert_eq!(r1.rows_posted, 1);

        // A doua rulare — trebuie să înlocuiască (nu dubleze)
        let r2 = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();
        assert_eq!(r2.rows_posted, 1);

        // Un singur jurnal FX_REVAL pentru perioadă
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='FX_REVAL' AND source_id='FX_REVAL-2026-01'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 1, "un singur jurnal FX_REVAL după re-rulare");

        // Un singur rând în fx_revaluation
        let cnt2: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM fx_revaluation \
             WHERE company_id='co' AND period='2026-01'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt2, 1, "un singur rând fx_revaluation după re-rulare");

        // generate_gl_entries nu atinge FX_REVAL
        generate_gl_entries(&pool, "co", "2026-01-01", "2026-01-31", false)
            .await
            .unwrap();
        let cnt3: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co' AND source_type='FX_REVAL'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            cnt3, 1,
            "FX_REVAL supraviețuiește generate_gl_entries (source_type diferit)"
        );
    }

    // ── Test: factură RON exclusă ──────────────────────────────────────────────

    #[tokio::test]
    async fn ron_invoice_excluded() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // Factură RON — trebuie exclusă
        insert_issued(
            &pool,
            "inv_ron",
            "co",
            "ct",
            "RON",
            1.0,
            "5000.00",
            "2025-12-01",
        )
        .await;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        let r = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();
        assert_eq!(r.rows_posted, 0, "factura RON trebuie exclusă");
    }

    // ── Test: factură complet plătită exclusă ─────────────────────────────────

    #[tokio::test]
    async fn fully_paid_invoice_excluded() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        insert_issued(
            &pool,
            "inv1",
            "co",
            "ct",
            "EUR",
            4.97,
            "500.00",
            "2025-12-01",
        )
        .await;

        // Plată completă
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, company_id, amount, currency, paid_at, method, created_at) \
             VALUES ('p1','inv1','co','500.00','EUR','2026-01-10','transfer',1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        let r = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();
        assert_eq!(r.rows_posted, 0, "factura complet plătită trebuie exclusă");
    }

    // ── Test: precizie Decimal (fără drift f64) ────────────────────────────────

    #[tokio::test]
    async fn decimal_exactness_no_f64_drift() {
        let pool = make_pool().await;
        insert_company(&pool, "co").await;
        insert_contact(&pool, "ct", "co").await;

        // 1/3 EUR ca booking — cifre repetitive, diferența trebuie să fie exactă
        insert_issued(
            &pool,
            "inv1",
            "co",
            "ct",
            "EUR",
            4.9700, // booking exact
            "1000.00",
            "2025-12-01",
        )
        .await;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-30">
      <Rate currency="EUR">5.0000</Rate>
    </Cube>
  </Body>
</DataSet>"#;

        let r = compute_fx_revaluation(&pool, "co", "2026-01", Some(xml))
            .await
            .unwrap();

        // 1000 × (5.00 − 4.97) = 30.00 exact (nu 29.999999... din f64)
        let fav = Decimal::from_str(&r.total_favorable).unwrap();
        assert_eq!(
            fav,
            dec!(30.00),
            "precizie Decimal exactă: 30.00, nu 29.999..."
        );
    }
}
