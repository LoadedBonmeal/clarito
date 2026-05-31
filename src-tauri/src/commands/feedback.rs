//! Diagnostic gather + mailto builder for the Suport section.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

use crate::commands::license;
use crate::db::license as license_db;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LicenseSummary {
    pub tier: String,
    pub days_remaining: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub machine_id_hash: String,
    pub log_tail: Vec<String>,
    pub license_summary: LicenseSummary,
}

const SUPPORT_EMAIL: &str = "support@lucaris.ro";
const MAX_LOG_LINES: usize = 50;
const MAX_LINE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 1900;

#[tauri::command]
pub async fn gather_diagnostic(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<DiagnosticReport> {
    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let machine_id_hash = license::machine_id_for_diagnostic();

    let log_tail = read_log_tail(&app).await.unwrap_or_default();

    let license_summary = match license_db::get(&state.db).await? {
        Some(lic) => {
            let now = chrono::Utc::now().timestamp();
            let days = if lic.tier == "TRIAL" {
                Some((lic.expires_at - now) / 86_400)
            } else {
                None
            };
            LicenseSummary {
                tier: lic.tier,
                days_remaining: days,
            }
        }
        None => LicenseSummary {
            tier: "NONE".into(),
            days_remaining: None,
        },
    };

    Ok(DiagnosticReport {
        app_version,
        os,
        arch,
        machine_id_hash,
        log_tail,
        license_summary,
    })
}

async fn read_log_tail(app: &tauri::AppHandle) -> Option<Vec<String>> {
    let log_dir: PathBuf = app.path().app_log_dir().ok()?;
    let path = log_dir.join("efactura.log");
    let bytes = tokio::fs::read_to_string(&path).await.ok()?;
    let mut lines: Vec<String> = bytes
        .lines()
        .rev()
        .take(MAX_LOG_LINES)
        .map(|l| {
            if l.len() > MAX_LINE_CHARS {
                format!("{}…", &l[..MAX_LINE_CHARS])
            } else {
                l.to_string()
            }
        })
        .collect();
    lines.reverse();
    Some(lines)
}

#[tauri::command]
pub fn build_feedback_mailto(
    report: DiagnosticReport,
    user_message: Option<String>,
) -> AppResult<String> {
    let mut body = String::new();
    if let Some(msg) = user_message.as_ref() {
        if !msg.trim().is_empty() {
            body.push_str(msg.trim());
            body.push_str("\n\n");
        }
    }
    body.push_str("---\n");
    body.push_str("Diagnostic:\n");
    body.push_str(&format!("App: RoFactura {}\n", report.app_version));
    body.push_str(&format!("OS: {}/{}\n", report.os, report.arch));
    body.push_str(&format!("Machine: {}\n", report.machine_id_hash));
    body.push_str(&format!(
        "License: {}{}\n\n",
        report.license_summary.tier,
        report
            .license_summary
            .days_remaining
            .map(|d| format!(" ({d}d remaining)"))
            .unwrap_or_default(),
    ));
    body.push_str("Log tail (last 50):\n");
    for line in &report.log_tail {
        body.push_str(line);
        body.push('\n');
    }

    // Windows mailto: limit ≈ 2048 chars. Truncate if needed.
    if body.len() > MAX_BODY_CHARS {
        let mut truncated: String = body.chars().take(MAX_BODY_CHARS).collect();
        truncated.push_str(if cfg!(target_os = "macos") {
            "\n[…truncat — trimite logs manual din ~/Library/Logs/com.lucaris.efactura/]"
        } else if cfg!(target_os = "windows") {
            "\n[…truncat — trimite logs manual din %APPDATA%\\com.lucaris.efactura\\logs\\]"
        } else {
            "\n[…truncat — trimite manual fișierul de log din directorul de loguri al aplicației]"
        });
        body = truncated;
    }

    let subject = format!("[RoFactura v{}] Feedback", report.app_version);
    Ok(format!(
        "mailto:{}?subject={}&body={}",
        SUPPORT_EMAIL,
        urlencoding_encode(&subject),
        urlencoding_encode(&body),
    ))
}

/// Inline minimal URL encoder (avoids adding the urlencoding crate dep).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DiagnosticReport {
        DiagnosticReport {
            app_version: "0.2.0".into(),
            os: "macos".into(),
            arch: "arm64".into(),
            machine_id_hash: "a4f2b30000000000".into(),
            log_tail: vec!["line 1".into(), "line 2".into()],
            license_summary: LicenseSummary {
                tier: "TRIAL".into(),
                days_remaining: Some(8),
            },
        }
    }

    #[test]
    fn mailto_url_starts_with_scheme() {
        let url = build_feedback_mailto(sample(), None).unwrap();
        assert!(url.starts_with("mailto:support@lucaris.ro?"));
    }

    #[test]
    fn mailto_url_truncates_long_body() {
        let mut report = sample();
        report.log_tail = (0..200)
            .map(|i| format!("log line {i} ").repeat(20))
            .collect();
        let url = build_feedback_mailto(report, None).unwrap();
        // truncated marker present
        assert!(url.contains("trun"));
    }

    #[test]
    fn user_message_is_included_when_non_empty() {
        let url = build_feedback_mailto(sample(), Some("test message".into())).unwrap();
        assert!(url.contains("test%20message") || url.contains("test+message"));
    }
}
