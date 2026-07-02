//! Background recurring invoice generation.

use tauri::{AppHandle, Emitter, Manager};

/// ROB-13: tell the user (idempotently) that a recurring template was skipped because its
/// lines are missing/invalid — or carry a VAT rate that is date-blocked (FISCAL-001) —
/// otherwise the invoice silently never generates and they only notice when a client
/// doesn't get billed. `reason` is the RO sentence fragment explaining why (ends with '.').
/// Keyed on the template id in `data`, which carries
/// a UNIQUE index (migration 0010): exactly one alert per broken template persists until the
/// user clears it. We pre-check existence (any read state) so the INSERT never trips the index.
async fn notify_recurring_skipped(
    pool: &sqlx::SqlitePool,
    template_id: &str,
    template_name: &str,
    reason: &str,
) {
    let dup: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications \
         WHERE notification_type = 'recurring_skipped' AND data = ?1",
    )
    .bind(template_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    if dup == 0 {
        let _ = crate::db::notifications::create(
            pool,
            crate::db::notifications::CreateNotificationInput {
                notification_type: "recurring_skipped".into(),
                title: "Factură recurentă negenerată".into(),
                body: format!(
                    "Șablonul recurent „{template_name}” nu a putut genera factura: {reason} \
                     Editați șablonul pentru a relua generarea automată."
                ),
                data: Some(template_id.to_string()),
            },
        )
        .await;
    }
}

/// Generate invoices for all active recurring templates whose next_issue_date is today or earlier.
/// One invoice is created per template per run (even if multiple periods were missed).
/// next_issue_date is advanced through ALL missed periods so it lands in the future.
pub(crate) async fn generate_recurring(app: &AppHandle) -> crate::error::AppResult<()> {
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or_else(|| crate::error::AppError::Other("AppState not available".into()))?;
    process_recurring_invoices(&state.db, app).await
}

async fn process_recurring_invoices(
    pool: &sqlx::SqlitePool,
    app: &AppHandle,
) -> crate::error::AppResult<()> {
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    // Use the contract-aware guard: recurring invoices linked to a non-active or
    // past-end contract are excluded from generation. Unlinked invoices behave
    // exactly as before (no regression). See db::contracts::list_due_with_contract_guard.
    let due = crate::db::contracts::list_due_with_contract_guard(pool).await?;

    for template in due {
        // Generate (or skip) the invoice for this template. The generation itself is
        // AppHandle-free (directly testable against an in-memory pool); only the
        // frontend emit + optional ANAF auto-submit below need the AppHandle.
        let Some((invoice_id, full_number)) = process_one_template(pool, &template, &today).await
        else {
            continue;
        };

        // Notify frontend
        let _ = app.emit(
            "recurring_invoice_generated",
            serde_json::json!({
                "invoiceId": invoice_id,
                "templateId": template.id,
                "templateName": template.template_name,
                "fullNumber": full_number,
            }),
        );

        // F-09: auto-submit to ANAF if template requests it and token exists
        if template.auto_submit_anaf {
            let has_token =
                crate::anaf::keychain::TokenBundle::load(&template.company_id).is_some();
            if has_token {
                let test_mode = crate::db::settings::get_bool(
                    pool,
                    crate::db::settings::keys::USE_ANAF_TEST_ENV,
                    false,
                )
                .await
                .unwrap_or(false);
                match crate::commands::anaf::submit_invoice_inner(
                    app,
                    pool,
                    &template.company_id,
                    &invoice_id,
                    test_mode,
                )
                .await
                {
                    Ok(upload_id) => {
                        tracing::info!(
                            invoice_id = %invoice_id,
                            upload_id = %upload_id,
                            "Recurring invoice auto-submitted to ANAF"
                        );
                    }
                    Err(e) => {
                        // If auto-submit fails, the invoice remains in DRAFT state (not deleted).
                        // This is intentional — the user can manually submit later.
                        tracing::warn!(
                            invoice_id = %invoice_id,
                            full_number = %full_number,
                            error = ?e,
                            "Auto-submit ANAF failed; invoice remains DRAFT"
                        );

                        // User-visible notification — auto-submit has a 5-working-day
                        // deadline, so the user must know to submit manually.
                        if let Err(notif_err) = crate::db::notifications::create(
                            pool,
                            crate::db::notifications::CreateNotificationInput {
                                notification_type: "auto_submit_failed".into(),
                                title: format!("Trimitere automată eșuată: {full_number}"),
                                body: format!(
                                    "Factura {full_number} nu a putut fi trimisă automat la ANAF: {e}. \
                                     A fost salvată ca DRAFT — trimitere manuală necesară."
                                ),
                                data: Some(
                                    serde_json::json!({
                                        "invoiceId": invoice_id,
                                        "fullNumber": full_number,
                                    })
                                    .to_string(),
                                ),
                            },
                        )
                        .await
                        {
                            tracing::error!(
                                invoice_id = %invoice_id,
                                error = ?notif_err,
                                "Failed to persist auto_submit_failed notification"
                            );
                        }

                        // Frontend event so UI can refresh
                        let _ = app.emit(
                            "invoice_status_changed",
                            serde_json::json!({
                                "invoiceId": invoice_id,
                                "newStatus": "DRAFT",
                                "reason": "auto_submit_failed"
                            }),
                        );
                    }
                }
            } else {
                tracing::warn!(
                    template_id = %template.id,
                    "auto_submit_anaf=true but no ANAF token found — invoice left as DRAFT"
                );
            }
        }
    }

    Ok(())
}

/// Generate the invoice for ONE due recurring template, using `today` (ISO `YYYY-MM-DD`)
/// as the issue date. Returns `Some((invoice_id, full_number))` when an invoice was
/// generated and committed; `None` when the template was skipped — in which case NOTHING
/// is committed (the tx rolls back, un-bumping the allocated number) and `next_issue_date`
/// is NOT advanced, so the template retries on the next run once the user fixes it.
///
/// AppHandle-free by design: directly testable against an in-memory pool. The caller
/// (`process_recurring_invoices`) performs the frontend emit + optional ANAF auto-submit.
async fn process_one_template(
    pool: &sqlx::SqlitePool,
    template: &crate::db::recurring::RecurringInvoice,
    today: &str,
) -> Option<(String, String)> {
    use crate::db::models::new_id;
    use crate::db::recurring;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;

    let hundred = Decimal::from(100u32);

    // Parse lines_json
    let lines: Vec<serde_json::Value> = match serde_json::from_str(&template.lines_json) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(
                error = ?e,
                template_id = %template.id,
                "Failed to parse lines_json for recurring template — skipping"
            );
            notify_recurring_skipped(
                pool,
                &template.id,
                &template.template_name,
                "nu are linii valide.",
            )
            .await;
            return None;
        }
    };

    if lines.is_empty() {
        tracing::warn!(template_id = %template.id, "Recurring template has no lines — skipping");
        notify_recurring_skipped(
            pool,
            &template.id,
            &template.template_name,
            "nu are linii valide.",
        )
        .await;
        return None;
    }

    let mut tx = match pool.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to begin transaction — skipping");
            return None;
        }
    };

    // Allocate invoice number atomically by bumping companies.last_invoice_number
    if let Err(e) = sqlx::query(
        "UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1",
    )
    .bind(&template.company_id)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(error = ?e, template_id = %template.id, "Failed to allocate invoice number — skipping");
        return None;
    }

    let allocated_number: i64 = match sqlx::query_scalar(
        "SELECT last_invoice_number FROM companies WHERE id = ?1",
    )
    .bind(&template.company_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to read allocated number — skipping");
            return None;
        }
    };

    let invoice_id = new_id();
    let full_number = format!("{}-{:04}", template.series, allocated_number);
    let issue_date = today.to_string();
    // Due date = issue date + 30 days — same rule as before, but derived from `today`
    // so the function is deterministic under test (the caller passes the real today).
    let due_date = (chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive())
        + chrono::Duration::days(30))
    .format("%Y-%m-%d")
    .to_string();

    // Calculate totals from lines using Decimal
    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;

    struct LineCalc {
        name: String,
        description: Option<String>,
        quantity: String,
        unit: String,
        unit_price: String,
        vat_rate: String,
        vat_category: String,
        subtotal: String,
        vat_amount: String,
        total_amount: String,
    }

    let mut line_calcs: Vec<LineCalc> = Vec::with_capacity(lines.len());

    // FIX 1 / FISCAL-001 (Legea 141/2025): set when any template line carries a VAT rate
    // that is hard-blocked at the issue date — aborts the WHOLE template below.
    let mut blocked_vat_rate: Option<Decimal> = None;

    for line in &lines {
        let name = line["name"]
            .as_str()
            .or_else(|| line["description"].as_str())
            .unwrap_or("Servicii")
            .to_string();
        let description = line["description"].as_str().map(|s| s.to_string());
        let unit = line["unit"].as_str().unwrap_or("BUC").to_string();
        let vat_category = line["vatCategory"].as_str().unwrap_or("S").to_string();

        // ROB-14: parse as a string first (exact Decimal), fall back to f64 only for legacy
        // numeric JSON — mirrors the unitPrice handling below; avoids sub-cent float loss.
        let qty = line["quantity"]
            .as_str()
            .and_then(|s| s.parse::<Decimal>().ok())
            .or_else(|| {
                line["quantity"]
                    .as_f64()
                    .and_then(|v| Decimal::try_from(v).ok())
            })
            .unwrap_or(Decimal::ONE);
        let price = line["unitPrice"]
            .as_str()
            .and_then(|s| s.parse::<Decimal>().ok())
            .or_else(|| {
                line["unitPrice"]
                    .as_f64()
                    .and_then(|v| Decimal::try_from(v).ok())
            })
            .unwrap_or(Decimal::ZERO);
        let vat_rate = if let Some(n) = line["vatRate"].as_i64() {
            if !crate::db::models::VALID_VAT_RATES.contains(&n) {
                tracing::warn!(
                    template_id = %template.id,
                    vat_rate = n,
                    "Recurring invoice: invalid VAT rate in template, skipping line"
                );
                continue;
            }
            Decimal::from(n)
        } else if let Some(s) = line["vatRate"].as_str() {
            match s.parse::<Decimal>() {
                Ok(d) => {
                    let rounded = d
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(-1);
                    if !crate::db::models::VALID_VAT_RATES.contains(&rounded) {
                        tracing::warn!(
                            template_id = %template.id,
                            vat_rate = s,
                            "Recurring invoice: invalid VAT rate in template, skipping line"
                        );
                        continue;
                    }
                    d
                }
                Err(_) => {
                    tracing::warn!(
                        template_id = %template.id,
                        "Recurring invoice: unparseable VAT rate in template, skipping line"
                    );
                    continue;
                }
            }
        } else {
            tracing::warn!(
                template_id = %template.id,
                "Recurring invoice: missing vatRate in template line, skipping"
            );
            continue;
        };

        // VAT1: only category 'S' (Standard) charges VAT; all other categories
        // (AE/E/Z/O reverse-charge/exempt/zero/out-of-scope) store rate 0 and
        // VAT 0 — same category-authoritative rule as commands/invoices.rs.
        let eff_rate = if vat_category == "S" {
            vat_rate
        } else {
            Decimal::ZERO
        };

        // FIX 1 / FISCAL-001 (Legea 141/2025): the manual paths (db::invoices::create /
        // update_invoice_draft) hard-block 19%/5% for issue dates >= 2025-08-01 and 9%
        // for issue dates >= 2026-08-01 via db::invoices::old_vat_rate_blocked. Recurring
        // generation must apply the SAME date-aware rule — otherwise a stale template
        // (e.g. the transitional 9% housing rate) silently keeps emitting blocked-rate
        // invoices after the cutoff. Mirror the manual paths exactly: check the RAW line
        // rate (category-independent). On hit, abort the WHOLE template (guard below) —
        // dropping only the line would silently under-bill the client.
        if crate::db::invoices::old_vat_rate_blocked(&issue_date, vat_rate.to_f64().unwrap_or(0.0))
        {
            blocked_vat_rate = Some(vat_rate);
            break;
        }

        // Commercial rounding (MidpointAwayFromZero) for money — the same helper invoices use.
        let ls = crate::db::invoices::round2(qty * price);
        let lv = crate::db::invoices::round2(ls * eff_rate / hundred);
        let lt = ls + lv;
        subtotal_dec += ls;
        vat_total_dec += lv;

        // ls/lv/lt sunt deja rotunjite comercial mai sus — re-rotunjirea cu round_dp
        // (banker's) era redundantă și cu strategia greșită. Intrările cu zecimale în plus
        // (qty/price/rate) se rotunjesc tot comercial, pentru consecvență.
        line_calcs.push(LineCalc {
            name,
            description,
            quantity: crate::db::invoices::round2(qty).to_string(),
            unit,
            unit_price: crate::db::invoices::round2(price).to_string(),
            vat_rate: crate::db::invoices::round2(eff_rate).to_string(),
            vat_category,
            subtotal: ls.to_string(),
            vat_amount: lv.to_string(),
            total_amount: lt.to_string(),
        });
    }

    // FISCAL-001: a blocked pre-reform VAT rate skips the WHOLE template — no invoice,
    // no next_issue_date advance — so generation resumes automatically once the user
    // re-prices the template (21%/11%). Same tx-release note as D1 below: the tx (holding
    // the last_invoice_number bump) must be dropped BEFORE the notification INSERT, which
    // runs on a separate pool connection; dropping rolls the bump back (no number is lost).
    if let Some(rate) = blocked_vat_rate {
        tracing::warn!(
            template_id = %template.id,
            template_name = %template.template_name,
            vat_rate = %rate,
            issue_date = %issue_date,
            "Recurring template uses a VAT rate blocked at the issue date (Legea 141/2025) — \
             skipping invoice generation and NOT advancing next_issue_date"
        );
        drop(tx);
        notify_recurring_skipped(
            pool,
            &template.id,
            &template.template_name,
            &format!(
                "cota TVA {rate}% nu mai este validă pentru facturi emise la {issue_date} \
                 (Legea 141/2025) — folosiți 21% sau 11%."
            ),
        )
        .await;
        return None;
    }

    // D1: guard — if ALL lines failed validation, line_calcs is empty.
    // Do NOT insert a zero-amount header; skip this template without
    // advancing next_issue_date so it retries next run after the data
    // is corrected.
    if line_calcs.is_empty() {
        tracing::warn!(
            template_id = %template.id,
            template_name = %template.template_name,
            "Recurring template: all lines failed validation — skipping invoice generation \
             and NOT advancing next_issue_date"
        );
        // E-004: don't skip silently — the user's client won't get billed. One alert per
        // broken template: notify_recurring_skipped COUNT-guards on data before inserting
        // (the notifications.data UNIQUE index from migration 0010 is the backstop).
        // The tx (begun above, holding the single WAL writer since the last_invoice_number
        // bump) MUST be released first: notify_recurring_skipped INSERTs on a SEPARATE pool
        // connection and would otherwise deadlock on the writer (~5s busy-timeout → SQLITE_BUSY
        // → the error is swallowed → no notification, defeating E-004). Dropping the tx rolls it
        // back, un-bumping the number we never used (no invoice is generated here).
        drop(tx);
        notify_recurring_skipped(
            pool,
            &template.id,
            &template.template_name,
            "nu are linii valide.",
        )
        .await;
        return None;
    }

    let subtotal = crate::db::invoices::round2(subtotal_dec).to_string();
    let vat_total = crate::db::invoices::round2(vat_total_dec).to_string();
    let total = crate::db::invoices::round2(subtotal_dec + vat_total_dec).to_string();

    let now_unix = chrono::Utc::now().timestamp();

    // Insert invoice header
    let insert_result = sqlx::query(
        "INSERT INTO invoices (
            id, company_id, contact_id, series, number, full_number,
            issue_date, due_date, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            payment_means_code, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, 'RON', NULL,
            ?9, ?10, ?11, 'DRAFT', ?12,
            '30', ?13, ?13
        )",
    )
    .bind(&invoice_id)
    .bind(&template.company_id)
    .bind(&template.client_id)
    .bind(&template.series)
    .bind(allocated_number)
    .bind(&full_number)
    .bind(&issue_date)
    .bind(&due_date)
    .bind(&subtotal)
    .bind(&vat_total)
    .bind(&total)
    .bind(template.notes.as_deref().unwrap_or(""))
    .bind(now_unix)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert_result {
        tracing::error!(error = ?e, template_id = %template.id, "Failed to insert recurring invoice header — skipping");
        return None; // tx drops, auto-rolls-back
    }

    // Insert line items
    let mut lines_ok = true;
    for (i, lc) in line_calcs.iter().enumerate() {
        let line_id = new_id();
        if let Err(e) = sqlx::query(
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, NULL
            )",
        )
        .bind(&line_id)
        .bind(&invoice_id)
        .bind((i as i64) + 1)
        .bind(&lc.name)
        .bind(&lc.description)
        .bind(&lc.quantity)
        .bind(&lc.unit)
        .bind(&lc.unit_price)
        .bind(&lc.vat_rate)
        .bind(&lc.vat_category)
        .bind(&lc.subtotal)
        .bind(&lc.vat_amount)
        .bind(&lc.total_amount)
        .execute(&mut *tx)
        .await
        {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to insert line item — aborting template");
            lines_ok = false;
            break;
        }
    }

    if !lines_ok {
        return None; // tx drops, auto-rolls-back
    }

    // Insert CREATED event
    let _ = sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
         VALUES (?1, ?2, 'CREATED', 'Factură creată automat din șablon recurent', ?3)",
    )
    .bind(new_id())
    .bind(&invoice_id)
    .bind(now_unix)
    .execute(&mut *tx)
    .await;

    // Advance next_issue_date through all missed periods until it's in the
    // future, capped at MAX_CATCHUP iterations.
    //
    // MAX_CATCHUP = 120 covers ~10 years of monthly billing — more than
    // enough for any realistic catch-up scenario.  A template whose date
    // is pathologically old (e.g. 1970, or a stale test record) would
    // otherwise spin thousands of iterations inside an open transaction.
    // After hitting the cap we fast-forward to the next date computed from
    // today, log a warning, and continue normally.
    const MAX_CATCHUP: u32 = 120;
    let mut current_date = template.next_issue_date.clone();
    let mut iterations: u32 = 0;
    let next_date = loop {
        let next = recurring::advance_date(
            &current_date,
            &template.frequency,
            template.day_of_month as u32,
        );
        if next.as_str() > today {
            // Found the correct next future date.
            break next;
        }
        current_date = next;
        iterations += 1;
        if iterations >= MAX_CATCHUP {
            // Pathological catch-up: fast-forward from today instead of
            // spinning further.  This is safe — we already generated one
            // invoice for the current run; the next one will be scheduled
            // from a sane baseline.
            tracing::warn!(
                template_id = %template.id,
                next_issue_date = %template.next_issue_date,
                iterations = MAX_CATCHUP,
                "Recurring template catch-up hit MAX_CATCHUP ({MAX_CATCHUP}) — \
                 advancing next_issue_date from today to avoid infinite loop"
            );
            break recurring::advance_date(
                today,
                &template.frequency,
                template.day_of_month as u32,
            );
        }
    };

    if let Err(e) = sqlx::query(
        "UPDATE recurring_invoices SET next_issue_date = ?1, updated_at = unixepoch() WHERE id = ?2",
    )
    .bind(&next_date)
    .bind(&template.id)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(error = ?e, template_id = %template.id, "Failed to advance next_issue_date — aborting template");
        lines_ok = false; // reuse flag to skip commit
    }

    if !lines_ok {
        return None; // tx drops, auto-rolls-back
    }

    // Commit
    if let Err(e) = tx.commit().await {
        tracing::error!(error = ?e, template_id = %template.id, "Failed to commit recurring invoice transaction");
        return None;
    }

    tracing::info!(
        invoice_id = %invoice_id,
        full_number = %full_number,
        template_id = %template.id,
        template_name = %template.template_name,
        "Generated recurring invoice"
    );

    Some((invoice_id, full_number))
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::db::recurring::advance_date;

    // MAX_CATCHUP mirrors the constant in process_recurring_invoices.
    // We test the pure advance_date logic to verify the cap arithmetic.
    const MAX_CATCHUP: u32 = 120;

    /// A next_issue_date from 1970 must not produce more than MAX_CATCHUP
    /// iterations in the catch-up loop before we bail out.
    #[test]
    fn advance_date_caps_at_max() {
        let start = "1970-01-01";
        let today = "2026-05-31";
        let frequency = "monthly";

        let mut current = start.to_string();
        let mut iterations: u32 = 0;

        let next_date = loop {
            let next = advance_date(&current, frequency, 1);
            if next.as_str() > today {
                break next;
            }
            current = next;
            iterations += 1;
            if iterations >= MAX_CATCHUP {
                // Simulate the fast-forward from today.
                break advance_date(today, frequency, 1);
            }
        };

        assert!(
            iterations == MAX_CATCHUP,
            "Expected loop to hit exactly MAX_CATCHUP={MAX_CATCHUP}, got {iterations}"
        );
        // The result should be a date in the future relative to today.
        assert!(
            next_date.as_str() > today,
            "next_date {next_date} should be after today {today}"
        );
    }

    /// D1: When all template lines have an invalid VAT rate, line_calcs is empty
    /// after the loop.  We verify that the empty-guard condition is triggered
    /// (the production code does `if line_calcs.is_empty() { continue }` before
    /// the tx.begin() / INSERT), meaning no zero-amount invoice is created.
    #[test]
    fn d1_all_invalid_vat_lines_produces_empty_line_calcs() {
        use crate::db::models::VALID_VAT_RATES;
        use rust_decimal::Decimal;

        struct FakeLine {
            vat_rate: i64,
        }
        let lines = vec![
            FakeLine { vat_rate: 99 }, // invalid
            FakeLine { vat_rate: 77 }, // invalid
        ];

        let mut line_calcs: Vec<String> = Vec::new(); // simulates Vec<LineCalc>
        let hundred = Decimal::from(100u32);
        let mut subtotal_dec = Decimal::ZERO;
        let mut vat_total_dec = Decimal::ZERO;

        for line in &lines {
            let n = line.vat_rate;
            if !VALID_VAT_RATES.contains(&n) {
                // mirrors the `continue` inside the production loop
                continue;
            }
            let vat_rate = Decimal::from(n);
            let qty = Decimal::ONE;
            let price = Decimal::from(100u32);
            let ls = crate::db::invoices::round2(qty * price);
            let lv = crate::db::invoices::round2(ls * vat_rate / hundred);
            subtotal_dec += ls;
            vat_total_dec += lv;
            line_calcs.push(format!("line-{n}"));
        }

        // After the loop, line_calcs must be empty → guard fires → no invoice.
        assert!(
            line_calcs.is_empty(),
            "Expected empty line_calcs when all VAT rates are invalid, got {:?}",
            line_calcs
        );
        assert_eq!(subtotal_dec, Decimal::ZERO);
        assert_eq!(vat_total_dec, Decimal::ZERO);
    }

    /// D1: Verify that at least one valid line still produces a non-empty line_calcs
    /// (sanity check that the guard doesn't fire for normal templates).
    #[test]
    fn d1_one_valid_line_produces_non_empty_line_calcs() {
        use crate::db::models::VALID_VAT_RATES;
        use rust_decimal::Decimal;

        struct FakeLine {
            vat_rate: i64,
        }
        let lines = vec![
            FakeLine { vat_rate: 99 }, // invalid → skipped
            FakeLine { vat_rate: 19 }, // valid
        ];

        let mut line_calcs: Vec<String> = Vec::new();
        let hundred = Decimal::from(100u32);

        for line in &lines {
            let n = line.vat_rate;
            if !VALID_VAT_RATES.contains(&n) {
                continue;
            }
            line_calcs.push(format!("line-{n}"));
        }
        let _ = hundred; // suppress unused warning

        assert!(
            !line_calcs.is_empty(),
            "Expected non-empty line_calcs when at least one valid line exists"
        );
    }

    /// A recently-started template (date slightly in the past) should complete
    /// in well under MAX_CATCHUP iterations.
    #[test]
    fn advance_date_normal_catchup_stays_under_max() {
        let start = "2026-03-01"; // two months behind
        let today = "2026-05-31";
        let frequency = "monthly";

        let mut current = start.to_string();
        let mut iterations: u32 = 0;

        let next_date = loop {
            let next = advance_date(&current, frequency, 1);
            if next.as_str() > today {
                break next;
            }
            current = next;
            iterations += 1;
            if iterations >= MAX_CATCHUP {
                break advance_date(today, frequency, 1);
            }
        };

        assert!(
            iterations < MAX_CATCHUP,
            "Normal catch-up should not reach MAX_CATCHUP, got {iterations}"
        );
        assert!(
            next_date.as_str() > today,
            "next_date {next_date} should be after today {today}"
        );
    }

    /// ROB-13: skipping a line-less template alerts the user exactly once per template.
    /// The UNIQUE index on notifications.data (migration 0010) makes the alert persist
    /// until cleared — re-running the skip does NOT duplicate, and does NOT trip the index.
    #[tokio::test]
    async fn rob13_skipped_template_notifies_once_per_template() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let total = || async {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM notifications WHERE notification_type = 'recurring_skipped'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };

        super::notify_recurring_skipped(&pool, "tmpl-1", "Chirie lunară", "nu are linii valide.")
            .await;
        super::notify_recurring_skipped(&pool, "tmpl-1", "Chirie lunară", "nu are linii valide.")
            .await;
        assert_eq!(
            total().await,
            1,
            "repeated skips of the same template must not duplicate"
        );

        // A different template gets its own alert.
        super::notify_recurring_skipped(&pool, "tmpl-2", "Mentenanță", "nu are linii valide.")
            .await;
        assert_eq!(total().await, 2);

        // Even after the user reads it, re-skipping must not error on the UNIQUE(data) index.
        sqlx::query("UPDATE notifications SET is_read = 1 WHERE data = 'tmpl-1'")
            .execute(&pool)
            .await
            .unwrap();
        super::notify_recurring_skipped(&pool, "tmpl-1", "Chirie lunară", "nu are linii valide.")
            .await;
        assert_eq!(
            total().await,
            2,
            "no duplicate row and no UNIQUE-index violation"
        );
    }

    // ── FIX 1 / FISCAL-001: recurring generation respects the date-aware VAT block ──

    /// One 9% (transitional housing rate) line — valid through 2026-07-31, blocked after.
    const LINES_9PCT: &str = r#"[{"name":"Chirie locuință","quantity":"1","unitPrice":"1000","vatRate":9,"unit":"BUC","vatCategory":"S"}]"#;

    /// In-memory pool + migrations + company/contact/template rows so FKs pass and the
    /// skip path can assert next_issue_date/last_invoice_number stayed untouched.
    async fn fiscal001_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies \
             (id, legal_name, cui, registry_number, address, city, county, country, \
              email, vat_payer, last_invoice_number) \
             VALUES ('comp-1','Test SRL','RO1','J00/1/2020','Str 1','Bucuresti','B','RO', \
                     't@t.ro',1,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES ('cust-1','comp-1','CUSTOMER','Client SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO recurring_invoices \
             (id, company_id, template_name, client_id, frequency, next_issue_date, \
              day_of_month, series, lines_json) \
             VALUES ('tmpl-9','comp-1','Chirie 9%','cust-1','monthly','2026-07-01',1,'FCT',?1)",
        )
        .bind(LINES_9PCT)
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn template_9pct() -> crate::db::recurring::RecurringInvoice {
        crate::db::recurring::RecurringInvoice {
            id: "tmpl-9".into(),
            company_id: "comp-1".into(),
            template_name: "Chirie 9%".into(),
            client_id: "cust-1".into(),
            frequency: "monthly".into(),
            next_issue_date: "2026-07-01".into(),
            day_of_month: 1,
            auto_submit_anaf: false,
            active: true,
            series: "FCT".into(),
            lines_json: LINES_9PCT.into(),
            notes: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// A 9% housing template generated with issue date 2026-08-01 (past the 2026-07-31
    /// cutoff, Legea 141/2025) must be SKIPPED: no invoice, a recurring_skipped
    /// notification recorded, next_issue_date NOT advanced (so the user re-prices the
    /// template and generation resumes), and the allocated number rolled back.
    #[tokio::test]
    async fn fiscal001_blocked_rate_skips_template_and_does_not_advance() {
        let pool = fiscal001_pool().await;
        let template = template_9pct();

        let out = super::process_one_template(&pool, &template, "2026-08-01").await;
        assert!(out.is_none(), "blocked-rate template must not generate");

        let invoices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(invoices, 0, "no invoice may be committed");

        let (notif_count, body): (i64, String) = {
            let c: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM notifications \
                 WHERE notification_type = 'recurring_skipped' AND data = 'tmpl-9'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let b: String = sqlx::query_scalar(
                "SELECT body FROM notifications \
                 WHERE notification_type = 'recurring_skipped' AND data = 'tmpl-9'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            (c, b)
        };
        assert_eq!(notif_count, 1, "exactly one skip notification");
        assert!(
            body.contains("cota TVA 9%") && body.contains("nu mai este validă"),
            "notification must explain the blocked rate, got: {body}"
        );

        let next: String = sqlx::query_scalar(
            "SELECT next_issue_date FROM recurring_invoices WHERE id = 'tmpl-9'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            next, "2026-07-01",
            "next_issue_date must NOT advance on skip"
        );

        let last_no: i64 =
            sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id = 'comp-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(last_no, 0, "allocated number must be rolled back on skip");
    }

    /// Positive control: the SAME 9% template still generates normally while the rate is
    /// valid (issue date 2026-07-15 ≤ 2026-07-31) — proving the block is date-aware, not
    /// a blanket 9% ban.
    #[tokio::test]
    async fn fiscal001_nine_percent_still_generates_before_cutoff() {
        let pool = fiscal001_pool().await;
        let template = template_9pct();

        let out = super::process_one_template(&pool, &template, "2026-07-15").await;
        let (_invoice_id, full_number) = out.expect("9% must still generate before the cutoff");
        assert_eq!(full_number, "FCT-0001");

        let (issue_date, vat, total): (String, String, String) = sqlx::query_as(
            "SELECT issue_date, vat_amount, total_amount FROM invoices WHERE full_number = 'FCT-0001'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(issue_date, "2026-07-15");
        // round2 caps at 2dp but does not pad integral Decimals — 9% of 1000 stores as "90".
        assert_eq!(vat, "90");
        assert_eq!(total, "1090");

        let next: String = sqlx::query_scalar(
            "SELECT next_issue_date FROM recurring_invoices WHERE id = 'tmpl-9'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(next, "2026-08-01", "next_issue_date advances on success");

        let notif: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE notification_type = 'recurring_skipped'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(notif, 0, "no skip notification on success");
    }
}
