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

const CLIENT_ID: &str = "efactura-desktop";
const REDIRECT_URI: &str = "http://localhost:8787/callback";
const AUTH_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/authorize";
const TOKEN_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/token";
const REVOKE_URL: &str = "https://logincert.anaf.ro/anaf-oauth2-server/revoke";
const CALLBACK_PORT: u16 = 8787;

pub struct OAuthResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

// ─── Helpers criptografice ─────────────────────────────────────────────────

/// Generează un șir de N caractere hex din UUID-uri (caractere valide base64url).
fn random_hex(n: usize) -> String {
    let mut s = String::with_capacity(n + 32);
    while s.len() < n {
        // uuid::Uuid::now_v7() furnizează entropie suficientă pentru PKCE
        let u = uuid::Uuid::now_v7().to_string().replace('-', "");
        s.push_str(&u);
    }
    s.truncate(n);
    s
}

/// Encodare base64url (fără padding) — RFC 4648 §5.
fn base64url_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
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
/// localhost:8787 și returnează token-urile.
/// Blochează până la autorizare sau timeout 120s.
pub async fn authorize(_company_id: &str) -> Result<OAuthResult, String> {
    // 1. PKCE
    let code_verifier = random_hex(64);
    let code_challenge = sha256_base64url(&code_verifier);
    let state = random_hex(16);

    // 2. URL autorizare
    let auth_url = format!(
        "{AUTH_URL}?response_type=code&client_id={CLIENT_ID}\
         &redirect_uri={REDIRECT_URI}&scope=\
         &code_challenge={code_challenge}&code_challenge_method=S256\
         &state={state}"
    );

    // 3. Deschide browser (cross-platform)
    open_browser(&auth_url)?;

    // 4. TCP listener pe port 8787 cu timeout 120s
    let listener = TcpListener::bind(format!("127.0.0.1:{CALLBACK_PORT}"))
        .map_err(|e| format!("Nu pot asculta pe portul {CALLBACK_PORT}: {e}"))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;

    // Folosim un thread dedicat pentru accept() cu timeout simulat prin channel
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        // Setăm read_timeout pe socketul acceptat, nu pe listener
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .ok();

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
                    Some(c) => tx.send(Ok(format!("{c}|||{}", recv_state.unwrap_or_default()))).ok(),
                    None => tx.send(Err("Parametrul 'code' lipsește din callback".into())).ok(),
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
    // cu portul 8787 ocupat indefinit.
    let _ = std::net::TcpStream::connect(format!("127.0.0.1:{CALLBACK_PORT}"));

    let payload = result
        .map_err(|_| "Timeout: autorizarea ANAF nu a primit răspuns în 120s.".to_string())?
        .map_err(|e| e)?;

    let mut parts = payload.splitn(2, "|||");
    let code = parts.next().unwrap_or("").to_string();
    let recv_state = parts.next().unwrap_or("");

    if recv_state != state {
        return Err("State CSRF mismatch — posibil atac. Reîncercați.".into());
    }

    // 5. Schimbăm code-ul pe token
    exchange_code_for_token(&code, &code_verifier).await
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

    let params = [
        ("token", access_token),
        ("client_id", CLIENT_ID),
    ];

    // Fire-and-forget: dacă ANAF nu suportă revocarea corect, nu e critical.
    let _ = client
        .post(REVOKE_URL)
        .form(&params)
        .send()
        .await;
}

/// Reîmprospătează access_token-ul folosind refresh_token-ul existent.
pub async fn refresh_token_bundle(
    refresh_tok: &str,
) -> Result<OAuthResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_tok),
        ("client_id", CLIENT_ID),
    ];

    let resp = client
        .post(TOKEN_URL)
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
            return Some(
                val.split_whitespace()
                    .next()
                    .unwrap_or(val)
                    .to_string(),
            );
        }
    }
    None
}

async fn exchange_code_for_token(
    code: &str,
    code_verifier: &str,
) -> Result<OAuthResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("code_verifier", code_verifier),
    ];

    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token exchange request eșuat: {e}"))?;

    parse_token_response(resp).await
}

async fn parse_token_response(
    resp: reqwest::Response,
) -> Result<OAuthResult, String> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Nu pot citi răspunsul token: {e}"))?;

    if !status.is_success() {
        return Err(format!("Token endpoint error {status}: {body}"));
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
