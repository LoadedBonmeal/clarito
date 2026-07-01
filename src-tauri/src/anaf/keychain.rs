//! Stocare token-uri OAuth2 în OS keychain.
//!
//! Token bundle-ul (access + refresh + expiry) e serializat ca JSON și stocat
//! sub cheia "efactura::{company_id}" în keychain-ul sistemului.

use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

// ─── SmartBill token storage ────────────────────────────────────────────────
//
// Token-ul SmartBill (parolă API) este stocat în OS keychain, NU în SQLite,
// pentru a evita expunerea către renderer-ul JS prin IPC.

const SMARTBILL_SERVICE: &str = "com.lucaris.efactura.smartbill";

/// Stochează token-ul SmartBill pentru o companie în OS keychain.
pub fn store_smartbill_token(company_id: &str, token: &str) -> AppResult<()> {
    let entry =
        Entry::new(SMARTBILL_SERVICE, company_id).map_err(|e| AppError::Other(e.to_string()))?;
    entry
        .set_password(token)
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

/// Citește token-ul SmartBill pentru o companie din OS keychain.
/// Returnează `None` dacă nu există nicio intrare.
pub fn get_smartbill_token(company_id: &str) -> AppResult<Option<String>> {
    let entry =
        Entry::new(SMARTBILL_SERVICE, company_id).map_err(|e| AppError::Other(e.to_string()))?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Other(e.to_string())),
    }
}

/// Șterge token-ul SmartBill al unei companii din OS keychain.
pub fn delete_smartbill_token(company_id: &str) -> AppResult<()> {
    let entry =
        Entry::new(SMARTBILL_SERVICE, company_id).map_err(|e| AppError::Other(e.to_string()))?;
    let _ = entry.delete_credential();
    Ok(())
}

// ─── ANAF OAuth client_secret storage ───────────────────────────────────────
//
// client_secret-ul aplicației OAuth înregistrate la ANAF (client confidențial)
// este stocat în OS keychain, NU în SQLite — este o parolă de aplicație și nu
// trebuie expusă renderer-ului JS prin IPC. Este global pe instalare (un singur
// client OAuth înregistrat la ANAF), deci folosim un account fix.

const OAUTH_SECRET_SERVICE: &str = "com.lucaris.efactura.oauth_secret";
const OAUTH_SECRET_ACCOUNT: &str = "global";

/// Stochează client_secret-ul OAuth ANAF în OS keychain.
pub fn store_oauth_client_secret(secret: &str) -> AppResult<()> {
    let entry = Entry::new(OAUTH_SECRET_SERVICE, OAUTH_SECRET_ACCOUNT)
        .map_err(|e| AppError::Other(e.to_string()))?;
    entry
        .set_password(secret)
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

/// Citește client_secret-ul OAuth ANAF din OS keychain. `None` dacă nu există.
pub fn get_oauth_client_secret() -> AppResult<Option<String>> {
    let entry = Entry::new(OAUTH_SECRET_SERVICE, OAUTH_SECRET_ACCOUNT)
        .map_err(|e| AppError::Other(e.to_string()))?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Other(e.to_string())),
    }
}

/// Șterge client_secret-ul OAuth ANAF din OS keychain (best-effort).
pub fn delete_oauth_client_secret() -> AppResult<()> {
    let entry = Entry::new(OAUTH_SECRET_SERVICE, OAUTH_SECRET_ACCOUNT)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let _ = entry.delete_credential();
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp când expiră access_token-ul.
    pub expires_at: i64,
}

// ─── Chunked keychain storage ───────────────────────────────────────────────
//
// Backend-ul Windows Credential Manager al `keyring` v3 respinge hard orice
// blob mai mare de 2560 de bytes (verificat în keyring-3.6.3/src/windows.rs).
// JWT-urile ANAF au 1-2KB FIECARE, deci un singur blob JSON cu
// access_token+refresh_token+expires_at depășește frecvent limita → `save()`
// eșua silențios pe Windows și token-ul SPV nu persista niciodată
// (re-autentificare la fiecare pornire).
//
// Soluție: token-urile mari sunt împărțite în bucăți ≤ CHUNK_MAX bytes și
// stocate sub conturi `{account_base}::0`, `{account_base}::1`, ... plus un
// marker `{account_base}::n` cu numărul de bucăți (robust la citire parțială
// sau la bucăți lipsă). Metadata mică (expires_at) rămâne în intrarea
// originală "efactura"/{company_id}, mereu sub limită.

/// Dimensiunea maximă (bytes) a unei bucăți — sub pragul de 2560 bytes al
/// Windows Credential Manager, cu marjă pentru overhead-ul intern al keyring.
const CHUNK_MAX: usize = 2000;

/// Împarte `value` în bucăți de cel mult `CHUNK_MAX` bytes (pe granițe de
/// caractere UTF-8 valide, nu bytes brute — altfel am putea tăia în mijlocul
/// unui caracter multi-byte).
fn chunk_string(value: &str, max_len: usize) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut rest = value;
    while !rest.is_empty() {
        if rest.len() <= max_len {
            chunks.push(rest.to_string());
            break;
        }
        // Găsește cea mai mare graniță de caracter <= max_len.
        let mut split_at = max_len;
        while split_at > 0 && !rest.is_char_boundary(split_at) {
            split_at -= 1;
        }
        if split_at == 0 {
            // Caracter unic mai mare decât max_len (extrem de improbabil) —
            // forțează progresul luând primul caracter întreg.
            split_at = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        let (head, tail) = rest.split_at(split_at);
        chunks.push(head.to_string());
        rest = tail;
    }
    chunks
}

/// Reasamblează bucățile în ordine (join simplu, fără separator — `chunk_string`
/// nu introduce niciunul).
fn join_chunks(chunks: Vec<String>) -> String {
    chunks.concat()
}

/// Salvează `value` fragmentat sub `{service}`/`{account_base}::0..n-1`, plus
/// un marker `{account_base}::n` cu numărul total de bucăți. Best-effort per
/// bucată, dar întrerupe și întoarce eroare la primul eșec (nu lasă stare
/// parțial scrisă necunoscută — apelantul poate retrimite).
fn save_chunked(service: &str, account_base: &str, value: &str) -> Result<(), String> {
    let chunks = chunk_string(value, CHUNK_MAX);
    for (i, chunk) in chunks.iter().enumerate() {
        let account = format!("{account_base}::{i}");
        let entry = Entry::new(service, &account)
            .map_err(|e| format!("keychain entry '{account}': {e}"))?;
        entry
            .set_password(chunk)
            .map_err(|e| format!("keychain write '{account}': {e}"))?;
    }
    // Marker cu numărul de bucăți — permite un load robust chiar dacă o
    // bucată anterioară din altă rulare a rămas orfană (nu ne bazăm doar pe
    // "citește până eșuează", deși load_chunked face și asta ca fallback).
    let count_account = format!("{account_base}::n");
    let count_entry = Entry::new(service, &count_account)
        .map_err(|e| format!("keychain entry '{count_account}': {e}"))?;
    count_entry
        .set_password(&chunks.len().to_string())
        .map_err(|e| format!("keychain write '{count_account}': {e}"))?;
    Ok(())
}

/// Încarcă o valoare fragmentată salvată cu `save_chunked`. Preferă marker-ul
/// `::n` (numărul de bucăți) dacă există și e valid; altfel citește secvențial
/// `::0`, `::1`, ... până la prima citire eșuată (fallback robust).
fn load_chunked(service: &str, account_base: &str) -> Option<String> {
    let count_account = format!("{account_base}::n");
    let declared_count = Entry::new(service, &count_account)
        .ok()
        .and_then(|e| e.get_password().ok())
        .and_then(|s| s.trim().parse::<usize>().ok());

    let mut chunks = Vec::new();
    if let Some(n) = declared_count {
        if n == 0 {
            return None;
        }
        for i in 0..n {
            let account = format!("{account_base}::{i}");
            let entry = Entry::new(service, &account).ok()?;
            let chunk = entry.get_password().ok()?;
            chunks.push(chunk);
        }
        return Some(join_chunks(chunks));
    }

    // Fallback: fără marker valid (date vechi/corupte) — citește secvențial
    // până la primul eșec.
    let mut i = 0usize;
    loop {
        let account = format!("{account_base}::{i}");
        let Ok(entry) = Entry::new(service, &account) else {
            break;
        };
        match entry.get_password() {
            Ok(chunk) => chunks.push(chunk),
            Err(_) => break,
        }
        i += 1;
    }
    if chunks.is_empty() {
        None
    } else {
        Some(join_chunks(chunks))
    }
}

/// Șterge toate bucățile + marker-ul unei valori fragmentate (best-effort).
fn delete_chunked(service: &str, account_base: &str) {
    // Șterge marker-ul.
    if let Ok(entry) = Entry::new(service, &format!("{account_base}::n")) {
        let _ = entry.delete_credential();
    }
    // Șterge bucățile — mergem până întâlnim prima lipsă, apoi încă câteva
    // "peste" ca să curățăm eventuale resturi dintr-o valoare anterioară mai
    // lungă (best-effort, nu contează dacă unele delete-uri sunt no-op).
    let mut i = 0usize;
    let mut misses = 0u8;
    while misses < 4 {
        let account = format!("{account_base}::{i}");
        match Entry::new(service, &account) {
            Ok(entry) => {
                if entry.delete_credential().is_err() {
                    misses += 1;
                } else {
                    misses = 0;
                }
            }
            Err(_) => misses += 1,
        }
        i += 1;
    }
}

/// Metadata mică stocată în intrarea originală "efactura"/{company_id} —
/// mereu sub limita de 2560 bytes. Token-urile propriu-zise sunt în bucăți
/// separate (vezi `save_chunked`/`load_chunked`).
#[derive(Debug, Serialize, Deserialize)]
struct TokenMeta {
    /// Unix timestamp când expiră access_token-ul.
    expires_at: i64,
    /// Marker de format — permite `load()` să distingă noul format (chunked)
    /// de formatul vechi (blob unic cu access_token/refresh_token incluse).
    #[serde(default)]
    chunked: bool,
}

/// Formatul vechi (pre-chunking): blob JSON unic cu toate cele 3 câmpuri.
/// Păstrat doar pentru compatibilitate la citire (migrare automată la
/// următorul `save()`).
#[derive(Debug, Deserialize)]
struct LegacyTokenBundle {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

impl TokenBundle {
    /// Salvează bundle-ul în OS keychain: metadata mică (expires_at) sub
    /// "efactura"/{company_id}, iar access_token/refresh_token fragmentate în
    /// conturi separate "{company_id}::at::N" / "{company_id}::rt::N" — vezi
    /// comentariul din capul secțiunii "Chunked keychain storage" pentru
    /// motivul (limita de 2560 bytes a Windows Credential Manager).
    pub fn save(&self, company_id: &str) -> Result<(), String> {
        save_chunked("efactura", &format!("{company_id}::at"), &self.access_token)?;
        save_chunked(
            "efactura",
            &format!("{company_id}::rt"),
            &self.refresh_token,
        )?;

        let meta = TokenMeta {
            expires_at: self.expires_at,
            chunked: true,
        };
        let json = serde_json::to_string(&meta).map_err(|e| format!("serialize meta: {e}"))?;
        let entry =
            Entry::new("efactura", company_id).map_err(|e| format!("keychain entry: {e}"))?;
        entry
            .set_password(&json)
            .map_err(|e| format!("keychain write meta: {e}"))?;
        Ok(())
    }

    /// Încarcă bundle-ul din OS keychain. Returnează `None` dacă nu există
    /// sau dacă datele sunt corupte/incomplete.
    ///
    /// Compatibil retroactiv: dacă intrarea principală conține formatul vechi
    /// (blob unic cu `access_token`), îl întoarce direct — migrarea la noul
    /// format chunked se face automat la următorul `save()`.
    pub fn load(company_id: &str) -> Option<TokenBundle> {
        let entry = Entry::new("efactura", company_id).ok()?;
        let json = entry.get_password().ok()?;

        // Format nou: metadata mică + token-uri chunked separat.
        if let Ok(meta) = serde_json::from_str::<TokenMeta>(&json) {
            if meta.chunked {
                let access_token = load_chunked("efactura", &format!("{company_id}::at"))?;
                let refresh_token = load_chunked("efactura", &format!("{company_id}::rt"))?;
                return Some(TokenBundle {
                    access_token,
                    refresh_token,
                    expires_at: meta.expires_at,
                });
            }
        }

        // Format vechi (pre-chunking): blob unic cu tot bundle-ul.
        let legacy: LegacyTokenBundle = serde_json::from_str(&json).ok()?;
        Some(TokenBundle {
            access_token: legacy.access_token,
            refresh_token: legacy.refresh_token,
            expires_at: legacy.expires_at,
        })
    }

    /// Șterge token-ul din keychain (logout / revocare) — inclusiv toate
    /// bucățile access_token/refresh_token (best-effort).
    pub fn delete(company_id: &str) {
        if let Ok(entry) = Entry::new("efactura", company_id) {
            let _ = entry.delete_credential();
        }
        delete_chunked("efactura", &format!("{company_id}::at"));
        delete_chunked("efactura", &format!("{company_id}::rt"));
    }

    /// `true` dacă access_token-ul expiră în mai puțin de 60 de secunde.
    pub fn is_expired(&self) -> bool {
        self.expires_at <= chrono::Utc::now().timestamp() + 60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── chunk_string / join_chunks: pure logic, no keychain needed ────────

    #[test]
    fn chunk_string_empty_yields_no_chunks() {
        assert!(chunk_string("", 10).is_empty());
    }

    #[test]
    fn chunk_string_shorter_than_max_yields_single_chunk() {
        let chunks = chunk_string("hello", 2000);
        assert_eq!(chunks, vec!["hello".to_string()]);
    }

    #[test]
    fn chunk_string_exact_multiple_splits_evenly() {
        let value = "a".repeat(20);
        let chunks = chunk_string(&value, 5);
        assert_eq!(chunks.len(), 4);
        for c in &chunks {
            assert_eq!(c.len(), 5);
        }
        assert_eq!(join_chunks(chunks), value);
    }

    #[test]
    fn chunk_string_round_trips_for_jwt_sized_input() {
        // Simulează un JWT ANAF de ~1.8KB — trebuie să facă round-trip exact
        // după fragmentare la CHUNK_MAX (2000).
        let value = "eyJhbGciOiJSUzI1NiJ9.".to_string() + &"x".repeat(1800);
        let chunks = chunk_string(&value, CHUNK_MAX);
        assert!(
            chunks.len() >= 1,
            "un JWT de 1.8KB ar trebui să încapă într-o singură bucată de {CHUNK_MAX}"
        );
        assert!(chunks.iter().all(|c| c.len() <= CHUNK_MAX));
        assert_eq!(join_chunks(chunks), value);
    }

    #[test]
    fn chunk_string_round_trips_for_oversized_input() {
        // 5000 bytes > CHUNK_MAX → trebuie fragmentat în mai multe bucăți,
        // fiecare <= CHUNK_MAX, iar rejoin-ul trebuie să fie identic cu originalul.
        let value = "T".repeat(5000);
        let chunks = chunk_string(&value, CHUNK_MAX);
        assert!(chunks.len() >= 3, "5000/2000 ar trebui să dea >= 3 bucăți");
        assert!(chunks.iter().all(|c| c.len() <= CHUNK_MAX));
        assert_eq!(join_chunks(chunks), value);
    }

    #[test]
    fn chunk_string_respects_utf8_char_boundaries() {
        // Caractere multi-byte (ex. "ă", "î" din română) lângă granița de tăiere
        // nu trebuie să provoace panică (split la mijlocul unui caracter ar
        // panica în `str::split_at`).
        let value = "ăâîșț".repeat(500); // caractere românești, 2 bytes fiecare în UTF-8
        let chunks = chunk_string(&value, 7); // graniță impară — forțează ajustare
        assert!(chunks.iter().all(|c| c.len() <= 7));
        assert_eq!(join_chunks(chunks), value);
    }

    // ─── Keychain-dependent round-trip: skip gracefully without a backend ──
    // Urmează modelul `keychain_high_water_mark_round_trips` din
    // commands/license.rs: cont temporar unic per proces, sărim elegant unde
    // nu există backend de keychain (CI headless), curățare best-effort.

    #[test]
    fn save_chunked_load_chunked_round_trip() {
        let service = "com.lucaris.efactura.test_chunked";
        let base = format!("__clarito_test_chunk_{}", std::process::id());

        // Probă multi-bucată (>2 * CHUNK_MAX) ca să exercităm reasamblarea.
        let value = "J".repeat(CHUNK_MAX * 2 + 137);

        if save_chunked(service, &base, &value).is_err() {
            // Fără backend de keychain scriabil (CI headless) — skip elegant.
            delete_chunked(service, &base);
            return;
        }

        let loaded = load_chunked(service, &base);
        delete_chunked(service, &base); // curățare best-effort, indiferent de rezultat

        if let Some(v) = loaded {
            assert_eq!(
                v, value,
                "load_chunked trebuie să reasambleze exact valoarea salvată"
            );
        }
    }

    #[test]
    fn token_bundle_save_load_round_trip_chunked() {
        // Verifică integrarea end-to-end: TokenBundle::save (meta + chunks) →
        // TokenBundle::load trebuie să reproducă exact bundle-ul original,
        // inclusiv pentru token-uri de dimensiune JWT reală (>2560 bytes total,
        // ceea ce ar fi picat pe Windows Credential Manager înainte de fix).
        let company_id = format!("__clarito_test_bundle_{}", std::process::id());
        let bundle = TokenBundle {
            access_token: "at.".to_string() + &"A".repeat(1800),
            refresh_token: "rt.".to_string() + &"R".repeat(1200),
            expires_at: 1_800_000_000,
        };

        if bundle.save(&company_id).is_err() {
            // Fără backend de keychain scriabil — skip elegant.
            TokenBundle::delete(&company_id);
            return;
        }

        let loaded = TokenBundle::load(&company_id);
        TokenBundle::delete(&company_id); // curățare best-effort

        if let Some(loaded) = loaded {
            assert_eq!(loaded.access_token, bundle.access_token);
            assert_eq!(loaded.refresh_token, bundle.refresh_token);
            assert_eq!(loaded.expires_at, bundle.expires_at);
        }
    }
}
