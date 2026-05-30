//! Gestionarea licenței și perioadei de probă.
//!
//! Straturi de securitate implementate:
//!
//! 1. **Keychain OS** — La pornirea trial-ului, un marcaj este scris în
//!    keychain-ul sistemului (același mecanism ca token-urile ANAF). Chiar
//!    dacă utilizatorul șterge baza de date SQLite, marcajul persistă și
//!    blochează un al doilea trial pe aceeași mașină.
//!
//! 2. **Fingerprint integritate** — `expires_at`, `tier` și `email` sunt
//!    hash-uite cu SHA-256 + secret embedded în binar. Fingerprint-ul este
//!    stocat în tabelul `settings`. Dacă utilizatorul modifică direct
//!    `expires_at` în SQLite, fingerprint-ul nu mai corespunde → licență
//!    tratată ca expirată.
//!
//! 3. **Anti-rollback ceas** — La fiecare verificare, timestamp-ul curent
//!    este comparat cu `license_last_seen`. Dacă ceasul sistemului a fost
//!    dat înapoi cu mai mult de 5 minute, licența este respinsă.
//!
//! 4. **Machine ID îmbunătățit** — Combină hostname + username + OS,
//!    hash-uit cu SHA-256 (primii 24 de caractere hex).

use keyring::Entry;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::State;

use crate::db::license::{self, License};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── Constante ───────────────────────────────────────────────────────────────

/// Durată perioadă de probă: 14 zile.
const TRIAL_DAYS: i64 = 14;

/// Serviciu keychain pentru marcajul trial (cont fix — NU folosiți machine_id
/// ca account, altfel schimbarea hostname-ului ocolește verificarea).
const TRIAL_KC_SERVICE: &str = "ro.lucaris.efactura.trial.v1";
const TRIAL_KC_ACCOUNT: &str = "trial_status";

/// Cheie settings pentru fingerprint-ul de integritate al licenței.
const FP_SETTINGS_KEY: &str = "license_fp_v2";

/// Cheie settings pentru timestamp-ul last-seen (anti-rollback ceas).
const LAST_SEEN_KEY: &str = "license_last_seen_v2";

/// Secret obfuscat în binar pentru fingerprint-ul de integritate.
/// Nu este securitate perfectă, dar ridică substanțial bara față de un
/// utilizator care editează manual SQLite-ul.
///
/// NOTE: SEC-05 — these constants (INTEGRITY_SECRET, KEY_HMAC_SECRET below)
/// are extractable via `strings` on the compiled binary. Offline license
/// validation is fundamentally limited in this regard. Server-side activation
/// would be required for stronger tamper-resistance.
const INTEGRITY_SECRET: &[u8] = b"RoF@ctura#2026!intgr1ty_K3y\xd4\x9a\x7f\x01\xbe\xc3v2";

// ─── License Key Validation ─────────────────────────────────────────────────

/// Format: XXXX-XXXX-XXXX-XXXXXXXX (A-Z0-9 only).
/// Segment 4 = first 8 hex chars of SHA-256(KEY_HMAC_SECRET || 0x00 || "SEG1-SEG2-SEG3").
/// Offline validation without server — prevents random key guessing.
///
/// SEC-10: checksum was extended from 4 to 8 hex chars (16-bit → 32-bit) for
/// significantly better collision resistance. Legacy 4-char keys are still
/// accepted during transition to avoid breaking deployed installations.
const KEY_HMAC_SECRET: &[u8] = b"RoF@ctura#Key!HMAC2026\xb2\x7f\xd4\x91\xc3\x0a";

/// Returns true if the key format is valid AND the embedded checksum matches.
fn validate_license_key(key: &str) -> bool {
    let parts: Vec<&str> = key.split('-').collect();
    if parts.len() != 4 {
        return false;
    }
    // First three segments are always 4 chars.
    for part in &parts[..3] {
        if part.len() != 4
            || !part
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return false;
        }
    }
    // Last segment is the checksum: 8 chars (new format) or 4 chars (legacy).
    let checksum_part = parts[3];
    if !checksum_part
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return false;
    }

    let payload = format!("{}-{}-{}", parts[0], parts[1], parts[2]);
    let full_checksum = key_checksum(payload.as_bytes());

    match checksum_part.len() {
        8 => checksum_part.eq_ignore_ascii_case(&full_checksum[..8]),
        4 => {
            // Legacy keys (16-bit checksum). Deprecated — keep accepting for
            // transitional compatibility but warn in logs.
            tracing::warn!(
                "Legacy 4-char license checksum detected — please request a new 8-char key."
            );
            checksum_part.eq_ignore_ascii_case(&full_checksum[..4])
        }
        _ => false,
    }
}

/// SHA-256(KEY_HMAC_SECRET || 0x00 || data) → full hex digest.
/// Callers slice the prefix they need (`..4` legacy, `..8` current).
fn key_checksum(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(KEY_HMAC_SECRET);
    h.update(b"\x00");
    h.update(data);
    format!("{:x}", h.finalize())
}

// ─── Machine ID ──────────────────────────────────────────────────────────────

/// Identificator stabil al sesiunii utilizatorului pe această mașină.
/// Combină hostname + username + OS, hash-uite cu SHA-256.
/// Mai rezistent decât simplu `hostname` — schimbarea numelui mașinii nu
/// schimbă username-ul și viceversa.
///
/// SEC-09: Hostname/username sunt citite preferențial via OS API (comenzi de
/// sistem), nu via variabile de mediu, deoarece variabilele de mediu sunt
/// trivial de modificat de utilizator. Variabilele de mediu rămân ca fallback
/// final dacă apelurile OS eșuează.
fn machine_id() -> String {
    let host = read_hostname_os().unwrap_or_else(|| {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "host_unknown".into())
    });
    let user = read_username_os().unwrap_or_else(|| {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user_unknown".into())
    });
    let os = std::env::consts::OS;

    let raw = format!("{}||{}||{}", host, user, os);
    let hash = Sha256::digest(raw.as_bytes());
    // Primii 24 hex chars = 96 biți — suficient pentru unicitate, compact pentru stocare
    format!("{:x}", hash)[..24].to_string()
}

/// Reads the system hostname via OS-specific tooling, bypassing env vars.
/// Returns `None` if the call fails — caller falls back to env vars.
fn read_hostname_os() -> Option<String> {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    let out = Command::new("scutil")
        .arg("--get")
        .arg("LocalHostName")
        .output()
        .ok()?;

    #[cfg(target_os = "windows")]
    let out = Command::new("hostname").output().ok()?;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux/BSD: /etc/hostname is the canonical source.
        if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        // Fall back to `hostname` binary.
        let out = Command::new("hostname").output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed.to_string());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }
}

/// Reads the current user's login name via OS tooling, bypassing env vars.
/// Returns `None` if the call fails — caller falls back to env vars.
fn read_username_os() -> Option<String> {
    use std::process::Command;

    #[cfg(unix)]
    {
        // `id -un` queries the password DB via the OS, not env vars.
        let out = Command::new("id").arg("-un").output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    #[cfg(windows)]
    {
        // `whoami` prints "DOMAIN\user"; strip the domain.
        let out = Command::new("whoami").output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        // Strip everything up to and including the last backslash.
        let bare = trimmed.rsplit('\\').next().unwrap_or(trimmed);
        Some(bare.to_string())
    }
}

// ─── Fingerprint integritate ──────────────────────────────────────────────────

/// SHA-256(secret || fields…) — semnează câmpurile critice ale licenței.
/// Modificarea `expires_at` în SQLite fără a cunoaște secretul → fingerprint invalid.
fn compute_fingerprint(email: &str, mid: &str, expires_at: i64, tier: &str) -> String {
    let mut h = Sha256::new();
    h.update(INTEGRITY_SECRET);
    h.update(b"\x00");
    h.update(email.as_bytes());
    h.update(b"\x00");
    h.update(mid.as_bytes());
    h.update(b"\x00");
    h.update(expires_at.to_string().as_bytes());
    h.update(b"\x00");
    h.update(tier.as_bytes());
    format!("{:x}", h.finalize())
}

// ─── Keychain trial marker ────────────────────────────────────────────────────

/// Returnează `true` dacă pe această sesiune de utilizator trial-ul a fost
/// deja activat (cheia există în keychain, indiferent de conținut).
fn trial_already_used_in_keychain() -> bool {
    let Ok(entry) = Entry::new(TRIAL_KC_SERVICE, TRIAL_KC_ACCOUNT) else {
        return false;
    };
    entry.get_password().is_ok()
}

/// Marchează trial-ul ca folosit în OS keychain.
/// Stochează machine_id-ul curent ca valoare — util pentru debugging.
fn mark_trial_used_in_keychain(mid: &str) {
    if let Ok(entry) = Entry::new(TRIAL_KC_SERVICE, TRIAL_KC_ACCOUNT) {
        let _ = entry.set_password(mid);
    }
}

// ─── Helpers DB ──────────────────────────────────────────────────────────────

/// Scrie o valoare în tabelul `settings` (upsert).
async fn set_setting(pool: &sqlx::SqlitePool, key: &str, value: &str) {
    let _ = sqlx::query(
        "INSERT INTO settings(key, value) VALUES(?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await;
}

/// Citește o valoare din tabelul `settings`. Returnează `None` dacă lipsește.
async fn get_setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String, _>("value").ok())
}

// ─── Comenzi Tauri ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_license(state: State<'_, AppState>) -> AppResult<Option<License>> {
    let Some(mut lic) = license::get(&state.db).await? else {
        return Ok(None);
    };

    let now = chrono::Utc::now().timestamp();

    // Populate computed expiry field
    lic.is_expired = lic.expires_at <= now;

    // For TRIAL licenses, compute days remaining (negative = already expired)
    if lic.tier == "TRIAL" {
        let seconds_remaining = lic.expires_at - now;
        lic.trial_days_remaining = Some(seconds_remaining / 86_400);
    }

    Ok(Some(lic))
}

/// Verifică dacă licența curentă este validă, cu trei straturi de securitate:
/// 1. Expirare de bază (expires_at > now)
/// 2. Fingerprint integritate (pentru TRIAL)
/// 3. Anti-rollback ceas (ultimul timestamp salvat nu poate fi în viitor față de now)
#[tauri::command]
pub async fn check_license_validity(state: State<'_, AppState>) -> AppResult<bool> {
    let pool = &state.db;
    let now = chrono::Utc::now().timestamp();

    // ── 1. Citire licență + expirare de bază ──────────────────────────────
    let row = sqlx::query("SELECT expires_at, tier, email, machine_id FROM license WHERE id = 1")
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?;

    let Some(row) = row else {
        return Ok(false);
    };

    let expires_at: i64 = row.try_get("expires_at").unwrap_or(0);
    let tier: String = row.try_get("tier").unwrap_or_default();
    let email: String = row.try_get("email").unwrap_or_default();
    let stored_mid: String = row.try_get("machine_id").unwrap_or_default();

    if expires_at <= now {
        return Ok(false);
    }

    // ── 2. Fingerprint integritate (TRIAL și SOLO) ───────────────────────
    if tier == "TRIAL" || tier == "SOLO" {
        let mid = machine_id();

        // Dacă machine_id-ul s-a schimbat dramatic față de cel stocat, e suspect
        // (user-ul ar fi putut muta DB-ul pe altă mașină)
        if !stored_mid.is_empty() && stored_mid != mid {
            return Ok(false);
        }

        let email_for_fp = email.to_lowercase();
        let expected_fp = compute_fingerprint(&email_for_fp, &mid, expires_at, &tier);
        let stored_fp = get_setting(pool, FP_SETTINGS_KEY).await;

        match stored_fp {
            Some(fp) if fp == expected_fp => {} // fingerprint OK
            _ => return Ok(false),              // lipsă sau alterat
        }
    }

    // ── 3. Anti-rollback ceas ────────────────────────────────────────────
    let last_seen: Option<i64> = get_setting(pool, LAST_SEEN_KEY)
        .await
        .and_then(|s| s.parse::<i64>().ok());

    if let Some(ls) = last_seen {
        // Dacă acum e cu mai mult de 5 minute înainte față de last_seen → ceas dat înapoi
        if now < ls - 300 {
            return Ok(false);
        }
    }

    // ── 4. Actualizare last_seen ─────────────────────────────────────────
    set_setting(pool, LAST_SEEN_KEY, &now.to_string()).await;

    Ok(true)
}

/// Pornește perioada de probă de 14 zile.
///
/// Securitate:
/// - Verifică keychain-ul OS: dacă marcajul trial există, refuză (chiar dacă DB-ul
///   a fost șters și reinstalat).
/// - Stochează un fingerprint de integritate în settings (protejează `expires_at`).
/// - Marchează trial-ul în keychain imediat după creare.
#[tauri::command]
pub async fn start_trial(state: State<'_, AppState>, email: String) -> AppResult<License> {
    let pool = &state.db;
    let mid = machine_id();

    // Verificare keychain — cel mai puternic strat (persistă după ștergere DB)
    if trial_already_used_in_keychain() {
        return Err(AppError::Validation(
            "Perioada de probă gratuită a fost deja folosită pe această mașină. \
             Achiziționați o licență pentru a continua să utilizați RoFactura."
                .into(),
        ));
    }

    // Verificare suplimentară în DB (edge case: keychain lipsă dar DB există)
    if let Ok(Some(existing)) = license::get(pool).await {
        if existing.tier == "TRIAL" {
            return Err(AppError::Validation(
                "Există deja o perioadă de probă activă sau expirată pe acest dispozitiv.".into(),
            ));
        }
    }

    // Creare înregistrare trial în DB
    let lic = license::start_trial(pool, &email, &mid, TRIAL_DAYS).await?;

    // Fingerprint integritate — protejează expires_at de editare manuală în SQLite
    let fp = compute_fingerprint(&email, &mid, lic.expires_at, "TRIAL");
    set_setting(pool, FP_SETTINGS_KEY, &fp).await;

    // Last-seen inițial
    let now = chrono::Utc::now().timestamp();
    set_setting(pool, LAST_SEEN_KEY, &now.to_string()).await;

    // Marcare keychain — ultimul pas (dacă ceva a eșuat înainte, nu marcăm)
    mark_trial_used_in_keychain(&mid);

    Ok(lic)
}

/// Activează o licență plătită.
/// Validare offline: format XXXX-XXXX-XXXX-XXXX + checksum HMAC-SHA-256 embedded în segmentul 4.
#[tauri::command]
pub async fn activate_license(
    state: State<'_, AppState>,
    key: String,
    email: String,
) -> AppResult<License> {
    let pool = &state.db;
    let key_upper = key.trim().to_uppercase();

    // 1. Validate key format + offline HMAC checksum
    if !validate_license_key(&key_upper) {
        return Err(AppError::Validation(
            "Cheia de licență este invalidă. Verificați că ați introdus corect \
             cheia primită prin email (format: XXXX-XXXX-XXXX-XXXX)."
                .into(),
        ));
    }

    // 2. Validate email
    if email.trim().is_empty() || !email.contains('@') {
        return Err(AppError::Validation(
            "Adresa de email este obligatorie pentru activarea licenței.".into(),
        ));
    }

    let mid = machine_id();
    let one_year = chrono::Utc::now().timestamp() + 365 * 86_400;

    let lic = license::activate(pool, &key_upper, "SOLO", one_year, email.trim(), &mid).await?;

    // 3. Apply SHA-256 integrity fingerprint to SOLO license (prevents SQLite tampering)
    let email_lower = email.trim().to_lowercase();
    let fp = compute_fingerprint(&email_lower, &mid, lic.expires_at, "SOLO");
    set_setting(pool, FP_SETTINGS_KEY, &fp).await;

    // 4. Anti-rollback timestamp
    let now = chrono::Utc::now().timestamp();
    set_setting(pool, LAST_SEEN_KEY, &now.to_string()).await;

    Ok(lic)
}
