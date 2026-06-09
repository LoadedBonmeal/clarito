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

/// e-Transport upload response (UploadV2Response): the upload index + the issued Cod UIT.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct EtransportUploadResponse {
    pub index_incarcare: String,
    #[serde(rename = "UIT", default)]
    pub uit: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpvMessage {
    pub id: String,
    pub tip: String,
    pub data_creare: String,
    pub cif: String,
    /// Null in the live SPVWS2 inbox for unsolicited messages (recipise/notificări/somații).
    #[serde(default)]
    pub id_solicitare: Option<String>,
    pub detalii: Option<String>,
}

/// Categorise a general-SPV message `tip` into an inbox bucket. The SPVWS2 inbox carries
/// declaration recipise, notificări, somații, decizii etc. (distinct from the e-Factura FCTEL
/// message list). Pure — drives the inbox grouping + the "actionable" highlight (somații).
pub fn classify_spv_tip(tip: &str) -> &'static str {
    let t = tip.to_uppercase();
    if t.contains("SOMA") {
        "somatie"
    } else if t.contains("RECIP") {
        "recipisa"
    } else if t.contains("DECIZ") {
        "decizie"
    } else if t.contains("NOTIF") {
        "notificare"
    } else if t.contains("FACTUR") {
        "factura"
    } else {
        "altele"
    }
}

/// Parse the e-Transport upload response (ANAF UploadV2). Surfaces a logical rejection
/// (ExecutionStatus != 0 or an errors[] array) as a human message instead of a generic
/// parse error; tolerates `index_incarcare` as a JSON number OR string; `UIT` may be absent
/// (it is issued asynchronously — fetched via a later status query). Pure + testable.
fn parse_etransport_upload(body: &str) -> Result<EtransportUploadResponse, String> {
    let preview = || body.trim().chars().take(300).collect::<String>();
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| format!("Răspuns e-Transport neașteptat de la ANAF: {}", preview()))?;

    let errors: Vec<String> = v
        .get("Errors")
        .or_else(|| v.get("errors"))
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    e.get("errorMessage")
                        .or_else(|| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    if !errors.is_empty() {
        return Err(format!(
            "ANAF a respins declarația e-Transport: {}",
            errors.join("; ")
        ));
    }
    if let Some(st) = v.get("ExecutionStatus").and_then(|x| x.as_i64()) {
        if st != 0 {
            return Err(format!(
                "ANAF a respins declarația e-Transport (ExecutionStatus={st})."
            ));
        }
    }

    let index = v
        .get("index_incarcare")
        .map(|x| {
            x.as_str()
                .map(|s| s.to_string())
                .or_else(|| x.as_i64().map(|n| n.to_string()))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if index.is_empty() {
        return Err(format!(
            "Răspuns e-Transport fără index de încărcare: {}",
            preview()
        ));
    }
    let uit = v.get("UIT").and_then(|x| x.as_str()).map(|s| s.to_string());
    Ok(EtransportUploadResponse {
        index_incarcare: index,
        uit,
    })
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
    #[serde(default)]
    id_solicitare: Option<String>,
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
        // Build URL with query params via reqwest so values are percent-encoded.
        let url = format!("{}/FCTEL/rest/upload", self.base_url);

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
                .query(&[("standard", "UBL"), ("cif", company_cui)])
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
        // Base URL; query params added via .query() for proper percent-encoding.
        let url = format!("{}/FCTEL/rest/stareMesaj", self.base_url);

        let mut retry_5xx = 0usize;
        let mut retry_429 = 0usize;

        loop {
            let resp = self
                .client
                .get(&url)
                .query(&[("id_incarcare", upload_id)])
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
                return Err("ANAF check_status rate-limited (429)".to_string());
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
    ///
    /// A2 — Pagination cap: breaks after MAX_PAGES pages and logs a warning to
    /// prevent an infinite loop when a buggy server returns a constant non-empty page.
    pub async fn list_messages(
        &self,
        token: &str,
        company_cui: &str,
        days: u32,
    ) -> Result<Vec<SpvMessage>, String> {
        /// A2: upper bound on pagination to guard against a misbehaving server
        /// that never returns an empty page.
        const MAX_PAGES: u32 = 1000;

        let mut all_messages: Vec<SpvMessage> = Vec::new();
        let mut page = 1u32;

        loop {
            // A2: break if we have exceeded the safe pagination cap.
            if page > MAX_PAGES {
                tracing::warn!(
                    page,
                    company_cui,
                    "list_messages: exceeded MAX_PAGES ({MAX_PAGES}) — \
                     terminating pagination loop to prevent infinite loop"
                );
                break;
            }
            // tip=F → facturi primite (received invoices).
            // NOTĂ: tip=E → erori/mesaje de status pentru trimiteri ANAF. Acestea sunt
            // urmărite prin polling stareMesaj (check_status), deci adăugarea tip=E ar
            // crea notificări duplicate și ar risca procesarea incorectă a mesajelor de
            // tip E ca facturi în parser-ul receive (care presupune structură UBL).
            // Menținerea tip=F ca filtru este intenționată — R13 Wave E.
            // Query params are added via .query() for proper percent-encoding of
            // company_cui and other dynamic values.
            let url = format!("{}/FCTEL/rest/listaMesajePaginatieFiltrare", self.base_url);
            let days_str = days.to_string();
            let page_str = page.to_string();

            let mut retry_5xx = 0usize;
            let mut retry_429 = 0usize;

            // Inner retry loop for this page
            let page_messages: Vec<SpvMessageRaw> = loop {
                let resp = self
                    .client
                    .get(&url)
                    .query(&[
                        ("zile", days_str.as_str()),
                        ("cif", company_cui),
                        ("tip", "F"),
                        ("pagina", page_str.as_str()),
                    ])
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

    /// List the GENERAL SPV inbox via SPVWS2 — declaration recipise, notificări, somații, decizii
    /// (distinct from the e-Factura FCTEL message list above). Read-only; reuses the OAuth bearer
    /// token. NOTE: SPVWS2 lives on a different host (`webserviced.anaf.ro`) and there is no public
    /// test instance, so this is exercised only against live ANAF — not reachable in unit tests.
    /// The response shares the e-Factura `{mesaje:[…]}` shape, so `MessagesRaw` is reused.
    pub async fn list_spv_messages(
        &self,
        token: &str,
        company_cui: &str,
        days: u32,
    ) -> Result<Vec<SpvMessage>, String> {
        let base = "https://webserviced.anaf.ro/SPVWS2/rest";
        let url = format!("{base}/listaMesaje");
        let days_str = days.to_string();
        let resp = self
            .client
            .get(&url)
            .query(&[("zile", days_str.as_str()), ("cif", company_cui)])
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("List SPV messages request eșuat: {e}"))?;
        let status = resp.status();
        if status == 401 {
            return Err(ERR_UNAUTHORIZED.to_string());
        }
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            tracing::warn!(
                status = status.as_u16(),
                body_len = body.len(),
                "ANAF SPVWS2 list messages error"
            );
            return Err(format!(
                "Eroare comunicare ANAF SPV ({status}). Reîncercați sau contactați suportul."
            ));
        }
        let raw: MessagesRaw =
            serde_json::from_str(&body).map_err(|e| format!("JSON SPV messages invalid: {e}"))?;
        Ok(raw
            .mesaje
            .unwrap_or_default()
            .into_iter()
            .map(|m| SpvMessage {
                id: m.id,
                tip: m.tip,
                data_creare: m.data_creare,
                cif: m.cif,
                id_solicitare: m.id_solicitare,
                detalii: m.detalii,
            })
            .collect())
    }

    /// Submit an e-Transport declaration (schema v2). Unlike D300/D394, e-Transport HAS an OAuth
    /// REST API: POST {base}/ETRANSPORT/ws/v1/upload/ETRANSP/{cif}/2, same Bearer token as
    /// e-Factura. Returns the upload index + the issued Cod UIT. Live-only (needs ANAF auth).
    pub async fn upload_etransport(
        &self,
        token: &str,
        company_cui: &str,
        xml_bytes: Vec<u8>,
    ) -> Result<EtransportUploadResponse, String> {
        // Strip an "RO"/"ro" prefix once (case-insensitive) so the URL CIF matches the XML
        // codDeclarant (which uses the same RO-strip) — digits-only path segment.
        let trimmed = company_cui.trim();
        let cui = trimmed
            .strip_prefix("RO")
            .or_else(|| trimmed.strip_prefix("ro"))
            .unwrap_or(trimmed)
            .trim();
        let url = format!(
            "{}/ETRANSPORT/ws/v1/upload/ETRANSP/{}/2",
            self.base_url, cui
        );
        let resp = self
            .client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/xml")
            .bearer_auth(token)
            .body(xml_bytes)
            .send()
            .await
            .map_err(|e| format!("Upload e-Transport request eșuat: {e}"))?;
        let status = resp.status();
        if status == 401 {
            return Err(ERR_UNAUTHORIZED.to_string());
        }
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            tracing::warn!(
                status = status.as_u16(),
                body_len = body.len(),
                "ANAF e-Transport upload error"
            );
            return Err(format!(
                "Eroare comunicare ANAF e-Transport ({status}). Reîncercați."
            ));
        }
        parse_etransport_upload(&body)
    }

    /// Fetch the RO e-TVA "decont precompletat" (P300ETVA) for a period. Dedicated ANAF service
    /// (NOT the general SPV /cerere): GET {base}/decont/ws/v1/info?cui&an&luna (OAuth2 bearer).
    /// Returns the raw ZIP bytes, which contain two JSON files (the precompletat decont + details).
    /// Live-only (needs ANAF auth + a real period). cui/an/luna are numeric.
    pub async fn fetch_etva_decont(
        &self,
        token: &str,
        company_cui: &str,
        an: i32,
        luna: u32,
    ) -> Result<Vec<u8>, String> {
        let trimmed = company_cui.trim();
        let cui = trimmed
            .strip_prefix("RO")
            .or_else(|| trimmed.strip_prefix("ro"))
            .unwrap_or(trimmed)
            .trim();
        let url = format!("{}/decont/ws/v1/info", self.base_url);
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("cui", cui.to_string()),
                ("an", an.to_string()),
                ("luna", luna.to_string()),
            ])
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Cerere e-TVA decont eșuată: {e}"))?;
        let status = resp.status();
        if status == 401 {
            return Err(ERR_UNAUTHORIZED.to_string());
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "Eroare comunicare ANAF e-TVA ({status}). Verificați perioada/permisiunile."
            ));
        }
        Ok(bytes.to_vec())
    }

    /// Descarcă un mesaj SPV după ID. Returnează bytes ZIP.
    ///
    /// Retry policy: 5xx → backoff; 429 → Retry-After; 401 → ERR_UNAUTHORIZED.
    pub async fn download_message(&self, token: &str, message_id: &str) -> Result<Vec<u8>, String> {
        // Percent-encode the message_id path segment to prevent path traversal
        // (e.g. a message_id of "../other" must not resolve to a different endpoint).
        let encoded_id: String = message_id
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    vec![b as char]
                }
                _ => format!("%{b:02X}").chars().collect(),
            })
            .collect();
        let url = format!("{}/FCTEL/rest/descarca/{}", self.base_url, encoded_id);

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

#[cfg(test)]
mod tests {
    use super::{classify_spv_tip, parse_etransport_upload, MessagesRaw};

    #[test]
    fn etransport_upload_parse_matches_documented_uploadv2_contract() {
        // Golden fixture: the documented ANAF e-Transport UploadV2 JSON reply (per the
        // printesoi/e-factura-go SDK + the MF Swagger spec). index_incarcare is an int64,
        // ExecutionStatus int32 (0 = accepted), plus UIT/dateResponse/trace_id/ref_declarant and
        // an optional `atentie` (non-fatal warning). We must extract the index + UIT and tolerate
        // the extra fields. Locks the contract this deep-research verified.
        let accepted = r#"{
            "dateResponse": "202606091000",
            "ExecutionStatus": 0,
            "index_incarcare": 5012345678,
            "UIT": "3R0ABCDEF123456",
            "trace_id": "9f1c-aa",
            "ref_declarant": "REF-1",
            "atentie": "Declaratie acceptata cu observatii."
        }"#;
        let r = parse_etransport_upload(accepted).unwrap();
        assert_eq!(
            r.index_incarcare, "5012345678",
            "int64 index → string, no parse error"
        );
        assert_eq!(r.uit.as_deref(), Some("3R0ABCDEF123456"));

        // Documented rejection: ExecutionStatus != 0 with an Errors[] list → human message.
        let rejected = r#"{
            "dateResponse": "202606091000",
            "ExecutionStatus": 1,
            "trace_id": "9f1c-bb",
            "Errors": [{"errorMessage": "Greutate bruta invalida pe linia 1"}]
        }"#;
        let err = parse_etransport_upload(rejected).unwrap_err();
        assert!(
            err.contains("Greutate bruta invalida"),
            "surfaces ANAF errorMessage: {err}"
        );
    }

    #[test]
    fn etransport_upload_parse_handles_number_string_errors_and_garbage() {
        // index as a JSON NUMBER (the real shape) — must not fail on the String-typed field.
        let ok_num = parse_etransport_upload(
            r#"{"dateResponse":"x","ExecutionStatus":0,"index_incarcare":5012345678,"UIT":"3R0ABC"}"#,
        )
        .unwrap();
        assert_eq!(ok_num.index_incarcare, "5012345678");
        assert_eq!(ok_num.uit.as_deref(), Some("3R0ABC"));
        // index as a string + no UIT yet (issued async).
        let ok_str =
            parse_etransport_upload(r#"{"ExecutionStatus":0,"index_incarcare":"42"}"#).unwrap();
        assert_eq!(ok_str.index_incarcare, "42");
        assert!(ok_str.uit.is_none());
        // logical rejection: errors[] surfaced as a human message.
        let rej = parse_etransport_upload(
            r#"{"ExecutionStatus":1,"errors":[{"errorMessage":"codTipOperatiune invalid"}]}"#,
        )
        .unwrap_err();
        assert!(rej.contains("codTipOperatiune invalid"));
        // ExecutionStatus != 0 without errors[].
        assert!(parse_etransport_upload(r#"{"ExecutionStatus":2,"index_incarcare":"1"}"#).is_err());
        // non-JSON (e.g. an XML/HTML body) → graceful error, not a panic.
        assert!(parse_etransport_upload("<html>503</html>").is_err());
    }

    #[test]
    fn classifies_spv_message_types() {
        assert_eq!(classify_spv_tip("SOMATIE"), "somatie");
        assert_eq!(classify_spv_tip("Recipisa declaratie D300"), "recipisa");
        assert_eq!(classify_spv_tip("DECIZIE de impunere"), "decizie");
        assert_eq!(classify_spv_tip("Notificare conformare"), "notificare");
        assert_eq!(classify_spv_tip("FACTURA PRIMITA"), "factura");
        assert_eq!(classify_spv_tip("altceva"), "altele");
    }

    #[test]
    fn parses_spv_message_with_null_id_solicitare() {
        // Real SPVWS2 inbox returns id_solicitare:null for unsolicited messages (recipise etc.).
        let json = r#"{"mesaje":[{"id":"100","tip":"RECIPISA","data_creare":"202606090900","cif":"123","id_solicitare":null}]}"#;
        let raw: MessagesRaw = serde_json::from_str(json).expect("must parse null id_solicitare");
        let msgs = raw.mesaje.unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].id_solicitare.is_none());
        // and an entirely missing id_solicitare must also parse (serde default).
        let json2 = r#"{"mesaje":[{"id":"101","tip":"MESAJ","data_creare":"x","cif":"123"}]}"#;
        assert!(serde_json::from_str::<MessagesRaw>(json2).is_ok());
    }
}
