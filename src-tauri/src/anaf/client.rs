//! Client HTTP pentru e-Factura ANAF REST API.
//!
//! Documentație oficială:
//! - Upload:       POST /FCTEL/rest/upload?standard=UBL&cif={cui}
//! - Status:       GET  /FCTEL/rest/stareMesaj?id_incarcare={id}
//! - Lista SPV:    GET  /FCTEL/rest/listaMesajePaginatieFiltrare?zile=60&cif={cui}&tip=F&pagina=1
//! - Descarca:     GET  /FCTEL/rest/descarca/{id}
//!
//! Retry policy:
//! - 5xx: 3 reîncercări cu backoff exponential (2s, 8s, 30s)
//! - 429: Retry-After header (max 60s), max 3 reîncercări
//! - 401: returnează ERR_UNAUTHORIZED — caller-ul face refresh + retry

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Marker de eroare pentru 401 Unauthorized.
/// Caller-ul trebuie să reîmprospăteze token-ul și să reîncerce.
pub const ERR_UNAUTHORIZED: &str = "ANAF_UNAUTHORIZED";

/// Întârzieri backoff (secunde) pentru erori 5xx: maxim 3 reîncercări.
const BACKOFF_5XX: &[u64] = &[2, 8, 30];

// ─── Response types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub index_incarcare: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub stare: String,
    pub descriere: Option<String>,
    pub index_incarcare: Option<String>,
    pub erori: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpvMessage {
    pub id: String,
    pub tip: String,
    pub data_creare: String,
    pub cif: String,
    pub id_solicitare: String,
    pub detalii: Option<String>,
}

// ─── Structuri raw pentru parsare JSON ANAF ────────────────────────────────

#[derive(Deserialize)]
struct UploadRaw {
    #[serde(rename = "dateResponse")]
    date_response: UploadDateResponse,
}

#[derive(Deserialize)]
struct UploadDateResponse {
    index_incarcare: String,
}

#[derive(Deserialize)]
struct MessagesRaw {
    #[serde(rename = "mesaje")]
    mesaje: Option<Vec<SpvMessageRaw>>,
}

#[derive(Deserialize)]
struct SpvMessageRaw {
    id: String,
    tip: String,
    data_creare: String,
    cif: String,
    id_solicitare: String,
    detalii: Option<String>,
}

// ─── Client ────────────────────────────────────────────────────────────────

pub struct AnafClient {
    client: Client,
    base_url: String,
}

impl AnafClient {
    pub fn new(test_mode: bool) -> Self {
        let base = if test_mode {
            "https://api.anaf.ro/test"
        } else {
            "https://api.anaf.ro/prod"
        };
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            base_url: base.to_string(),
        }
    }

    /// Extrage valoarea în secunde din header-ul `Retry-After` (capat la 60s).
    fn parse_retry_after(resp: &reqwest::Response) -> u64 {
        resp.headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5)
            .min(60)
    }

    /// Uploadează un XML UBL la ANAF. Returnează `index_incarcare` (upload ID).
    ///
    /// Retry policy: 5xx → backoff (2s, 8s, 30s); 429 → Retry-After; 401 → ERR_UNAUTHORIZED.
    pub async fn upload_invoice(
        &self,
        token: &str,
        company_cui: &str,
        xml_bytes: Vec<u8>,
    ) -> Result<UploadResponse, String> {
        let url = format!(
            "{}/FCTEL/rest/upload?standard=UBL&cif={}",
            self.base_url, company_cui
        );

        let mut retry_5xx = 0usize;
        let mut retry_429 = 0usize;

        loop {
            let part = reqwest::multipart::Part::bytes(xml_bytes.clone())
                .file_name("factura.xml")
                .mime_str("text/xml")
                .map_err(|e| e.to_string())?;
            let form = reqwest::multipart::Form::new().part("file", part);

            let resp = self
                .client
                .post(&url)
                .bearer_auth(token)
                .multipart(form)
                .send()
                .await
                .map_err(|e| format!("Upload request eșuat: {e}"))?;

            let status = resp.status();

            if status == 401 {
                return Err(ERR_UNAUTHORIZED.to_string());
            }

            if status.as_u16() == 429 {
                if retry_429 < 3 {
                    let delay = Self::parse_retry_after(&resp);
                    tracing::warn!(delay, "ANAF 429 rate-limit — așteptăm");
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    retry_429 += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(body_len = body.len(), "ANAF upload rate-limited (429)");
                return Err(
                    "Limita ANAF depășită (429). Așteptați câteva minute și reîncercați."
                        .to_string(),
                );
            }

            if status.is_server_error() {
                if retry_5xx < BACKOFF_5XX.len() {
                    let delay = BACKOFF_5XX[retry_5xx];
                    tracing::warn!(
                        attempt = retry_5xx + 1,
                        delay,
                        status = status.as_u16(),
                        "ANAF 5xx — reîncercare"
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    retry_5xx += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF upload server error"
                );
                return Err(format!(
                    "Eroare server ANAF ({status}). Serviciul poate fi temporar indisponibil."
                ));
            }

            let body = resp.text().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF upload error"
                );
                return Err(format!(
                    "Eroare comunicare ANAF ({status}). Reîncercați sau contactați suportul."
                ));
            }

            let raw: UploadRaw =
                serde_json::from_str(&body).map_err(|e| format!("Răspuns ANAF invalid: {e}"))?;

            return Ok(UploadResponse {
                index_incarcare: raw.date_response.index_incarcare,
            });
        }
    }

    /// Verifică statusul unui mesaj ANAF după `upload_id`.
    ///
    /// Retry policy: 5xx → backoff; 429 → Retry-After; 401 → ERR_UNAUTHORIZED.
    pub async fn check_status(
        &self,
        token: &str,
        upload_id: &str,
    ) -> Result<StatusResponse, String> {
        let url = format!(
            "{}/FCTEL/rest/stareMesaj?id_incarcare={}",
            self.base_url, upload_id
        );

        let mut retry_5xx = 0usize;
        let mut retry_429 = 0usize;

        loop {
            let resp = self
                .client
                .get(&url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| format!("Status request eșuat: {e}"))?;

            let status = resp.status();

            if status == 401 {
                return Err(ERR_UNAUTHORIZED.to_string());
            }

            if status.as_u16() == 429 {
                if retry_429 < 3 {
                    let delay = Self::parse_retry_after(&resp);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    retry_429 += 1;
                    continue;
                }
                return Err(format!("ANAF check_status rate-limited (429)"));
            }

            if status.is_server_error() {
                if retry_5xx < BACKOFF_5XX.len() {
                    tokio::time::sleep(Duration::from_secs(BACKOFF_5XX[retry_5xx])).await;
                    retry_5xx += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF check_status server error"
                );
                return Err(format!(
                    "Eroare server ANAF ({status}). Serviciul poate fi temporar indisponibil."
                ));
            }

            let body = resp.text().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF status error"
                );
                return Err(format!(
                    "Eroare comunicare ANAF ({status}). Reîncercați sau contactați suportul."
                ));
            }

            let parsed: StatusResponse =
                serde_json::from_str(&body).map_err(|e| format!("JSON status invalid: {e}"))?;

            return Ok(parsed);
        }
    }

    /// Listează mesajele din SPV pentru o companie (ultimele `days` zile).
    ///
    /// Paginare completă: iterează toate paginile până la răspuns gol.
    /// Retry policy per pagină: 5xx → backoff; 429 → Retry-After; 401 → ERR_UNAUTHORIZED.
    pub async fn list_messages(
        &self,
        token: &str,
        company_cui: &str,
        days: u32,
    ) -> Result<Vec<SpvMessage>, String> {
        let mut all_messages: Vec<SpvMessage> = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "{}/FCTEL/rest/listaMesajePaginatieFiltrare?zile={}&cif={}&tip=F&pagina={}",
                self.base_url, days, company_cui, page
            );

            let mut retry_5xx = 0usize;
            let mut retry_429 = 0usize;

            // Inner retry loop for this page
            let page_messages: Vec<SpvMessageRaw> = loop {
                let resp = self
                    .client
                    .get(&url)
                    .bearer_auth(token)
                    .send()
                    .await
                    .map_err(|e| format!("List messages request eșuat (pagina {page}): {e}"))?;

                let status = resp.status();

                if status == 401 {
                    return Err(ERR_UNAUTHORIZED.to_string());
                }

                if status.as_u16() == 429 {
                    if retry_429 < 3 {
                        let delay = Self::parse_retry_after(&resp);
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        retry_429 += 1;
                        continue;
                    }
                    return Err(format!(
                        "ANAF list_messages rate-limited (429) la pagina {page}"
                    ));
                }

                if status.is_server_error() {
                    if retry_5xx < BACKOFF_5XX.len() {
                        tokio::time::sleep(Duration::from_secs(BACKOFF_5XX[retry_5xx])).await;
                        retry_5xx += 1;
                        continue;
                    }
                    let body = resp.text().await.unwrap_or_default();
                    tracing::warn!(
                        status = status.as_u16(),
                        body_len = body.len(),
                        page,
                        "ANAF list_messages server error"
                    );
                    return Err(format!(
                        "Eroare server ANAF ({status}). Serviciul poate fi temporar indisponibil."
                    ));
                }

                let body = resp.text().await.map_err(|e| e.to_string())?;
                if !status.is_success() {
                    tracing::warn!(
                        status = status.as_u16(),
                        body_len = body.len(),
                        page,
                        "ANAF list messages error"
                    );
                    return Err(format!(
                        "Eroare comunicare ANAF ({status}). Reîncercați sau contactați suportul."
                    ));
                }

                let raw: MessagesRaw = serde_json::from_str(&body)
                    .map_err(|e| format!("JSON messages invalid la pagina {page}: {e}"))?;

                break raw.mesaje.unwrap_or_default();
            };

            if page_messages.is_empty() {
                // No more pages
                break;
            }

            all_messages.extend(page_messages.into_iter().map(|m| SpvMessage {
                id: m.id,
                tip: m.tip,
                data_creare: m.data_creare,
                cif: m.cif,
                id_solicitare: m.id_solicitare,
                detalii: m.detalii,
            }));

            page += 1;
        }

        Ok(all_messages)
    }

    /// Descarcă un mesaj SPV după ID. Returnează bytes ZIP.
    ///
    /// Retry policy: 5xx → backoff; 429 → Retry-After; 401 → ERR_UNAUTHORIZED.
    pub async fn download_message(&self, token: &str, message_id: &str) -> Result<Vec<u8>, String> {
        let url = format!("{}/FCTEL/rest/descarca/{}", self.base_url, message_id);

        let mut retry_5xx = 0usize;
        let mut retry_429 = 0usize;

        loop {
            let resp = self
                .client
                .get(&url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| format!("Download request eșuat: {e}"))?;

            let status = resp.status();

            if status == 401 {
                return Err(ERR_UNAUTHORIZED.to_string());
            }

            if status.as_u16() == 429 {
                if retry_429 < 3 {
                    let delay = Self::parse_retry_after(&resp);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    retry_429 += 1;
                    continue;
                }
                return Err("ANAF download rate-limited (429)".to_string());
            }

            if status.is_server_error() {
                if retry_5xx < BACKOFF_5XX.len() {
                    tokio::time::sleep(Duration::from_secs(BACKOFF_5XX[retry_5xx])).await;
                    retry_5xx += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF download server error"
                );
                return Err(format!(
                    "Eroare server ANAF ({status}). Serviciul poate fi temporar indisponibil."
                ));
            }

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = status.as_u16(),
                    body_len = body.len(),
                    "ANAF download error"
                );
                return Err(format!(
                    "Eroare comunicare ANAF ({status}). Reîncercați sau contactați suportul."
                ));
            }

            let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
            return Ok(bytes.to_vec());
        }
    }
}
