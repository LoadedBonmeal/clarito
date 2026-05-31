//! Flux OAuth2 PKCE pentru autentificarea cu ANAF SPV.
//!
//! Pași:
//! 1. Generează code_verifier (UUID-based hex, valid base64url)
//! 2. Calculează code_challenge = BASE64URL(SHA256(verifier))
//! 3. Deschide browser-ul cu URL-ul de autorizare ANAF
//! 4. Ascultă pe localhost:8787 pentru callback
//! 5. Extrage `code` din URL
//! 6. Schimbă code-ul pe token (POST la token endpoint)

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

// ─── Default constants ────────────────────────────────────────────────────────

const DEFAULT_CLIENT_ID: &str = "efactura-desktop";
const DEFAULT_REDIRECT_URI: &str = "http://localhost:8787/callback";
/// URL de autorizare ANAF producție.
const DEFAULT_AUTH_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/authorize";
/// URL token ANAF producție.
const DEFAULT_TOKEN_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/token";
const DEFAULT_REVOKE_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/revoke";
const DEFAULT_CALLBACK_PORT: u16 = 8787;

// ─── OAuthConfig ─────────────────────────────────────────────────────────────

/// Configurație OAuth2 PKCE, citită din setări la runtime.
///
/// Setările suprascriibile (via `api.settings.set`):
/// - `anaf_oauth_client_id`      — client_id înregistrat la ANAF (implicit: "efactura-desktop")
/// - `anaf_oauth_redirect_uri`   — redirect URI (implicit: "http://localhost:8787/callback")
/// - `anaf_oauth_callback_port`  — portul TCP pe care ascultăm (implicit: 8787)
/// - `anaf_oauth_authorize_url`  — URL autorizare (implicit: prod ANAF)
/// - `anaf_oauth_token_url`      — URL token (implicit: prod ANAF)
///
/// Când `use_anaf_test_env = "1"`: dacă nu există override explicit pentru URL-uri,
/// se folosesc tot URL-urile prod ANAF (ANAF nu documentează un host OAuth separat
/// pentru test — sandbox-ul de test se referă la API-ul de facturare, nu la OAuth).
/// Utilizatorii avansați pot suprascrie URL-urile prin chei de setări.
#[derive(Clone, Debug)]
pub struct OAuthConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub callback_port: u16,
    pub authorize_url: String,
    pub token_url: String,
}

impl OAuthConfig {
    /// Construiește configurația folosind valori implicite.
    pub fn default_prod() -> Self {
        OAuthConfig {
            client_id: DEFAULT_CLIENT_ID.to_string(),
            redirect_uri: DEFAULT_REDIRECT_URI.to_string(),
            callback_port: DEFAULT_CALLBACK_PORT,
            authorize_url: DEFAULT_AUTH_URL.to_string(),
            token_url: DEFAULT_TOKEN_URL.to_string(),
        }
    }
}

pub struct OAuthResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

// ─── Helpers criptografice ─────────────────────────────────────────────────

/// Generează un șir de N octeți aleatori criptografic securizat, encodat hex.
/// Folosit pentru code_verifier (PKCE) și state (CSRF). Furnizează 8*N biți
/// de entropie reală — în contrast cu UUID v7 care este timestamp-dominat.
fn random_bytes_hex(n: usize) -> String {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = vec![0u8; n];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Encodare base64url (fără padding) — RFC 4648 §5.
fn base64url_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(TABLE[((n >> 18) & 63) as usize] as char);
        result.push(TABLE[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            result.push(TABLE[((n >> 6) & 63) as usize] as char);
        }
        if i + 2 < bytes.len() {
            result.push(TABLE[(n & 63) as usize] as char);
        }
        i += 3;
    }
    result
}

/// SHA256(input) → base64url (fără padding) — code_challenge PKCE S256.
fn sha256_base64url(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    base64url_encode(&hash)
}

// ─── OAuth flow ────────────────────────────────────────────────────────────

/// Deschide browser-ul pentru autorizare ANAF, captează redirect-ul pe
/// localhost și returnează token-urile.
/// Blochează până la autorizare sau timeout 120s.
///
/// # Erori
/// - Port ocupat → mesaj clar cu portul și recomandare.
/// - Timeout 120s → mesaj RO cu hint certificat digital.
/// - Token exchange eșuat → include descrierea ANAF dacă există.
pub async fn authorize(_company_id: &str, config: &OAuthConfig) -> Result<OAuthResult, String> {
    // 1. PKCE
    let code_verifier = random_bytes_hex(32); // 32 bytes = 64 hex chars = 256 bits entropy
    let code_challenge = sha256_base64url(&code_verifier);
    let state = random_bytes_hex(16); // 16 bytes = 32 hex chars = 128 bits entropy

    // 2. URL autorizare
    let redirect_encoded = encode_uri_component(&config.redirect_uri);
    let auth_url = format!(
        "{}?response_type=code&client_id={}\
         &redirect_uri={}&scope=\
         &code_challenge={}&code_challenge_method=S256\
         &state={}",
        config.authorize_url, config.client_id, redirect_encoded, code_challenge, state
    );

    // 3. Pre-flight: verificăm că portul este disponibil înainte de a deschide browser-ul.
    //    Dacă portul e ocupat, returnăm imediat un mesaj clar în loc să blocăm.
    let port = config.callback_port;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
        format!(
            "Portul {port} este ocupat. Închideți aplicația care îl folosește \
             sau schimbați portul în Setări → ANAF (câmpul Configurare avansată)."
        )
    })?;
    listener.set_nonblocking(false).map_err(|e| e.to_string())?;

    // 4. Deschide browser (cross-platform)
    open_browser(&auth_url)?;

    // Folosim un thread dedicat pentru accept() cu timeout simulat prin channel
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        // Setăm read_timeout pe socketul acceptat, nu pe listener
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    let _ = tx.send(Err("Eroare citire request HTTP".into()));
                    return;
                }

                // "GET /callback?code=...&state=... HTTP/1.1"
                let code = extract_query_param(&request_line, "code");
                let recv_state = extract_query_param(&request_line, "state");

                // Trimite răspuns HTML
                let body = "<!DOCTYPE html><html><body><h2>\
                    Autentificare reușită. Puteți închide această fereastră.\
                    </h2></body></html>";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());

                match code {
                    Some(c) => tx
                        .send(Ok(format!("{c}|||{}", recv_state.unwrap_or_default())))
                        .ok(),
                    None => tx
                        .send(Err("Parametrul 'code' lipsește din callback".into()))
                        .ok(),
                };
            }
            Err(e) => {
                let _ = tx.send(Err(format!("Conexiune eșuată: {e}")));
            }
        }
    });

    // Așteptăm max 120s
    let result = rx.recv_timeout(Duration::from_secs(120));

    // Deblocăm thread-ul de accept() conectându-ne la propriul nostru port, indiferent
    // de rezultat (timeout sau succes). Astfel thread-ul nu rămâne blocat pe accept()
    // cu portul ocupat indefinit.
    let _ = std::net::TcpStream::connect(format!("127.0.0.1:{port}"));

    let payload = result.map_err(|_| {
        "Autorizarea ANAF a expirat (120s). Verificați că aveți certificatul digital \
         instalat în browser (token USB conectat sau soft-cert importat) și reîncercați."
            .to_string()
    })??;

    let mut parts = payload.splitn(2, "|||");
    let code = parts.next().unwrap_or("").to_string();
    let recv_state = parts.next().unwrap_or("");

    if recv_state != state {
        return Err("State CSRF mismatch — posibil atac. Reîncercați.".into());
    }

    // 5. Schimbăm code-ul pe token
    exchange_code_for_token(&code, &code_verifier, config).await
}

/// Revocă token-ul OAuth2 la serverul ANAF (best-effort: erorile sunt ignorate).
/// Trebuie apelată la logout înainte de ștergerea token-ului din keychain.
pub async fn revoke_token(access_token: &str) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let params = [("token", access_token), ("client_id", DEFAULT_CLIENT_ID)];

    // Fire-and-forget: dacă ANAF nu suportă revocarea corect, nu e critical.
    let _ = client.post(DEFAULT_REVOKE_URL).form(&params).send().await;
}

/// Reîmprospătează access_token-ul folosind refresh_token-ul existent.
/// Folosește URL-ul token din configurație (dacă s-a schimbat).
pub async fn refresh_token_bundle(refresh_tok: &str) -> Result<OAuthResult, String> {
    // refresh_token_bundle este apelat fără context de config — folosim default prod.
    // Dacă utilizatorul a configurat un token_url custom, acesta va fi folosit la
    // autorizare inițială; refresh-ul poate folosi prod în continuare (tokens sunt
    // emise de același server).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_tok),
        ("client_id", DEFAULT_CLIENT_ID),
    ];

    let resp = client
        .post(DEFAULT_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token refresh request eșuat: {e}"))?;

    parse_token_response(resp).await
}

// ─── Internals ─────────────────────────────────────────────────────────────

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| format!("Nu pot deschide browser-ul: {e}"))?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()
        .map_err(|e| format!("Nu pot deschide browser-ul: {e}"))?;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|e| format!("Nu pot deschide browser-ul: {e}"))?;

    Ok(())
}

fn extract_query_param(request_line: &str, param: &str) -> Option<String> {
    // request_line: "GET /callback?code=ABC&state=XYZ HTTP/1.1"
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix(&format!("{param}=")) {
            return Some(val.split_whitespace().next().unwrap_or(val).to_string());
        }
    }
    None
}

/// Encodare minimă URI component pentru redirect_uri (spații → %20, etc.).
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')'
            | b':'
            | b'/' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0xf) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

async fn exchange_code_for_token(
    code: &str,
    code_verifier: &str,
    config: &OAuthConfig,
) -> Result<OAuthResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("client_id", config.client_id.as_str()),
        ("code_verifier", code_verifier),
    ];

    let resp = client
        .post(&config.token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token exchange request eșuat: {e}"))?;

    parse_token_response(resp).await
}

async fn parse_token_response(resp: reqwest::Response) -> Result<OAuthResult, String> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Nu pot citi răspunsul token: {e}"))?;

    if !status.is_success() {
        tracing::warn!(%status, "ANAF token endpoint returned non-success");
        // Încercăm să extragem error_description din JSON ANAF
        let description = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|j| {
                j["error_description"]
                    .as_str()
                    .or(j["error"].as_str())
                    .map(|s| s.to_string())
            });
        return Err(if let Some(desc) = description {
            format!(
                "Autentificare ANAF eșuată (HTTP {status}): {desc}. \
                 Verificați că certificatul digital este instalat și activ în browser."
            )
        } else {
            format!(
                "Autentificare ANAF eșuată (HTTP {status}). \
                 Verificați că certificatul digital este instalat și activ în browser."
            )
        });
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Token JSON invalid: {e}"))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or("Câmp 'access_token' lipsește")?
        .to_string();

    let refresh_token = json["refresh_token"]
        .as_str()
        .ok_or("Câmp 'refresh_token' lipsește")?
        .to_string();

    let expires_in = json["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    Ok(OAuthResult {
        access_token,
        refresh_token,
        expires_at,
    })
}
