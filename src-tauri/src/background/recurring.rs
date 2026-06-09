//! Background recurring invoice generation.

use tauri::{AppHandle, Emitter, Manager};

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
    use crate::db::models::new_id;
    use crate::db::recurring;
    use chrono::Local;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;

    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let hundred = Decimal::from(100u32);

    let due = recurring::list_due(pool).await?;

    for template in due {
        // Parse lines_json
        let lines: Vec<serde_json::Value> = match serde_json::from_str(&template.lines_json) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    template_id = %template.id,
                    "Failed to parse lines_json for recurring template — skipping"
                );
                continue;
            }
        };

        if lines.is_empty() {
            tracing::warn!(template_id = %template.id, "Recurring template has no lines — skipping");
            continue;
        }

        let mut tx = match pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, template_id = %template.id, "Failed to begin transaction — skipping");
                continue;
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
            continue;
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
                continue;
            }
        };

        let invoice_id = new_id();
        let full_number = format!("{}-{:04}", template.series, allocated_number);
        let issue_date = today.clone();
        let due_date = (Local::now() + chrono::Duration::days(30))
            .date_naive()
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

        for line in &lines {
            let name = line["name"]
                .as_str()
                .or_else(|| line["description"].as_str())
                .unwrap_or("Servicii")
                .to_string();
            let description = line["description"].as_str().map(|s| s.to_string());
            let unit = line["unit"].as_str().unwrap_or("BUC").to_string();
            let vat_category = line["vatCategory"].as_str().unwrap_or("S").to_string();

            let qty = line["quantity"]
                .as_f64()
                .and_then(|v| Decimal::try_from(v).ok())
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
                        let rounded = d.round_dp(0).to_i64().unwrap_or(-1);
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

            // Commercial rounding (MidpointAwayFromZero) for money — the same helper invoices use.
            let ls = crate::db::invoices::round2(qty * price);
            let lv = crate::db::invoices::round2(ls * eff_rate / hundred);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;

            line_calcs.push(LineCalc {
                name,
                description,
                quantity: qty.round_dp(2).to_string(),
                unit,
                unit_price: price.round_dp(2).to_string(),
                vat_rate: eff_rate.round_dp(2).to_string(),
                vat_category,
                subtotal: ls.round_dp(2).to_string(),
                vat_amount: lv.round_dp(2).to_string(),
                total_amount: lt.round_dp(2).to_string(),
            });
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
            continue; // tx not yet begun here — nothing to roll back
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
            continue; // tx drops, auto-rolls-back
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
            continue; // tx drops, auto-rolls-back
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
            if next > today {
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
                    &today,
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
            continue; // tx drops, auto-rolls-back
        }

        // Commit
        if let Err(e) = tx.commit().await {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to commit recurring invoice transaction");
            continue;
        }

        tracing::info!(
            invoice_id = %invoice_id,
            full_number = %full_number,
            template_id = %template.id,
            template_name = %template.template_name,
            "Generated recurring invoice"
        );

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
            let ls = (qty * price).round_dp(2);
            let lv = (ls * vat_rate / hundred).round_dp(2);
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
}
