//! Curs valutar BNR (Banca Națională a României).
//!
//! Expune comanda `fetch_bnr_rate` care întoarce cursul oficial BNR în RON
//! pentru o valută și o dată date.
//!
//! Strategie de fetch:
//! 1. Încearcă feed-ul zilnic  `https://www.bnr.ro/nbrfxrates.xml`.
//! 2. Dacă data cerută nu se găsește (e.g. dată mai veche),
//!    încearcă fișierul anual `https://www.bnr.ro/files/xml/years/nbrfx{YYYY}.xml`.
//!
//! Securitate: host-ul este întotdeauna `www.bnr.ro` (hardcodat).
//! Singurul input al utilizatorului care ajunge în URL este anul (YYYY),
//! extras din prefixul ISO al câmpului `date` și validat ca 4 cifre.

use crate::error::{AppError, AppResult};
use reqwest::Client;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::time::Duration;

// ─── Parser pur (testabil în izolare) ─────────────────────────────────────

/// Parsează XML-ul BNR și returnează cursul (RON / 1 unitate valutară).
///
/// Structura XML BNR (namespace `http://www.bnr.ro/xsd`, ignorat — folosim `local_name`):
/// ```xml
/// <DataSet><Body>
///   <Cube date="2026-05-30">
///     <Rate currency="EUR">4.9771</Rate>
///     <Rate currency="HUF" multiplier="100">1.2700</Rate>
///   </Cube>
/// </Body></DataSet>
/// ```
///
/// Logică: dintre toate `<Cube>` cu `date <= target_date` (comparație ISO string),
/// alege cel cu data cea mai mare care conține `<Rate currency=…>` (case-insensitive);
/// returnează `valoare / multiplier` rotunjit la 4 zecimale.
pub(crate) fn parse_bnr_rate(xml: &str, currency: &str, target_date: &str) -> Option<Decimal> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    // Starea curentă
    let mut in_cube = false;
    let mut cube_date = String::new();
    let mut in_rate = false;
    let mut rate_currency = String::new();
    let mut rate_multiplier: u32 = 1;

    // Cel mai bun rezultat găsit până acum: (date_string, value, multiplier)
    let mut best: Option<(String, Decimal, u32)> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "Cube" => {
                        in_cube = false;
                        cube_date.clear();
                        // Citim atributul `date`
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.local_name().into_inner())
                                .unwrap_or("");
                            if key == "date" {
                                cube_date = std::str::from_utf8(attr.value.as_ref())
                                    .unwrap_or("")
                                    .to_string();
                                // Acceptăm acest Cube dacă date <= target_date (comparație ISO)
                                if !cube_date.is_empty() && cube_date.as_str() <= target_date {
                                    in_cube = true;
                                }
                                break;
                            }
                        }
                    }
                    "Rate" if in_cube => {
                        in_rate = false;
                        rate_currency.clear();
                        rate_multiplier = 1;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.local_name().into_inner())
                                .unwrap_or("");
                            match key {
                                "currency" => {
                                    rate_currency = std::str::from_utf8(attr.value.as_ref())
                                        .unwrap_or("")
                                        .to_string();
                                }
                                "multiplier" => {
                                    rate_multiplier = std::str::from_utf8(attr.value.as_ref())
                                        .ok()
                                        .and_then(|s| s.parse::<u32>().ok())
                                        .unwrap_or(1);
                                }
                                _ => {}
                            }
                        }
                        if rate_currency.eq_ignore_ascii_case(currency) {
                            in_rate = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // <Rate .../> self-closing (rar, dar posibil)
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                if local == "Rate" {
                    in_rate = false;
                }
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "Cube" => {
                        in_cube = false;
                        cube_date.clear();
                    }
                    "Rate" => {
                        in_rate = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_rate => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => {
                        in_rate = false;
                        continue;
                    }
                };
                if text.is_empty() {
                    continue;
                }
                if let Ok(val) = Decimal::from_str(&text) {
                    // Actualizăm `best` dacă data acestui Cube e mai mare
                    let is_better = best
                        .as_ref()
                        .map(|(d, _, _)| cube_date.as_str() > d.as_str())
                        .unwrap_or(true);
                    if is_better {
                        best = Some((cube_date.clone(), val, rate_multiplier));
                    }
                }
                in_rate = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    best.map(|(_, val, mult)| {
        let divisor = Decimal::from(mult.max(1));
        (val / divisor)
            .round_dp_with_strategy(4, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    })
}

// ─── Helper: construiești clientul reqwest cu rustls ──────────────────────

fn build_client() -> Result<Client, AppError> {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Other(format!("Eroare creare client HTTP BNR: {e}")))
}

// ─── Tauri command ─────────────────────────────────────────────────────────

/// Returnează cursul oficial BNR (RON / 1 unitate valutară) pentru `currency`
/// la data `date` (format ISO `YYYY-MM-DD`).
///
/// - RON → returnează `1.0` imediat.
/// - Încearcă mai întâi feed-ul zilnic; dacă nu găsește rata pentru data cerută,
///   încearcă fișierul anual al anului respectiv.
/// - Numai host-ul `www.bnr.ro` este folosit (hardcodat). Singurul input care
///   ajunge în URL este anul (4 cifre), validat explicit.
#[tauri::command]
pub async fn fetch_bnr_rate(currency: String, date: String) -> AppResult<f64> {
    // RON e deja în RON — curs 1:1
    if currency.eq_ignore_ascii_case("RON") {
        return Ok(1.0);
    }

    // Validăm că `date` are prefixul YYYY valid (4 cifre) înainte de orice
    // interpolare în URL, ca să prevenim injecții de tip path traversal.
    let year_str = date.get(..4).ok_or_else(|| {
        AppError::Validation(format!("Data '{date}' nu este în format YYYY-MM-DD"))
    })?;
    if !year_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(format!(
            "Anul '{year_str}' din data '{date}' nu este valid"
        )));
    }

    let client = build_client()?;

    // ── Pasul 1: feed zilnic ──────────────────────────────────────────────
    let daily_url = "https://www.bnr.ro/nbrfxrates.xml";
    let daily_xml = fetch_xml(&client, daily_url).await?;
    if let Some(rate) = parse_bnr_rate(&daily_xml, &currency, &date) {
        return rate.to_f64().ok_or_else(|| {
            AppError::Other(format!(
                "Nu se poate converti cursul BNR la f64 pentru {currency}"
            ))
        });
    }

    // ── Pasul 2: fișier anual ─────────────────────────────────────────────
    // Numai anul (validat ca 4 cifre mai sus) intră în URL — fără alt input de user.
    let year_url = format!("https://www.bnr.ro/files/xml/years/nbrfx{year_str}.xml");
    let year_xml = fetch_xml(&client, &year_url).await?;
    if let Some(rate) = parse_bnr_rate(&year_xml, &currency, &date) {
        return rate.to_f64().ok_or_else(|| {
            AppError::Other(format!(
                "Nu se poate converti cursul BNR la f64 pentru {currency}"
            ))
        });
    }

    Err(AppError::Validation(format!(
        "Cursul BNR pentru {currency} la {date} nu a fost găsit"
    )))
}

/// Descarcă XML-ul de la `url` și returnează textul.
async fn fetch_xml(client: &Client, url: &str) -> AppResult<String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("Eroare rețea BNR ({url}): {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "BNR a returnat HTTP {} pentru {url}",
            resp.status()
        )));
    }

    resp.text()
        .await
        .map_err(|e| AppError::Other(format!("Eroare citire răspuns BNR: {e}")))
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // XML zilnic minimal (un singur Cube, ca în nbrfxrates.xml)
    const DAILY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Header>
    <PublishingDate>2026-05-30</PublishingDate>
  </Header>
  <Body>
    <Cube date="2026-05-30">
      <Rate currency="EUR">4.9771</Rate>
      <Rate currency="USD">4.5200</Rate>
      <Rate currency="HUF" multiplier="100">1.2700</Rate>
      <Rate currency="GBP">5.8100</Rate>
    </Cube>
  </Body>
</DataSet>"#;

    // XML anual minimal (mai multe Cube-uri, ca în nbrfx2026.xml)
    const YEARLY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataSet xmlns="http://www.bnr.ro/xsd">
  <Body>
    <Cube date="2026-01-02">
      <Rate currency="EUR">4.9600</Rate>
      <Rate currency="USD">4.4900</Rate>
    </Cube>
    <Cube date="2026-03-15">
      <Rate currency="EUR">4.9700</Rate>
      <Rate currency="USD">4.5000</Rate>
    </Cube>
    <Cube date="2026-05-28">
      <Rate currency="EUR">4.9750</Rate>
      <Rate currency="USD">4.5150</Rate>
      <Rate currency="HUF" multiplier="100">1.2680</Rate>
    </Cube>
    <Cube date="2026-05-30">
      <Rate currency="EUR">4.9771</Rate>
      <Rate currency="USD">4.5200</Rate>
      <Rate currency="HUF" multiplier="100">1.2700</Rate>
    </Cube>
    <Cube date="2026-06-01">
      <Rate currency="EUR">4.9800</Rate>
    </Cube>
  </Body>
</DataSet>"#;

    /// Feed zilnic: EUR găsit corect.
    #[test]
    fn daily_eur_rate_is_correct() {
        let rate = parse_bnr_rate(DAILY_XML, "EUR", "2026-05-30");
        assert_eq!(rate, Some(dec!(4.9771)));
    }

    /// Feed zilnic: USD găsit corect.
    #[test]
    fn daily_usd_rate_is_correct() {
        let rate = parse_bnr_rate(DAILY_XML, "USD", "2026-05-30");
        assert_eq!(rate, Some(dec!(4.5200)));
    }

    /// Feed zilnic cu multiplier=100: HUF → valoare / 100, rotunjit la 4 zecimale.
    #[test]
    fn daily_huf_with_multiplier_divides_correctly() {
        let rate = parse_bnr_rate(DAILY_XML, "HUF", "2026-05-30");
        // 1.2700 / 100 = 0.0127
        assert_eq!(rate, Some(dec!(0.0127)));
    }

    /// Valuta absentă din XML → None.
    #[test]
    fn missing_currency_returns_none() {
        let rate = parse_bnr_rate(DAILY_XML, "JPY", "2026-05-30");
        assert_eq!(rate, None);
    }

    /// Comparare case-insensitive: "eur" funcționează ca "EUR".
    #[test]
    fn currency_match_is_case_insensitive() {
        let rate_lower = parse_bnr_rate(DAILY_XML, "eur", "2026-05-30");
        let rate_upper = parse_bnr_rate(DAILY_XML, "EUR", "2026-05-30");
        assert_eq!(rate_lower, rate_upper);
        assert!(rate_lower.is_some());
    }

    /// XML anual (multi-Cube): alege cel mai recent Cube cu date <= target_date.
    /// Cu target_date = "2026-05-29", cel mai recent Cube eligibil e "2026-05-28".
    #[test]
    fn yearly_xml_picks_most_recent_cube_before_target() {
        let rate = parse_bnr_rate(YEARLY_XML, "EUR", "2026-05-29");
        // Cube "2026-05-28" e cel mai recent <= "2026-05-29"
        assert_eq!(rate, Some(dec!(4.9750)));
    }

    /// XML anual: cu target_date exact pe un Cube, alege acel Cube.
    #[test]
    fn yearly_xml_picks_exact_date_cube() {
        let rate = parse_bnr_rate(YEARLY_XML, "EUR", "2026-05-30");
        assert_eq!(rate, Some(dec!(4.9771)));
    }

    /// XML anual: Cube-urile cu date > target_date sunt ignorate.
    /// Cu target = "2026-05-30", Cube-ul "2026-06-01" e ignorat.
    #[test]
    fn yearly_xml_ignores_future_cubes() {
        // "2026-06-01" are EUR = 4.9800, dar e după target "2026-05-30"
        // deci trebuie să găsim 4.9771 (din "2026-05-30")
        let rate = parse_bnr_rate(YEARLY_XML, "EUR", "2026-05-30");
        assert_eq!(rate, Some(dec!(4.9771)));
        assert_ne!(rate, Some(dec!(4.9800)));
    }

    /// XML anual: HUF cu multiplier=100 în fișierul anual.
    #[test]
    fn yearly_huf_with_multiplier_divides_correctly() {
        let rate = parse_bnr_rate(YEARLY_XML, "HUF", "2026-05-30");
        // 1.2700 / 100 = 0.0127
        assert_eq!(rate, Some(dec!(0.0127)));
    }

    /// XML anual: valuta prezentă doar în unele Cube-uri.
    /// HUF apare numai din "2026-05-28" încolo; dacă target < "2026-05-28" → None.
    #[test]
    fn yearly_currency_absent_from_all_eligible_cubes_returns_none() {
        // HUF apare abia de la 2026-05-28; pentru 2026-03-20 nu există
        let rate = parse_bnr_rate(YEARLY_XML, "HUF", "2026-03-20");
        assert_eq!(rate, None);
    }

    /// XML gol → None.
    #[test]
    fn empty_xml_returns_none() {
        let rate = parse_bnr_rate("", "EUR", "2026-05-30");
        assert_eq!(rate, None);
    }

    /// XML invalid → None (nu panic).
    #[test]
    fn malformed_xml_returns_none_without_panic() {
        let rate = parse_bnr_rate("<<<not xml>>>", "EUR", "2026-05-30");
        assert_eq!(rate, None);
    }

    // Nota: apelul de rețea real (fetch_bnr_rate cu host www.bnr.ro)
    // necesită conectivitate internet și NU este testat în unit tests.
    // Poate fi testat manual cu: `curl https://www.bnr.ro/nbrfxrates.xml`
}
