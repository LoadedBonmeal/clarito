//! Background SPV sync: download and store received invoices from ANAF SPV.

use tauri::{AppHandle, Emitter, Manager};

pub(crate) async fn sync_spv(app: &AppHandle) -> crate::error::AppResult<()> {
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or_else(|| crate::error::AppError::Other("AppState not available".into()))?;
    let pool = &state.db;
    let test_mode =
        crate::db::settings::get_bool(pool, crate::db::settings::keys::USE_ANAF_TEST_ENV, false)
            .await
            .unwrap_or(false);
    let companies = crate::db::companies::list(pool).await?;

    for company in companies {
        // Only proceed if a token exists for this company
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        // Re-use the same sync logic as the anaf_sync_spv command
        match do_sync_spv(pool, &company.id, app, test_mode).await {
            Ok(new_count) => {
                if new_count > 0 {
                    if let Err(e) = app.emit("spv://new-messages", new_count) {
                        tracing::warn!("Failed to emit spv://new-messages: {:?}", e);
                    }
                    crate::notifications::notify_new_received(app, new_count as u32).await;
                }
            }
            Err(e) => {
                tracing::warn!("SPV sync failed for company {}: {:?}", company.id, e);
            }
        }
    }

    Ok(())
}

pub(crate) async fn do_sync_spv(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    app: &AppHandle,
    test_mode: bool,
) -> crate::error::AppResult<i32> {
    use crate::anaf::{
        client::{AnafClient, ERR_UNAUTHORIZED},
        keychain::TokenBundle,
        oauth,
    };
    use crate::db::notifications;
    use crate::error::AppError;

    let bundle = TokenBundle::load(company_id).ok_or_else(|| AppError::Other("No token".into()))?;

    let mut access_token = if !bundle.is_expired() {
        bundle.access_token.clone()
    } else {
        let result = oauth::refresh_token_bundle(&bundle.refresh_token)
            .await
            .map_err(AppError::Other)?;
        let new_bundle = TokenBundle {
            access_token: result.access_token.clone(),
            refresh_token: result.refresh_token,
            expires_at: result.expires_at,
        };
        new_bundle
            .save(company_id)
            .map_err(|e| AppError::Other(e.to_string()))?;
        result.access_token
    };

    let company = crate::db::companies::get(pool, company_id).await?;
    let client = AnafClient::new(test_mode);

    // list_messages: handle 401 with one refresh+retry
    let mut messages_result = client.list_messages(&access_token, &company.cui, 60).await;
    if let Err(ref e) = messages_result {
        if e == ERR_UNAUTHORIZED {
            tracing::info!(
                company_id,
                "ANAF 401 on list_messages — reîmprospătăm token"
            );
            if let Ok(new_tok) = super::poll::refresh_token_for(company_id).await {
                access_token = new_tok;
                messages_result = client.list_messages(&access_token, &company.cui, 60).await;
            }
        }
    }
    let messages = messages_result.map_err(AppError::Other)?;

    // Resolve app data dir once for archive paths
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut new_count = 0i32;
    for msg in messages {
        let data_key = format!("spv_msg_{}", msg.id);

        // ── Atomic dedup via INSERT OR IGNORE on notifications.data ──────
        // RUST-06: previously a SELECT-then-INSERT race could produce
        // duplicate notifications when two SPV workers ran concurrently.
        // Migration 0010 adds a partial UNIQUE index on `data`, so this
        // INSERT becomes the canonical "claim" — if a row already exists
        // for this msg.id, `rows_affected` is 0 and we skip the message.
        //
        // We seed the row with a tentative title/body. If we successfully
        // download + parse the invoice we UPDATE the row with the final
        // text. If anything fails the tentative text remains, which is
        // still informative.
        let tentative_title = format!("Mesaj SPV nou: {}", msg.tip);
        let tentative_body = msg
            .detalii
            .clone()
            .unwrap_or_else(|| format!("Mesaj primit la {}", msg.data_creare));
        let notif_id = crate::db::models::new_id();
        let now = chrono::Utc::now().timestamp();

        let claim = sqlx::query(
            "INSERT OR IGNORE INTO notifications \
             (id, notification_type, title, body, data, created_at) \
             VALUES (?1, 'SPV_MESSAGE', ?2, ?3, ?4, ?5)",
        )
        .bind(&notif_id)
        .bind(&tentative_title)
        .bind(&tentative_body)
        .bind(&data_key)
        .bind(now)
        .execute(pool)
        .await;

        let claimed = match claim {
            Ok(r) => r.rows_affected() > 0,
            Err(e) => {
                tracing::warn!(
                    msg_id = msg.id.as_str(),
                    error = ?e,
                    "Failed to claim SPV notification slot — skipping"
                );
                continue;
            }
        };
        if !claimed {
            // Already processed by another sync — skip the rest of this message.
            continue;
        }

        // Emit early so the badge updates regardless of download outcome.
        let _ = app.emit("new_notification", serde_json::json!({}));
        new_count += 1;

        // ── Download the ZIP from ANAF (with 401 refresh-once) ───────
        let mut dl_result = client.download_message(&access_token, &msg.id).await;
        if let Err(ref e) = dl_result {
            if e == ERR_UNAUTHORIZED {
                tracing::info!(
                    msg_id = msg.id.as_str(),
                    "ANAF 401 on download — reîmprospătăm token"
                );
                if let Ok(new_tok) = super::poll::refresh_token_for(company_id).await {
                    access_token = new_tok;
                    dl_result = client.download_message(&access_token, &msg.id).await;
                }
            }
        }
        let zip_bytes = match dl_result {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("download_message {} failed: {}", msg.id, e);
                // Tentative notification already inserted; nothing else to do.
                continue;
            }
        };

        // ── Extract XML from ZIP ──────────────────────────────────────
        let xml_content = match extract_xml_from_zip(&zip_bytes) {
            Some(x) => x,
            None => {
                tracing::warn!("No XML found in ZIP for message {}", msg.id);
                continue;
            }
        };

        // ── Compute archive path (but do NOT write yet) ───────────────
        let year = if msg.data_creare.len() >= 4 {
            &msg.data_creare[..4]
        } else {
            "0000"
        };
        // Sanitize msg.id (received from ANAF) to prevent path traversal.
        let safe_msg_id: String = msg
            .id
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .take(64)
            .collect();
        let safe_cui: String = company
            .cui
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .take(64)
            .collect();
        let safe_year: String = year
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .take(64)
            .collect();
        let archive_path = app_data_dir
            .join("archive")
            .join("received")
            .join(&safe_cui)
            .join(&safe_year)
            .join(&safe_msg_id);
        // Belt-and-suspenders: verify the resolved path stays under app_data_dir.
        if !archive_path.starts_with(&app_data_dir) {
            tracing::error!(
                "Archive path escape attempt for msg {}: {:?}",
                msg.id,
                archive_path
            );
            continue;
        }
        let xml_path = archive_path.join("invoice.xml");
        let zip_path = archive_path.join("original.zip");

        // ── Parse XML for basic info ──────────────────────────────────
        let (issuer_cui, issuer_name, total_amount, issue_date) = parse_received_xml(&xml_content);

        // ── RUST-07: INSERT received_invoice FIRST, then write files ─
        // The previous flow wrote the XML to disk before the DB insert,
        // so a duplicate (INSERT OR IGNORE → 0 rows) left an orphaned
        // archive on disk. Now we claim the DB row first; only on success
        // do we touch the filesystem, and if the file write fails we
        // delete the row we just inserted so state stays consistent.
        let recv_id = crate::db::models::new_id();
        let recv_now = chrono::Utc::now().timestamp();
        let insert_res = sqlx::query(
            "INSERT OR IGNORE INTO received_invoices \
             (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
              series, number, total_amount, currency, issue_date, xml_path, status, \
              downloaded_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'RON', ?10, ?11, 'NEW', ?12, ?12)",
        )
        .bind(&recv_id)
        .bind(company_id)
        .bind(&msg.id)
        .bind(&msg.id_solicitare)
        .bind(&issuer_cui)
        .bind(&issuer_name)
        .bind(Option::<String>::None) // series — NULL until extracted from XML
        .bind(Option::<String>::None) // number — NULL until extracted from XML
        .bind(&total_amount)
        .bind(&issue_date)
        .bind(xml_path.to_string_lossy().as_ref())
        .bind(recv_now)
        .execute(pool)
        .await;

        let inserted = match insert_res {
            Ok(r) => r.rows_affected() > 0,
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    anaf_download_id = %msg.id,
                    issuer_cui = %issuer_cui,
                    "Failed to insert received_invoice — invoice will be lost unless re-downloaded from SPV"
                );
                continue;
            }
        };

        if !inserted {
            // received_invoices already had this anaf_download_id — skip file
            // write so we don't create an orphaned archive entry.
            continue;
        }

        // ── DB row exists — now persist files to disk ─────────────────
        if let Err(e) = std::fs::create_dir_all(&archive_path) {
            tracing::error!("Failed to create archive dir for msg {}: {}", msg.id, e);
            // Roll back the DB insert so we can retry on the next sync.
            let _ = sqlx::query("DELETE FROM received_invoices WHERE id = ?1")
                .bind(&recv_id)
                .execute(pool)
                .await;
            continue;
        }
        if let Err(e) = std::fs::write(&xml_path, &xml_content) {
            tracing::error!("Failed to write XML archive for msg {}: {}", msg.id, e);
            // Roll back the DB insert so we can retry on the next sync.
            let _ = sqlx::query("DELETE FROM received_invoices WHERE id = ?1")
                .bind(&recv_id)
                .execute(pool)
                .await;
            continue;
        }
        if let Err(e) = std::fs::write(&zip_path, &zip_bytes) {
            tracing::warn!("Failed to write ZIP archive for msg {}: {}", msg.id, e);
            // ZIP is supplementary; proceed even if it fails.
        }

        // ── Upgrade the tentative notification with the final text ───
        // RUST-08: notification update errors must NOT abort the loop.
        let final_title = format!("Factură primită de la {}", issuer_name);
        let final_body = format!("Sumă: {} RON — {}", total_amount, issue_date);
        if let Err(e) = sqlx::query("UPDATE notifications SET title = ?1, body = ?2 WHERE id = ?3")
            .bind(&final_title)
            .bind(&final_body)
            .bind(&notif_id)
            .execute(pool)
            .await
        {
            tracing::warn!(
                company_id = %company_id,
                message_id = %msg.id,
                error = ?e,
                "Failed to update SPV notification text (continuing)"
            );
        }

        // Re-emit so the frontend picks up the upgraded text.
        let _ = app.emit("new_notification", serde_json::json!({}));
    }

    // Silence unused-import warning when no error path runs `notifications::*`.
    let _ = std::any::type_name::<notifications::CreateNotificationInput>();

    Ok(new_count)
}

// ─── ZIP + XML helpers ─────────────────────────────────────────────────────

fn extract_xml_from_zip(zip_bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).ok()?;
        if file.name().ends_with(".xml") && !file.name().contains("semnatura") {
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).ok()?;
            return Some(contents);
        }
    }
    None
}

/// Parsează XML-ul UBL primit de la ANAF folosind quick-xml (namespace-aware).
/// Extrage: CUI emitent, denumire emitent, valoare totală, dată emisie.
fn parse_received_xml(xml_bytes: &[u8]) -> (String, String, String, String) {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // Strip UTF-8 BOM dacă există
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let xml_str = xml_str.trim_start_matches('\u{FEFF}');

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut issuer_cui = String::new();
    let mut issuer_name = String::new();
    let mut total_amount_str = "0.00".to_string();
    let mut issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // State machine pentru navigarea structurii UBL
    let mut depth_supplier = 0i32; // >0 când suntem în AccountingSupplierParty
    let mut depth_party_tax = 0i32; // >0 când suntem în PartyTaxScheme (al supplier)
    let mut depth_party_legal = 0i32; // >0 când suntem în PartyLegalEntity (al supplier)
    let mut current_local = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_string();
                match local.as_str() {
                    "AccountingSupplierParty" => depth_supplier += 1,
                    "PartyTaxScheme" if depth_supplier > 0 => depth_party_tax += 1,
                    "PartyLegalEntity" if depth_supplier > 0 => depth_party_legal += 1,
                    _ => {}
                }
                current_local = local;
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "AccountingSupplierParty" => {
                        depth_supplier -= 1;
                    }
                    "PartyTaxScheme" if depth_supplier > 0 => {
                        depth_party_tax -= 1;
                    }
                    "PartyLegalEntity" if depth_supplier > 0 => {
                        depth_party_legal -= 1;
                    }
                    _ => {}
                }
                current_local.clear();
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => continue,
                };
                if text.is_empty() {
                    continue;
                }
                match current_local.as_str() {
                    // CompanyID în PartyTaxScheme al furnizorului = CUI fiscal
                    "CompanyID"
                        if depth_supplier > 0 && depth_party_tax > 0 && issuer_cui.is_empty() =>
                    {
                        issuer_cui = text;
                    }
                    // RegistrationName în PartyLegalEntity al furnizorului = denumire
                    "RegistrationName"
                        if depth_supplier > 0
                            && depth_party_legal > 0
                            && issuer_name.is_empty() =>
                    {
                        issuer_name = text;
                    }
                    // PayableAmount = valoarea totală de plată
                    "PayableAmount" => {
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            total_amount_str = d.round_dp(2).to_string();
                        }
                    }
                    // IssueDate = data emiterii
                    "IssueDate" => {
                        issue_date = text;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    (
        if issuer_cui.is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            issuer_cui
        },
        if issuer_name.is_empty() {
            "Necunoscut".to_string()
        } else {
            issuer_name
        },
        total_amount_str,
        issue_date,
    )
}
