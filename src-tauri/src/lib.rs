//! Entry-point al backend-ului Rust.
//!
//! Aici:
//! - configurăm plugin-urile Tauri
//! - inițializăm pool-ul SQLite (la `setup`)
//! - înregistrăm toate Tauri commands prin `generate_handler!`
//!
//! Logica de business e în submodule (`db`, `commands`, etc.).

mod anaf;
pub mod anaf_decl;
mod background;
pub mod commands;
mod constraint_guard;
pub mod db;
mod error;
pub mod notifications;
mod state;
mod ubl;

// The `license-gen` workspace crate (./license-gen) reaches
// commands::license::{key_checksum, validate_license_key} via this narrow
// re-export — single source of truth for the HMAC key algorithm.
pub use commands::license::{key_checksum, validate_license_key};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("efactura".into()),
                    }),
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    window.hide().ok();
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            // ── Logging bridge: tracing → on-disk efactura.log ──────────────
            //
            // tauri_plugin_log writes frontend JS logs + `log::` crate records to
            // LogDir/efactura.log.  `tracing::*!` events (86 call sites in the
            // backend) previously went only to stderr.  Here we open the same log
            // file in append mode and add a second `tracing_subscriber::fmt` layer
            // that writes there, so backend tracing events also reach the file that
            // the feedback diagnostic reads.
            //
            // No new Cargo dependency required: `std::fs::File` as the writer,
            // wrapped in `Arc<Mutex<…>>` for the `MakeWriter` trait requirement.
            use std::sync::{Arc, Mutex};
            use tracing_subscriber::prelude::*;

            let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into());

            // Build a file-appender layer if we can resolve the log directory.
            // The path mirrors what tauri_plugin_log uses:
            //   macOS: ~/Library/Logs/<bundle-id>/efactura.log
            let file_layer = app
                .path()
                .app_log_dir()
                .ok()
                .and_then(|log_dir| {
                    std::fs::create_dir_all(&log_dir).ok()?;
                    let log_path = log_dir.join("efactura.log");
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .map(|f| {
                            tracing::subscriber::with_default(
                                tracing::subscriber::NoSubscriber::new(),
                                || {
                                    tracing::info!(
                                        path = %log_path.display(),
                                        "Logging bridge: tracing → file active"
                                    );
                                },
                            );
                            Arc::new(Mutex::new(f))
                        })
                        .ok()
                })
                .map(|writer| {
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(move || {
                            // Clone the Arc so the closure is 'static-compatible.
                            struct MutexWriter(Arc<Mutex<std::fs::File>>);
                            impl std::io::Write for MutexWriter {
                                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                                    self.0.lock().unwrap_or_else(|e| e.into_inner()).write(buf)
                                }
                                fn flush(&mut self) -> std::io::Result<()> {
                                    self.0.lock().unwrap_or_else(|e| e.into_inner()).flush()
                                }
                            }
                            MutexWriter(writer.clone())
                        })
                });

            // Build the subscriber: stderr layer (always) + file layer (when available).
            let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

            let subscriber = tracing_subscriber::registry()
                .with(env_filter)
                .with(stderr_layer);

            if let Some(fl) = file_layer {
                let _ = subscriber.with(fl).try_init();
            } else {
                let _ = subscriber.try_init();
            }

            let handle = app.handle().clone();

            // Pool DB inițializat async, apoi mutat în AppState.
            // Blocăm setup-ul scurt aici (tokio runtime Tauri rulează deja).
            tauri::async_runtime::block_on(async move {
                match db::pool::init(&handle).await {
                    Ok(pool) => {
                        #[cfg(debug_assertions)]
                        if let Err(err) = db::seed::run_if_empty(&pool).await {
                            tracing::warn!(?err, "Seed failed");
                        }
                        handle.manage(AppState::new(pool));
                        tracing::info!("AppState initialized");
                        background::spawn_background_tasks(handle.clone());
                    }
                    Err(err) => {
                        tracing::error!(?err, "Failed to initialize SQLite pool");
                    }
                }
            });

            // System tray
            let show_item = MenuItem::with_id(app, "show", "Deschide Clarito", true, None::<&str>)?;
            let new_invoice_item =
                MenuItem::with_id(app, "new_invoice", "Factură nouă", true, None::<&str>)?;
            let sync_item =
                MenuItem::with_id(app, "sync", "Sincronizare ANAF", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Ieșire", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_item, &new_invoice_item, &sync_item, &quit_item],
            )?;
            let _tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "new_invoice" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                            use tauri::Emitter;
                            let _ = window.emit("tray_navigate", "/invoices/new");
                        }
                    }
                    "sync" => {
                        // Run manual sync in background task
                        let app_for_sync = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(state) = app_for_sync.try_state::<AppState>() {
                                let pool = &state.db;
                                let lock = &state.token_refresh_lock;
                                let all_companies =
                                    crate::db::companies::list(pool).await.unwrap_or_default();
                                for company in &all_companies {
                                    if crate::anaf::keychain::TokenBundle::load(&company.id)
                                        .is_none()
                                    {
                                        continue;
                                    }
                                    let _ = crate::background::poll_submitted_for_company(
                                        pool,
                                        &company.id,
                                        Some(&app_for_sync),
                                        lock,
                                    )
                                    .await;
                                    let test_mode = crate::db::settings::get_bool(
                                        pool,
                                        crate::db::settings::keys::USE_ANAF_TEST_ENV,
                                        false,
                                    )
                                    .await
                                    .unwrap_or(false);
                                    let _ = crate::background::do_sync_spv(
                                        pool,
                                        &company.id,
                                        &app_for_sync,
                                        test_mode,
                                    )
                                    .await;
                                }
                                use tauri::Emitter;
                                let _ = app_for_sync
                                    .emit("sync_completed", serde_json::json!({"source": "tray"}));
                            }
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // companies
            commands::companies::list_companies,
            commands::companies::get_company,
            commands::companies::tax_regime_status,
            commands::companies::vat_registration_status,
            commands::companies::create_company,
            commands::companies::update_company,
            commands::companies::delete_company,
            commands::companies::get_next_invoice_number,
            commands::companies::fetch_anaf_company_data,
            commands::companies::validate_vies,
            // contacts
            commands::contacts::list_contacts,
            commands::contacts::get_contact,
            commands::contacts::create_contact,
            commands::contacts::update_contact,
            commands::contacts::delete_contact,
            commands::contacts::search_contacts,
            // products (articole / catalog)
            commands::products::list_products,
            commands::products::get_product,
            commands::products::create_product,
            commands::products::update_product,
            commands::products::delete_product,
            commands::products::search_products,
            // invoices
            commands::invoices::list_invoices,
            commands::invoices::get_invoice,
            commands::invoices::create_invoice_draft,
            commands::invoices::delete_invoice,
            commands::invoices::set_invoice_status,
            commands::invoices::update_invoice_draft,
            commands::invoices::validate_invoice_draft,
            commands::invoices::verify_invoice_files,
            commands::invoices::storno_invoice,
            commands::invoices::duplicate_invoice,
            // received
            commands::received::list_received_invoices,
            commands::received::get_received_invoice,
            commands::received::update_received_status,
            commands::received::reparse_received_vat,
            commands::received::export_received_csv,
            commands::received::set_received_intra_eu_kind,
            // notifications
            commands::notifications::list_notifications,
            commands::notifications::unread_notification_count,
            commands::notifications::mark_notification_read,
            commands::notifications::mark_all_notifications_read,
            commands::notifications::delete_notification,
            commands::notifications::delete_all_read_notifications,
            // settings
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_all_settings,
            // license
            commands::license::get_license,
            commands::license::start_trial,
            commands::license::activate_license,
            commands::license::check_license_validity,
            // system
            commands::system::get_app_info,
            commands::system::get_db_path,
            commands::system::manual_sync,
            #[cfg(debug_assertions)]
            commands::system::dev_seed,
            commands::system::open_archive_folder,
            commands::system::get_activity_log,
            commands::system::export_activity_log_csv,
            commands::system::set_autostart,
            commands::system::get_autostart,
            commands::system::check_form_versions,
            // ubl
            commands::ubl::generate_invoice_xml,
            commands::ubl::generate_invoice_pdf,
            commands::ubl::preview_invoice_template,
            commands::ubl::validate_invoice_xml,
            // anaf
            commands::anaf::anaf_authorize,
            commands::anaf::anaf_is_authenticated,
            commands::anaf::anaf_logout,
            commands::anaf::anaf_set_oauth_client_secret,
            commands::anaf::anaf_has_oauth_client_secret,
            commands::anaf::anaf_submit_invoice,
            commands::anaf::anaf_check_invoice_status,
            commands::anaf::anaf_sync_spv,
            commands::anaf::anaf_list_spv_inbox,
            commands::anaf::anaf_refresh_certificate,
            commands::anaf::anaf_revoke_certificate,
            commands::anaf::anaf_get_certificates,
            // archive
            commands::archive::export_invoices_zip,
            commands::archive::export_backup,
            commands::archive::verify_archive_integrity,
            commands::archive::get_archive_size,
            commands::archive::import_backup,
            commands::archive::change_archive_location,
            // import
            commands::import::import_invoices_csv,
            commands::import::import_contacts_csv,
            commands::import::import_invoice_xml,
            commands::import::import_invoice_xml_from_file,
            commands::import::get_invoices_csv_template,
            commands::import::get_contacts_csv_template,
            // import wave c — multi-source migration importer
            commands::import_wave_c::commit::import_wave_c_detect_columns,
            commands::import_wave_c::commit::import_wave_c_stage,
            commands::import_wave_c::commit::import_wave_c_preview,
            commands::import_wave_c::commit::import_wave_c_commit,
            // integrations
            commands::integrations::smartbill_push_invoice,
            commands::integrations::export_saga_csv,
            commands::integrations::export_winmentor_csv,
            commands::integrations::get_smartbill_credentials,
            commands::integrations::set_smartbill_credentials,
            commands::integrations::clear_smartbill_credentials,
            commands::xlsx::export_invoices_xlsx,
            commands::xlsx::export_declaration_xlsx,
            commands::xlsx::open_doc_in_browser,
            // reports
            commands::reports::generate_vat_report,
            commands::reports::export_report,
            commands::reports::aging_report,
            commands::reports::export_aging_csv,
            // payments
            commands::payments::add_payment,
            commands::payments::list_payments,
            commands::payments::delete_payment,
            commands::payments::get_payment_summary,
            commands::payments::list_payment_summaries,
            // bank statement import (Wave 6 — jurnal de bancă)
            commands::bank_import::commands::create_bank_account,
            commands::bank_import::commands::list_bank_accounts,
            commands::bank_import::commands::delete_bank_account,
            commands::bank_import::commands::import_bank_statement,
            commands::bank_import::commands::list_bank_statements,
            commands::bank_import::commands::list_bank_transactions,
            commands::bank_import::commands::match_bank_txn,
            commands::bank_import::commands::unmatch_bank_txn,
            commands::bank_import::commands::ignore_bank_txn,
            // supplier payments (payments-out / buyer-side TVA la încasare)
            commands::received_payments::add_received_payment,
            commands::received_payments::list_received_payments,
            commands::received_payments::delete_received_payment,
            commands::received_payments::get_received_payment_summary,
            // recurring invoices
            commands::recurring::create_recurring_invoice,
            commands::recurring::list_recurring_invoices,
            commands::recurring::delete_recurring_invoice,
            commands::recurring::update_recurring_invoice,
            commands::recurring::toggle_recurring_active,
            // declarations d300
            commands::declarations::compute_d300,
            commands::declarations::cash_vat_plafon_status,
            commands::declarations::intrastat_status,
            commands::d390::compute_d390,
            commands::d390::export_d390,
            commands::d390::preview_d390_xml,
            commands::declarations::reconcile_etva,
            commands::declarations::etva_fetch_precompletat,
            commands::declarations::compute_d101,
            commands::declarations::compute_d100,
            commands::declarations::validate_declaration_xml,
            commands::declarations::compute_payroll,
            commands::etransport::etransport_validate,
            commands::etransport::etransport_generate_xml,
            commands::etransport::etransport_submit,
            commands::etransport::list_etransport_declarations,
            commands::declarations::export_d300,
            commands::declarations::export_d300_official,
            commands::declarations::preview_d300_xml,
            commands::declarations::preflight_declaration,
            commands::declarations::list_declaration_filings,
            commands::declarations::delete_declaration_filing,
            // d394 — livrări/achiziții pe teritoriul național
            commands::d394::compute_d394,
            commands::d394::export_d394,
            commands::d394::export_d394_official,
            commands::d394::preview_d394_xml,
            // jurnale contabile
            commands::journals::export_sales_journal,
            commands::journals::export_purchase_journal,
            // saft d406
            commands::saft::export_saft_d406,
            commands::saft::export_saft_official,
            commands::saft::preview_saft_official_xml,
            // feedback / diagnostic
            commands::feedback::gather_diagnostic,
            commands::feedback::build_feedback_mailto,
            // gdpr / data portability
            commands::gdpr::export_all_my_data,
            commands::gdpr::wipe_all_data,
            // bnr — curs valutar oficial (R17 Wave 2)
            commands::bnr::fetch_bnr_rate,
            // vat rates — global editable catalog (R15 Wave 2)
            commands::vat_rates::list_vat_rates,
            commands::vat_rates::get_vat_rate,
            commands::vat_rates::vat_rate_note,
            commands::vat_rates::create_vat_rate,
            commands::vat_rates::update_vat_rate,
            commands::vat_rates::delete_vat_rate,
            commands::vat_rates::set_vat_rate_active,
            // receipts — chitanțe (R15 Wave 3)
            commands::receipts::list_receipts,
            commands::receipts::get_receipt,
            commands::receipts::create_receipt,
            commands::receipts::delete_receipt,
            commands::receipts::generate_receipt_pdf,
            // chart of accounts — plan de conturi (R15 Wave 4)
            commands::accounts::list_accounts,
            commands::accounts::get_account,
            commands::accounts::create_account,
            commands::accounts::update_account,
            commands::accounts::delete_account,
            commands::accounts::seed_standard_accounts,
            // GL auto-posting engine (Phase 5a)
            commands::gl::generate_gl_entries,
            commands::gl::reconcile_gl,
            commands::gl::close_vat_period,
            commands::gl::trial_balance,
            commands::gl::profit_and_loss,
            commands::gl::bilant,
            commands::gl::export_bilant_xml,
            commands::gl::preview_bilant_xml,
            commands::gl::post_income_tax,
            commands::gl::post_annual_close,
            commands::payroll::list_employees,
            commands::payroll::create_employee,
            commands::payroll::update_employee,
            commands::payroll::delete_employee,
            commands::payroll::list_secondary_offices,
            commands::payroll::create_secondary_office,
            commands::payroll::delete_secondary_office,
            commands::dividends::list_dividends,
            commands::dividends::create_dividend,
            commands::dividends::update_dividend_beneficiary,
            commands::dividends::delete_dividend,
            commands::dividends::export_d205_official,
            commands::dividends::preview_d205_xml,
            commands::dividends::export_d207_official,
            commands::dividends::preview_d207_xml,
            commands::payroll::list_medical_leaves,
            commands::payroll::create_medical_leave,
            commands::payroll::delete_medical_leave,
            commands::payroll::run_payroll,
            commands::payroll::export_d112_xml,
            commands::payroll::preview_d112_xml,
            commands::gl::close_period,
            commands::gl::journal_register,
            commands::gl::general_ledger,
            commands::gl::partner_ledger,
            // note contabile manuale (cod 14-6-2A) — P1 Wave 4
            commands::manual_journal::create_manual_journal,
            commands::manual_journal::list_manual_journals,
            commands::manual_journal::delete_manual_journal,
            // stock movements — Phase 6a (SAF-T MovementOfGoods)
            commands::stock::create_stock_movement,
            commands::stock::list_stock_movements,
            commands::stock::delete_stock_movement,
            commands::stock::record_stock_receipt,
            commands::stock::record_stock_issue,
            commands::stock::stock_ledger,
            commands::stock::set_stock_valuation,
            // inventory — registru-inventar + inventariere (P1 Wave 5)
            commands::inventory::create_inventory_session,
            commands::inventory::get_inventory_session,
            commands::inventory::list_inventory_sessions,
            commands::inventory::delete_inventory_session,
            commands::inventory::list_inventory_lines,
            commands::inventory::update_inventory_line_faptic,
            commands::inventory::prefill_inventory_session,
            commands::inventory::finalize_inventory_session,
            commands::inventory::post_inventory_diffs,
            commands::inventory::list_registru_inventar,
            // fixed assets — Phase 6b (SAF-T Assets)
            commands::assets::create_fixed_asset,
            commands::assets::list_fixed_assets,
            commands::assets::delete_fixed_asset,
            commands::assets::update_fixed_asset,
            commands::assets::run_depreciation,
            commands::assets::dispose_asset,
            commands::assets::list_depreciation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
