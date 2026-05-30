use std::collections::HashMap;
use tauri::State;

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// SEC-04: chei security-critical care nu pot fi citite/scrise direct din frontend.
//
// `READONLY_KEYS` — chei exacte care semnează licența și protejează
// anti-rollback-ul de ceas. Modificarea lor din JS ar permite extinderea
// trial-ului sau dezactivarea verificărilor.
//
// `READONLY_PREFIXES` — prefixe pentru token-uri/secrete care trebuie să
// rămână în OS keychain, nu în settings (chiar dacă vechi versiuni le-au
// scris aici).
const READONLY_KEYS: &[&str] = &[
    "license_fp_v2",
    "license_last_seen_v2",
    "license_key",
    "license_tier",
    "license_expires_at",
    "license_activated_at",
    "license_machine_id",
];

const READONLY_PREFIXES: &[&str] = &[
    "smartbill_token_", // tokens go to keychain, not settings
    "anaf_token_",      // ANAF tokens are in keychain
];

fn is_readonly_key(key: &str) -> bool {
    READONLY_KEYS.contains(&key) || READONLY_PREFIXES.iter().any(|p| key.starts_with(p))
}

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> AppResult<Option<String>> {
    if is_readonly_key(&key) {
        // Nu expune valori protejate la frontend.
        return Ok(None);
    }
    settings::get(&state.db, &key).await
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    if READONLY_KEYS.contains(&key.as_str()) {
        return Err(AppError::Validation(format!(
            "Cheia '{key}' este protejată și nu poate fi modificată direct."
        )));
    }
    for prefix in READONLY_PREFIXES {
        if key.starts_with(prefix) {
            return Err(AppError::Validation(format!(
                "Cheile cu prefixul '{prefix}' sunt protejate."
            )));
        }
    }
    settings::set(&state.db, &key, &value).await
}

#[tauri::command]
pub async fn get_all_settings(state: State<'_, AppState>) -> AppResult<HashMap<String, String>> {
    let rows = settings::get_all(&state.db).await?;
    Ok(rows
        .into_iter()
        .filter(|(k, _)| !is_readonly_key(k))
        .collect())
}
