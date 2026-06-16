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

// SEC-03: chei care pot fi CITITE din frontend, dar NU scrise prin `set_setting` generic —
// trebuie scrise DOAR prin comanda dedicată care le validează. `archive_path_override` redirectează
// unde se scriu backup-urile; comanda `change_archive_location` impune absolut/no-UNC/no-`..`, dar
// `set_setting` ar fi ocolit acea validare. Blocăm scrierea directă (citirea rămâne permisă pentru ca
// UI-ul să afișeze calea curentă).
const WRITE_PROTECTED_KEYS: &[&str] = &[settings::keys::ARCHIVE_PATH_OVERRIDE];

fn is_readonly_key(key: &str) -> bool {
    READONLY_KEYS.contains(&key) || READONLY_PREFIXES.iter().any(|p| key.starts_with(p))
}

fn is_write_protected_key(key: &str) -> bool {
    WRITE_PROTECTED_KEYS.contains(&key)
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
    if is_write_protected_key(&key) {
        return Err(AppError::Validation(format!(
            "Cheia '{key}' nu poate fi scrisă direct; folosiți acțiunea dedicată (validată)."
        )));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_path_override_is_write_protected_but_readable() {
        // SEC-03: must be blocked from the generic set_setting writer …
        assert!(is_write_protected_key(
            settings::keys::ARCHIVE_PATH_OVERRIDE
        ));
        assert!(is_write_protected_key("archive_path_override"));
        // … yet still readable (NOT in the readonly/hidden set, so the UI can show the current path).
        assert!(!is_readonly_key(settings::keys::ARCHIVE_PATH_OVERRIDE));
    }

    #[test]
    fn ordinary_keys_are_neither_readonly_nor_write_protected() {
        for k in ["use_anaf_test_env", "polling_enabled", "quiet_hours"] {
            assert!(!is_readonly_key(k));
            assert!(!is_write_protected_key(k));
        }
    }

    #[test]
    fn license_and_token_keys_stay_readonly() {
        assert!(is_readonly_key("license_key"));
        assert!(is_readonly_key("anaf_token_abc"));
        assert!(is_readonly_key("smartbill_token_x"));
    }
}
