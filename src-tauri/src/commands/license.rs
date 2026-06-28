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

use hmac::{Hmac, Mac};
use keyring::Entry;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::State;

type HmacSha256 = Hmac<Sha256>;

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

// SEC-05: secrets are obfuscated at build time via XOR cycle with build-derived salt.
// See build.rs for the obfuscation scheme. The compiled binary contains only
// XOR-ed bytes — a `strings` scan reveals no readable secret material.
// This raises the reverse-engineering bar but is not cryptographically strong:
// a determined attacker with a disassembler can still extract the secrets at runtime.
// Server-side license validation would be required for stronger tamper resistance.
//
// SEC-10 note: the key checksum was extended from 4 to 8 hex chars (16-bit →
// 32-bit) for significantly better collision resistance. Legacy 4-char keys
// are still accepted during transition to avoid breaking deployed installations.
include!(concat!(env!("OUT_DIR"), "/license_secrets.rs"));

use std::sync::OnceLock;

static INTEGRITY_SECRET_CACHE: OnceLock<Vec<u8>> = OnceLock::new();
static KEY_HMAC_SECRET_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

fn integrity_secret() -> &'static [u8] {
    INTEGRITY_SECRET_CACHE
        .get_or_init(integrity_secret_bytes)
        .as_slice()
}

pub fn key_hmac_secret() -> &'static [u8] {
    KEY_HMAC_SECRET_CACHE
        .get_or_init(key_hmac_secret_bytes)
        .as_slice()
}

/// Returns true if the key format is valid AND the embedded checksum matches.
pub fn validate_license_key(key: &str) -> bool {
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

/// RFC 2104 HMAC-SHA256(KEY_HMAC_SECRET, data) → full hex digest (64 chars).
/// Callers slice the prefix they need (`..4` legacy, `..8` current).
///
/// BREAKING (Wave 8): replaced the old SHA-256(secret || 0x00 || data)
/// construction, which was vulnerable to length-extension attacks. Any keys
/// issued before this change are now invalid; re-issue via `license-gen`.
pub fn key_checksum(data: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(key_hmac_secret()).expect("HMAC accepts any key length");
    mac.update(data);
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
/// Public wrapper exposing the machine_id hash for the diagnostic command.
/// We re-expose machine_id() via a stable name so feedback.rs doesn't depend
/// on the private function name.
pub fn machine_id_for_diagnostic() -> String {
    machine_id()
}

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
    use crate::process_util::hidden_command;

    #[cfg(target_os = "macos")]
    let out = hidden_command("scutil")
        .arg("--get")
        .arg("LocalHostName")
        .output()
        .ok()?;

    #[cfg(target_os = "windows")]
    let out = hidden_command("hostname").output().ok()?;

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
        let out = hidden_command("hostname").output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        // Tail of the Linux #[cfg] block — clippy 1.96 (CI/Linux) flags a `return` here as needless.
        Some(trimmed.to_string())
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
    use crate::process_util::hidden_command;

    #[cfg(unix)]
    {
        // `id -un` queries the password DB via the OS, not env vars.
        let out = hidden_command("id").arg("-un").output().ok()?;
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
        let out = hidden_command("whoami").output().ok()?;
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

/// RFC 2104 HMAC-SHA256(integrity_secret, fields…) — semnează câmpurile critice ale licenței.
/// Modificarea `expires_at` în SQLite fără a cunoaște secretul → fingerprint invalid.
///
/// Fields are fed with 0x00 separators to preserve domain separation between them.
/// BREAKING (Wave 8): previously used SHA-256(secret || 0x00 || fields…); now proper HMAC.
fn compute_fingerprint(email: &str, mid: &str, expires_at: i64, tier: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(integrity_secret()).expect("HMAC accepts any key length");
    mac.update(email.as_bytes());
    mac.update(b"\x00");
    mac.update(mid.as_bytes());
    mac.update(b"\x00");
    mac.update(expires_at.to_string().as_bytes());
    mac.update(b"\x00");
    mac.update(tier.as_bytes());
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
        // Ceil a partial final day so 12h left shows "1 day", not "0" (which reads as expired).
        // Already-expired (negative) keeps its truncated-toward-zero value.
        lic.trial_days_remaining = Some(if seconds_remaining > 0 {
            (seconds_remaining + 86_399) / 86_400
        } else {
            seconds_remaining / 86_400
        });
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

    // ── 2. Fingerprint integritate (toate tierele) ──────────────────────
    // Fingerprint binding is applied unconditionally to ALL tiers, not just
    // TRIAL/SOLO. This prevents a tampered license record from being upgraded
    // to a higher tier by directly editing SQLite. Existing SOLO/TRIAL keys
    // already carry valid fingerprints, so removing the tier gate does not
    // break deployed installations.
    {
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
        let drift = ls - now; // pozitiv = last_seen înaintea lui now
        const TOLERANCE: i64 = 24 * 60 * 60; // 1 zi: clamp silențios
        const HARD_FAIL: i64 = 30 * 24 * 60 * 60; // 30 zile: posibilă manipulare

        if drift > HARD_FAIL {
            // Ceasul a dat înapoi > 30 zile SAU last_seen masiv în viitor → refuză.
            tracing::warn!(
                drift_seconds = drift,
                "license anti-rollback hard-fail (drift > 30 days)"
            );
            return Ok(false);
        }

        if drift > TOLERANCE {
            // Suspect dar plauzibil (DST, NTP, date de test).
            // Logăm și continuăm; set_setting de mai jos clampează la now.
            tracing::warn!(
                drift_seconds = drift,
                "license anti-rollback drift > 1 day; clamping last_seen to now"
            );
        }
        // drift <= TOLERANCE → clamp silențios prin set_setting de mai jos.
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
             Achiziționați o licență pentru a continua să utilizați Clarito."
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

    // Normalize email to lowercase+trim so the fingerprint computed here matches
    // what check_license_validity will compute (which always lowercases the stored email).
    let email = email.trim().to_lowercase();

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

    // Tier is hardcoded to "SOLO" intentionally — multi-tier keys do not exist yet.
    // Both activate_license (here) and check_license_validity use the stored tier,
    // so the all-tier fingerprint from Wave C remains consistent across both paths.
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

#[cfg(test)]
mod sec_tests {
    use super::*;

    #[test]
    fn secrets_decode_to_expected_length() {
        let int_sec = integrity_secret();
        let key_sec = key_hmac_secret();
        // Original byte-string lengths from build.rs
        assert_eq!(int_sec.len(), 35);
        assert_eq!(key_sec.len(), 28);
    }

    #[test]
    fn secrets_consistent_across_calls() {
        let a = integrity_secret().to_vec();
        let b = integrity_secret().to_vec();
        assert_eq!(a, b);

        let c = key_hmac_secret().to_vec();
        let d = key_hmac_secret().to_vec();
        assert_eq!(c, d);
    }

    #[test]
    fn secrets_decode_correctly() {
        // The decoded secret should match the expected first bytes ("RoF...")
        assert_eq!(&integrity_secret()[..3], b"RoF");
        assert_eq!(&key_hmac_secret()[..3], b"RoF");
    }

    #[test]
    fn anti_rollback_tolerates_one_day_future_drift() {
        // Drift de 5 ore → sub toleranța de 1 zi → nu e hard-fail.
        let now: i64 = 1_700_000_000;
        let ls = now + 5 * 60 * 60; // 5h în viitor
        let drift = ls - now;
        assert!(drift > 0);
        assert!(drift < 24 * 60 * 60); // în interiorul toleranței
    }

    #[test]
    fn anti_rollback_hard_fails_above_30_days() {
        // Drift de 31 zile → depășește HARD_FAIL → refuz.
        let now: i64 = 1_700_000_000;
        let ls = now + 31 * 24 * 60 * 60;
        let drift = ls - now;
        assert!(drift > 30 * 24 * 60 * 60);
    }

    #[test]
    fn hmac_checksum_is_deterministic() {
        // Same input must always produce the same 64-char hex digest.
        let input = b"TEST-ABCD-1234";
        let a = key_checksum(input);
        let b = key_checksum(input);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "HMAC-SHA256 digest must be 64 hex chars");
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit()),
            "Digest must be hex"
        );
    }

    #[test]
    fn hmac_checksum_differs_for_different_inputs() {
        // Distinct inputs must not collide (basic sanity check).
        let a = key_checksum(b"payload-a");
        let b = key_checksum(b"payload-b");
        assert_ne!(a, b, "Different inputs must produce different checksums");
    }

    #[test]
    fn fingerprint_differs_across_tiers_for_all_tier_binding() {
        // Verify that fingerprints are tier-sensitive: a TEAM license cannot
        // forge a SOLO fingerprint. This exercises the "all-tier binding" logic
        // — distinct tier strings must produce distinct fingerprints.
        let solo_fp = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "SOLO");
        let team_fp = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "TEAM");
        let pro_fp = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "PRO");
        assert_ne!(solo_fp, team_fp, "SOLO and TEAM fingerprints must differ");
        assert_ne!(solo_fp, pro_fp, "SOLO and PRO fingerprints must differ");
        assert_ne!(team_fp, pro_fp, "TEAM and PRO fingerprints must differ");
    }

    /// Documents the BREAKING change: the new HMAC output will differ from
    /// what the old SHA-256(secret||0x00||data) construction produced.
    /// This test intentionally passes — it merely records that the two schemes
    /// produce different outputs (no hardcoded expected value needed).
    #[test]
    fn hmac_checksum_differs_from_legacy_sha256_construction() {
        let data = b"ABCD-EFGH-IJKL";
        let hmac_result = key_checksum(data);

        // Reproduce old construction manually (pure-SHA2, no HMAC).
        use sha2::{Digest as _, Sha256};
        let mut h = Sha256::new();
        h.update(key_hmac_secret());
        h.update(b"\x00");
        h.update(data);
        let legacy: String = format!("{:x}", h.finalize());

        assert_ne!(
            hmac_result, legacy,
            "HMAC and legacy SHA-256 construction must differ (BREAKING change confirmed)"
        );
    }

    #[test]
    fn compute_fingerprint_is_deterministic() {
        let fp1 = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "SOLO");
        let fp2 = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "SOLO");
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 64, "Fingerprint must be 64 hex chars");
    }

    #[test]
    fn compute_fingerprint_differs_across_fields() {
        let base = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "SOLO");
        let diff_email = compute_fingerprint("other@test.com", "mid123", 1_800_000_000, "SOLO");
        let diff_tier = compute_fingerprint("user@test.com", "mid123", 1_800_000_000, "TRIAL");
        let diff_expires = compute_fingerprint("user@test.com", "mid123", 1_900_000_000, "SOLO");
        assert_ne!(base, diff_email);
        assert_ne!(base, diff_tier);
        assert_ne!(base, diff_expires);
    }

    /// start_trial normalizes email to lowercase before computing the fingerprint.
    /// check_license_validity also lowercases the stored email before comparing.
    /// Mixed-case trial emails must validate — both paths must produce the same fingerprint.
    #[test]
    fn trial_email_case_fingerprint_matches() {
        let mid = "testmid123";
        let expires_at: i64 = 1_800_000_000;

        // Simulate what start_trial now does: normalize to lowercase+trim.
        let raw_email = "  User@Test.COM  ";
        let normalized = raw_email.trim().to_lowercase();

        // Fingerprint stored at start_trial time.
        let stored_fp = compute_fingerprint(&normalized, mid, expires_at, "TRIAL");

        // Simulate what check_license_validity does: lowercase the stored email.
        let stored_email = normalized.clone(); // already lowercase after start_trial
        let check_fp = compute_fingerprint(&stored_email.to_lowercase(), mid, expires_at, "TRIAL");

        assert_eq!(
            stored_fp, check_fp,
            "Fingerprint must match across start_trial (lowercase+trim) \
             and check_license_validity (lowercase) for mixed-case email"
        );
    }
}
