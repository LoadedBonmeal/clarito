//! Native OS notification helper (tauri-plugin-notification)
//!
//! Preference keys in settings DB:
//!   notif_pref_{type} = "os" | "inapp" | "off"
//! where {type} is one of: validated, rejected, received, cert_expiring, cert_expired
//!
//! Default (key absent) = "os" — show OS desktop notifications.

use chrono::Timelike;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Check if OS notifications are allowed for a given notification type.
/// Returns `false` if quiet hours are active OR if the per-type pref is "inapp"/"off".
async fn should_notify_os(app: &AppHandle, notif_type: &str) -> bool {
    let pool = app.state::<crate::state::AppState>();

    // 1. Check global quiet hours
    let quiet = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(crate::db::settings::keys::NOTIFICATIONS_QUIET_HOURS)
        .fetch_optional(&pool.db)
        .await
        .ok()
        .flatten()
        .and_then(|row| {
            use sqlx::Row;
            row.try_get::<String, _>("value").ok()
        })
        .map(|v| v == "1")
        .unwrap_or(false);

    if quiet {
        let hour = chrono::Local::now().hour();
        // Quiet hours: 22:00–07:00
        if !(7..22).contains(&hour) {
            return false;
        }
    }

    // 2. Check per-type preference: "os" (default) | "inapp" | "off"
    let pref_key = format!("notif_pref_{}", notif_type);
    let pref = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(&pref_key)
        .fetch_optional(&pool.db)
        .await
        .ok()
        .flatten()
        .and_then(|row| {
            use sqlx::Row;
            row.try_get::<String, _>("value").ok()
        })
        .unwrap_or_else(|| "os".to_string()); // absent → default "os"

    pref == "os"
}

/// Generic notification — pass `notif_type` for per-type preference lookup.
async fn notify_typed(app: &AppHandle, notif_type: &str, title: &str, body: &str) {
    if should_notify_os(app, notif_type).await {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

/// Generic notify — kept for backward compatibility (uses "general" preference key).
pub async fn notify(app: &AppHandle, title: &str, body: &str) {
    notify_typed(app, "general", title, body).await;
}

pub async fn notify_invoice_validated(app: &AppHandle, invoice_number: &str) {
    notify_typed(
        app,
        "validated",
        "✓ Factură validată",
        &format!("Factura {} a fost validată de ANAF.", invoice_number),
    )
    .await;
}

pub async fn notify_invoice_rejected(app: &AppHandle, invoice_number: &str, reason: &str) {
    let short: String = reason.chars().take(80).collect();
    notify_typed(
        app,
        "rejected",
        "✗ Factură respinsă",
        &format!("Factura {} a fost respinsă: {}", invoice_number, short),
    )
    .await;
}

pub async fn notify_new_received(app: &AppHandle, count: u32) {
    if count > 0 {
        notify_typed(
            app,
            "received",
            "📥 Facturi noi primite",
            &format!("{} facturi noi descărcate din SPV.", count),
        )
        .await;
    }
}

pub async fn notify_certificate_expiring(app: &AppHandle, company_name: &str, days: i64) {
    let notif_type = if days <= 7 {
        "cert_expired"
    } else {
        "cert_expiring"
    };
    notify_typed(
        app,
        notif_type,
        "⏰ Certificat SPV expiră",
        &format!(
            "Certificatul pentru {} expiră în {} zile. Reautorizați din Setări.",
            company_name, days
        ),
    )
    .await;
}
