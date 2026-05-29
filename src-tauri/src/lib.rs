//! Entry-point al backend-ului Rust.
//!
//! Aici:
//! - configurăm plugin-urile Tauri
//! - inițializăm pool-ul SQLite (la `setup`)
//! - înregistrăm toate Tauri commands prin `generate_handler!`
//!
//! Logica de business e în submodule (`db`, `commands`, etc.).

mod anaf;
mod background;
mod commands;
mod db;
mod error;
pub mod notifications;
mod state;
mod ubl;

use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        .plugin(tauri_plugin_log::Builder::default().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    window.hide().ok();
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            // Logging structurat (early init).
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,sqlx=warn".into()),
                )
                .try_init();

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
            let show_item = MenuItem::with_id(app, "show", "Deschide RoFactura", true, None::<&str>)?;
            let new_invoice_item = MenuItem::with_id(app, "new_invoice", "Factură nouă", true, None::<&str>)?;
            let sync_item = MenuItem::with_id(app, "sync", "Sincronizare ANAF", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Ieșire", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &new_invoice_item, &sync_item, &quit_item])?;
            let _tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(true)
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
                                let all_companies = crate::db::companies::list(pool).await.unwrap_or_default();
                                for company in &all_companies {
                                    if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
                                        continue;
                                    }
                                    let _ = crate::background::poll_submitted_for_company(pool, &company.id, Some(&app_for_sync)).await;
                                    let test_mode = crate::db::settings::get_bool(pool, crate::db::settings::keys::USE_ANAF_TEST_ENV, false).await.unwrap_or(false);
                                    let _ = crate::background::do_sync_spv(pool, &company.id, &app_for_sync, test_mode).await;
                                }
                                use tauri::Emitter;
                                let _ = app_for_sync.emit("sync_completed", serde_json::json!({"source": "tray"}));
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
            commands::companies::create_company,
            commands::companies::update_company,
            commands::companies::delete_company,
            commands::companies::get_next_invoice_number,
            commands::companies::fetch_anaf_company_data,
            // contacts
            commands::contacts::list_contacts,
            commands::contacts::get_contact,
            commands::contacts::create_contact,
            commands::contacts::update_contact,
            commands::contacts::delete_contact,
            commands::contacts::search_contacts,
            // invoices
            commands::invoices::list_invoices,
            commands::invoices::get_invoice,
            commands::invoices::create_invoice_draft,
            commands::invoices::delete_invoice,
            commands::invoices::set_invoice_status,
            commands::invoices::update_invoice_draft,
            commands::invoices::validate_invoice_draft,
            commands::invoices::storno_invoice,
            // received
            commands::received::list_received_invoices,
            commands::received::get_received_invoice,
            commands::received::update_received_status,
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
            // ubl
            commands::ubl::generate_invoice_xml,
            commands::ubl::generate_invoice_pdf,
            commands::ubl::validate_invoice_xml,
            // anaf
            commands::anaf::anaf_authorize,
            commands::anaf::anaf_is_authenticated,
            commands::anaf::anaf_logout,
            commands::anaf::anaf_submit_invoice,
            commands::anaf::anaf_check_invoice_status,
            commands::anaf::anaf_sync_spv,
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
            // integrations
            commands::integrations::smartbill_push_invoice,
            commands::integrations::export_saga_csv,
            commands::integrations::export_winmentor_csv,
            commands::integrations::get_smartbill_credentials,
            commands::xlsx::export_invoices_xlsx,
            // reports
            commands::reports::generate_vat_report,
            commands::reports::export_report,
            // payments
            commands::payments::add_payment,
            commands::payments::list_payments,
            commands::payments::delete_payment,
            commands::payments::get_payment_summary,
            commands::payments::list_payment_summaries,
            // recurring invoices
            commands::recurring::create_recurring_invoice,
            commands::recurring::list_recurring_invoices,
            commands::recurring::delete_recurring_invoice,
            // saft d406
            commands::saft::export_saft_d406,
        ])
        .plugin(tauri_plugin_sql::Builder::default().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
