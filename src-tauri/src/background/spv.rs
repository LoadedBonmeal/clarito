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

    // Reach the app-wide refresh lock (fallback: local no-op lock if state not yet managed).
    let lock_arc = app
        .try_state::<crate::state::AppState>()
        .map(|s| s.token_refresh_lock.clone())
        .unwrap_or_else(|| std::sync::Arc::new(tokio::sync::Mutex::new(())));
    let lock: &tokio::sync::Mutex<()> = &lock_arc;

    let bundle = TokenBundle::load(company_id).ok_or_else(|| AppError::Other("No token".into()))?;

    // Proactive-expiry path: refresh token if needed, with single-flight lock.
    let mut access_token = if !bundle.is_expired() {
        bundle.access_token.clone()
    } else {
        // Acquire lock; double-check after acquiring.
        let _guard = lock.lock().await;
        let bundle =
            TokenBundle::load(company_id).ok_or_else(|| AppError::Other("No token".into()))?;
        if !bundle.is_expired() {
            // Another task already refreshed while we waited.
            bundle.access_token.clone()
        } else {
            let config = crate::commands::anaf::build_oauth_config(pool).await;
            let result = oauth::refresh_token_bundle_with_client_id(
                &bundle.refresh_token,
                &config.client_id,
                &config.token_url,
            )
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
        }
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
            if let Ok(new_tok) = super::poll::refresh_token_for(company_id, pool, lock).await {
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

    // Resolve archive root: prefer user-configured override (ARCHIVE_PATH_OVERRIDE),
    // fall back to <app_data>/archive.  This matches the logic in export_backup and
    // gdpr::resolve_archive_dir so all archive reads/writes target the same directory.
    let archive_root = {
        let override_val =
            crate::db::settings::get(pool, crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE)
                .await
                .unwrap_or(None);
        match override_val {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => app_data_dir.join("archive"),
        }
    };

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
                if let Ok(new_tok) = super::poll::refresh_token_for(company_id, pool, lock).await {
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
        let archive_path = archive_root
            .join("received")
            .join(&safe_cui)
            .join(&safe_year)
            .join(&safe_msg_id);
        // Belt-and-suspenders: verify the resolved path stays under archive_root.
        if !archive_path.starts_with(&archive_root) {
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
        let parsed = parse_received_xml(&xml_content);

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
              net_amount, vat_amount, \
              downloaded_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'RON', ?10, ?11, 'NEW', ?12, ?13, ?14, ?14)",
        )
        .bind(&recv_id)
        .bind(company_id)
        .bind(&msg.id)
        .bind(&msg.id_solicitare)
        .bind(&parsed.issuer_cui)
        .bind(&parsed.issuer_name)
        .bind(Option::<String>::None) // series — NULL until extracted from XML
        .bind(Option::<String>::None) // number — NULL until extracted from XML
        .bind(&parsed.total_amount)
        .bind(&parsed.issue_date)
        .bind(xml_path.to_string_lossy().as_ref())
        .bind(&parsed.net_amount)
        .bind(&parsed.vat_amount)
        .bind(recv_now)
        .execute(pool)
        .await;

        let inserted = match insert_res {
            Ok(r) => r.rows_affected() > 0,
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    anaf_download_id = %msg.id,
                    issuer_cui = %parsed.issuer_cui,
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
        if let Err(e) = tokio::fs::create_dir_all(&archive_path).await {
            tracing::error!("Failed to create archive dir for msg {}: {}", msg.id, e);
            // Roll back the DB insert so we can retry on the next sync.
            let _ = sqlx::query("DELETE FROM received_invoices WHERE id = ?1")
                .bind(&recv_id)
                .execute(pool)
                .await;
            continue;
        }
        if let Err(e) = tokio::fs::write(&xml_path, &xml_content).await {
            tracing::error!("Failed to write XML archive for msg {}: {}", msg.id, e);
            // Roll back the DB insert so we can retry on the next sync.
            let _ = sqlx::query("DELETE FROM received_invoices WHERE id = ?1")
                .bind(&recv_id)
                .execute(pool)
                .await;
            continue;
        }
        if let Err(e) = tokio::fs::write(&zip_path, &zip_bytes).await {
            tracing::warn!("Failed to write ZIP archive for msg {}: {}", msg.id, e);
            // ZIP is supplementary; proceed even if it fails.
        }

        // ── Insert VAT breakdown lines ─────────────────────────────
        for vat_line in &parsed.vat_lines {
            let line_id = crate::db::models::new_id();
            if let Err(e) = sqlx::query(
                "INSERT INTO received_invoice_vat_lines \
                 (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(&line_id)
            .bind(&recv_id)
            .bind(&vat_line.vat_rate)
            .bind(&vat_line.vat_category)
            .bind(&vat_line.base_amount)
            .bind(&vat_line.vat_amount)
            .execute(pool)
            .await
            {
                tracing::warn!(
                    error = ?e,
                    received_invoice_id = %recv_id,
                    "Failed to insert VAT line (continuing)"
                );
            }
        }

        // ── Upgrade the tentative notification with the final text ───
        // RUST-08: notification update errors must NOT abort the loop.
        let final_title = format!("Factură primită de la {}", parsed.issuer_name);
        let final_body = format!("Sumă: {} RON — {}", parsed.total_amount, parsed.issue_date);
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

/// Linie de defalcare TVA extrasă dintr-un `cac:TaxSubtotal` UBL.
#[derive(Debug, Clone)]
pub(crate) struct ReceivedVatLine {
    pub vat_rate: String,
    pub vat_category: String,
    pub base_amount: String,
    pub vat_amount: String,
}

/// Rezultatul parsării XML-ului UBL al unei facturi primite.
#[derive(Debug)]
pub(crate) struct ParsedReceived {
    pub issuer_cui: String,
    pub issuer_name: String,
    /// PayableAmount — valoarea totală de plată (comportament existent).
    pub total_amount: String,
    pub issue_date: String,
    /// cac:LegalMonetaryTotal/cbc:TaxExclusiveAmount — net fără TVA.
    pub net_amount: Option<String>,
    /// cac:TaxTotal/cbc:TaxAmount (direct, nu din TaxSubtotal) — TVA document.
    pub vat_amount: Option<String>,
    /// Câte un ReceivedVatLine pentru fiecare cac:TaxSubtotal.
    pub vat_lines: Vec<ReceivedVatLine>,
}

/// Parsează XML-ul UBL primit de la ANAF folosind quick-xml (namespace-aware).
/// Extrage: CUI emitent, denumire emitent, valoare totală, dată emisie,
/// plus defalcarea net/TVA și linii per cotă TVA.
pub(crate) fn parse_received_xml(xml_bytes: &[u8]) -> ParsedReceived {
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
    let mut net_amount: Option<String> = None;
    let mut vat_amount_doc: Option<String> = None;
    let mut vat_lines: Vec<ReceivedVatLine> = Vec::new();

    // State machine pentru navigarea structurii UBL
    let mut depth_supplier = 0i32; // >0 când suntem în AccountingSupplierParty
    let mut depth_party_tax = 0i32; // >0 când suntem în PartyTaxScheme (al supplier)
    let mut depth_party_legal = 0i32; // >0 când suntem în PartyLegalEntity (al supplier)
    let mut depth_monetary_total = 0i32; // >0 când suntem în LegalMonetaryTotal
    let mut depth_tax_total = 0i32; // >0 când suntem în TaxTotal
    let mut depth_tax_subtotal = 0i32; // >0 când suntem în TaxSubtotal (din TaxTotal)
    let mut depth_tax_category = 0i32; // >0 când suntem în TaxCategory (din TaxSubtotal)

    // Colectare câmpuri pentru subtotalul curent
    let mut sub_base: Option<String> = None;
    let mut sub_vat: Option<String> = None;
    let mut sub_rate: Option<String> = None;
    let mut sub_category: Option<String> = None;

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
                    "LegalMonetaryTotal" => depth_monetary_total += 1,
                    "TaxTotal" => depth_tax_total += 1,
                    "TaxSubtotal" if depth_tax_total > 0 => {
                        depth_tax_subtotal += 1;
                        // Resetăm colectoarele pentru noul subtotal
                        sub_base = None;
                        sub_vat = None;
                        sub_rate = None;
                        sub_category = None;
                    }
                    "TaxCategory" if depth_tax_subtotal > 0 => depth_tax_category += 1,
                    _ => {}
                }
                current_local = local;
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "AccountingSupplierParty" => depth_supplier -= 1,
                    "PartyTaxScheme" if depth_supplier > 0 => depth_party_tax -= 1,
                    "PartyLegalEntity" if depth_supplier > 0 => depth_party_legal -= 1,
                    "LegalMonetaryTotal" => depth_monetary_total -= 1,
                    "TaxTotal" => depth_tax_total -= 1,
                    "TaxSubtotal" if depth_tax_total > 0 && depth_tax_subtotal > 0 => {
                        // Emitem o linie dacă avem cel puțin baza
                        if let Some(base) = sub_base.take() {
                            let line_vat = sub_vat.take().unwrap_or_else(|| "0.00".to_string());
                            let line_rate = sub_rate.take().unwrap_or_else(|| "0".to_string());
                            let line_category =
                                sub_category.take().unwrap_or_else(|| "S".to_string());
                            vat_lines.push(ReceivedVatLine {
                                vat_rate: line_rate,
                                vat_category: line_category,
                                base_amount: base,
                                vat_amount: line_vat,
                            });
                        } else {
                            // skip subtotal fără bază
                            sub_vat = None;
                            sub_rate = None;
                            sub_category = None;
                        }
                        depth_tax_subtotal -= 1;
                    }
                    "TaxCategory" if depth_tax_subtotal > 0 => depth_tax_category -= 1,
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
                    // TaxExclusiveAmount în LegalMonetaryTotal = net fără TVA
                    "TaxExclusiveAmount" if depth_monetary_total > 0 && net_amount.is_none() => {
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            net_amount = Some(d.round_dp(2).to_string());
                        }
                    }
                    // TaxAmount: distingem doc-level (în TaxTotal dar NU în TaxSubtotal)
                    // vs subtotal-level (în TaxSubtotal)
                    "TaxAmount" if depth_tax_total > 0 && depth_tax_subtotal == 0 => {
                        // Nivel document — TVA total factură
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            vat_amount_doc = Some(d.round_dp(2).to_string());
                        }
                    }
                    "TaxAmount" if depth_tax_subtotal > 0 => {
                        // Nivel subtotal — TVA aferent liniei
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            sub_vat = Some(d.round_dp(2).to_string());
                        }
                    }
                    // TaxableAmount în TaxSubtotal = baza impozabilă a liniei
                    "TaxableAmount" if depth_tax_subtotal > 0 => {
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            sub_base = Some(d.round_dp(2).to_string());
                        }
                    }
                    // Percent în TaxCategory = cota TVA (ex. "19")
                    "Percent" if depth_tax_category > 0 => {
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            sub_rate = Some(d.round_dp(0).to_string());
                        }
                    }
                    // ID în TaxCategory = codul categoriei (S/AE/E/Z/K/G/O)
                    "ID" if depth_tax_category > 0 && sub_category.is_none() => {
                        sub_category = Some(text);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    ParsedReceived {
        issuer_cui: if issuer_cui.is_empty() {
            "NECUNOSCUT".to_string()
        } else {
            issuer_cui
        },
        issuer_name: if issuer_name.is_empty() {
            "Necunoscut".to_string()
        } else {
            issuer_name
        },
        total_amount: total_amount_str,
        issue_date,
        net_amount,
        vat_amount: vat_amount_doc,
        vat_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XML UBL minimal cu TaxTotal + TaxSubtotal pentru testarea parser-ului.
    const SAMPLE_UBL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:IssueDate>2024-03-15</cbc:IssueDate>
  <cac:AccountingSupplierParty>
    <cac:Party>
      <cac:PartyTaxScheme>
        <cbc:CompanyID>RO12345678</cbc:CompanyID>
      </cac:PartyTaxScheme>
      <cac:PartyLegalEntity>
        <cbc:RegistrationName>SC TEST SRL</cbc:RegistrationName>
      </cac:PartyLegalEntity>
    </cac:Party>
  </cac:AccountingSupplierParty>
  <cac:TaxTotal>
    <cbc:TaxAmount currencyID="RON">190.00</cbc:TaxAmount>
    <cac:TaxSubtotal>
      <cbc:TaxableAmount currencyID="RON">1000.00</cbc:TaxableAmount>
      <cbc:TaxAmount currencyID="RON">190.00</cbc:TaxAmount>
      <cac:TaxCategory>
        <cbc:ID>S</cbc:ID>
        <cbc:Percent>19</cbc:Percent>
      </cac:TaxCategory>
    </cac:TaxSubtotal>
  </cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:TaxExclusiveAmount currencyID="RON">1000.00</cbc:TaxExclusiveAmount>
    <cbc:PayableAmount currencyID="RON">1190.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
</Invoice>"#;

    #[test]
    fn parser_extracts_issuer() {
        let result = parse_received_xml(SAMPLE_UBL.as_bytes());
        assert_eq!(result.issuer_cui, "RO12345678");
        assert_eq!(result.issuer_name, "SC TEST SRL");
        assert_eq!(result.issue_date, "2024-03-15");
    }

    #[test]
    fn parser_extracts_total_amount() {
        let result = parse_received_xml(SAMPLE_UBL.as_bytes());
        assert_eq!(result.total_amount, "1190.00");
    }

    #[test]
    fn parser_extracts_net_and_vat() {
        let result = parse_received_xml(SAMPLE_UBL.as_bytes());
        assert_eq!(result.net_amount, Some("1000.00".to_string()));
        assert_eq!(result.vat_amount, Some("190.00".to_string()));
    }

    #[test]
    fn parser_extracts_vat_lines() {
        let result = parse_received_xml(SAMPLE_UBL.as_bytes());
        assert_eq!(result.vat_lines.len(), 1);
        let line = &result.vat_lines[0];
        assert_eq!(line.vat_rate, "19");
        assert_eq!(line.vat_category, "S");
        assert_eq!(line.base_amount, "1000.00");
        assert_eq!(line.vat_amount, "190.00");
    }

    #[test]
    fn parser_distinguishes_doc_tax_from_subtotal_tax() {
        // Documentul TVA (190.00) trebuie să fie la nivel document, nu la subtotal
        let result = parse_received_xml(SAMPLE_UBL.as_bytes());
        // TaxAmount la nivel document
        assert_eq!(result.vat_amount, Some("190.00".to_string()));
        // TaxAmount la nivel subtotal (identic în exemplu, dar câmpuri separate)
        assert_eq!(result.vat_lines[0].vat_amount, "190.00");
    }

    #[test]
    fn parser_returns_none_for_missing_vat_structure() {
        let minimal = r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:IssueDate>2024-01-01</cbc:IssueDate>
  <cac:LegalMonetaryTotal>
    <cbc:PayableAmount currencyID="RON">500.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
</Invoice>"#;
        let result = parse_received_xml(minimal.as_bytes());
        assert!(result.net_amount.is_none());
        assert!(result.vat_amount.is_none());
        assert!(result.vat_lines.is_empty());
    }

    #[test]
    fn parser_handles_multiple_tax_subtotals() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:IssueDate>2024-06-01</cbc:IssueDate>
  <cac:TaxTotal>
    <cbc:TaxAmount currencyID="RON">199.00</cbc:TaxAmount>
    <cac:TaxSubtotal>
      <cbc:TaxableAmount currencyID="RON">1000.00</cbc:TaxableAmount>
      <cbc:TaxAmount currencyID="RON">190.00</cbc:TaxAmount>
      <cac:TaxCategory>
        <cbc:ID>S</cbc:ID>
        <cbc:Percent>19</cbc:Percent>
      </cac:TaxCategory>
    </cac:TaxSubtotal>
    <cac:TaxSubtotal>
      <cbc:TaxableAmount currencyID="RON">100.00</cbc:TaxableAmount>
      <cbc:TaxAmount currencyID="RON">9.00</cbc:TaxAmount>
      <cac:TaxCategory>
        <cbc:ID>S</cbc:ID>
        <cbc:Percent>9</cbc:Percent>
      </cac:TaxCategory>
    </cac:TaxSubtotal>
  </cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:TaxExclusiveAmount currencyID="RON">1100.00</cbc:TaxExclusiveAmount>
    <cbc:PayableAmount currencyID="RON">1299.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
</Invoice>"#;
        let result = parse_received_xml(xml.as_bytes());
        assert_eq!(result.vat_lines.len(), 2);
        assert_eq!(result.vat_lines[0].vat_rate, "19");
        assert_eq!(result.vat_lines[0].base_amount, "1000.00");
        assert_eq!(result.vat_lines[1].vat_rate, "9");
        assert_eq!(result.vat_lines[1].base_amount, "100.00");
        assert_eq!(result.vat_amount, Some("199.00".to_string()));
        assert_eq!(result.net_amount, Some("1100.00".to_string()));
    }
}
