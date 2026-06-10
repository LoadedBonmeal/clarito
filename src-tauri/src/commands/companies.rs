use tauri::State;

use crate::db::companies::{
    self, Company, CreateCompanyInput, TaxRegimeStatus, UpdateCompanyInput,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_companies(state: State<'_, AppState>) -> AppResult<Vec<Company>> {
    companies::list(&state.db).await
}

#[tauri::command]
pub async fn get_company(state: State<'_, AppState>, id: String) -> AppResult<Company> {
    companies::get(&state.db, &id).await
}

/// Micro-ceiling status for a company: its year-to-date sales turnover (RON) vs the 100.000 EUR
/// micro ceiling (`eur_ron` = the EUR→RON rate the FE supplies, e.g. the year-end BNR rate), with
/// an advisory when approaching/exceeding it. Turnover proxy = validated, non-storno sales net.
#[tauri::command]
pub async fn tax_regime_status(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    eur_ron: f64,
) -> AppResult<TaxRegimeStatus> {
    use rust_decimal::Decimal;
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let turnover_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(subtotal_amount AS REAL) * COALESCE(exchange_rate, 1.0)), 0.0) \
         FROM invoices \
         WHERE company_id = ?1 AND status = 'VALIDATED' AND storno_of_invoice_id IS NULL \
           AND issue_date >= ?2 AND issue_date <= ?3",
    )
    .bind(&company_id)
    .bind(&from)
    .bind(&to)
    .fetch_one(pool)
    .await?;
    let turnover = Decimal::try_from(turnover_f.max(0.0))
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let eur = Decimal::try_from(eur_ron.max(0.0)).unwrap_or(Decimal::ZERO);
    Ok(companies::micro_ceiling_status(
        &company.tax_regime,
        company.cash_vat,
        turnover,
        eur,
    ))
}

/// Starea plafonului de scutire TVA (art. 310 Cod fiscal, Legea 141/2025): 395.000 lei cifră de
/// afaceri anuală de la 01.09.2025. Relevant DOAR pentru neplătitorii de TVA — depășirea obligă la
/// înregistrarea în scopuri de TVA. `level`: none / approaching (≥80%) / exceeded.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatRegistrationStatus {
    pub applicable: bool,
    pub level: String,
    pub ytd_turnover_ron: String,
    pub plafon_ron: String,
    pub pct: i64,
}

#[tauri::command]
pub async fn vat_registration_status(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
) -> AppResult<VatRegistrationStatus> {
    use rust_decimal::Decimal;
    const PLAFON_LEI: i64 = 395_000;
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    if company.vat_payer {
        return Ok(VatRegistrationStatus {
            applicable: false,
            level: "none".into(),
            ytd_turnover_ron: "0".into(),
            plafon_ron: PLAFON_LEI.to_string(),
            pct: 0,
        });
    }
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let turnover_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(subtotal_amount AS REAL) * COALESCE(exchange_rate, 1.0)), 0.0) \
         FROM invoices \
         WHERE company_id = ?1 AND status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?2 AND issue_date <= ?3",
    )
    .bind(&company_id)
    .bind(&from)
    .bind(&to)
    .fetch_one(pool)
    .await?;
    let turnover = crate::db::invoices::round2(
        Decimal::try_from(turnover_f.max(0.0)).unwrap_or(Decimal::ZERO),
    );
    let plafon = Decimal::from(PLAFON_LEI);
    let pct: i64 = ((turnover / plafon) * Decimal::from(100))
        .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .try_into()
        .unwrap_or(0);
    let level = if turnover >= plafon {
        "exceeded"
    } else if pct >= 80 {
        "approaching"
    } else {
        "none"
    };
    Ok(VatRegistrationStatus {
        applicable: true,
        level: level.into(),
        ytd_turnover_ron: turnover.to_string(),
        plafon_ron: PLAFON_LEI.to_string(),
        pct,
    })
}

#[tauri::command]
pub async fn create_company(
    state: State<'_, AppState>,
    input: CreateCompanyInput,
) -> AppResult<Company> {
    // Verificăm limita de companii conform planului de licență:
    // TRIAL → 3, SOLO → 1, ACCOUNTANT → 15, FIRM → nelimitat
    if let Some(lic) = crate::db::license::get(&state.db).await? {
        let current_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM companies WHERE is_active = 1")
                .fetch_one(&state.db)
                .await
                .unwrap_or(0);
        let limit: Option<i64> = match lic.tier.as_str() {
            "TRIAL" => Some(3),
            "SOLO" => Some(1),
            "ACCOUNTANT" => Some(15),
            "FIRM" => None, // nelimitat
            _ => Some(3),   // tier necunoscut → fallback la TRIAL
        };
        if let Some(max) = limit {
            if current_count >= max {
                return Err(AppError::Validation(format!(
                    "Planul {} permite maxim {} companie(i). \
                     Actualizați licența pentru a adăuga mai multe.",
                    lic.tier, max
                )));
            }
        }
    }
    let new_company = companies::create(&state.db, input).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "company_created",
        "company",
        &new_company.id,
        Some(&new_company.cui),
    )
    .await;
    Ok(new_company)
}

#[tauri::command]
pub async fn update_company(
    state: State<'_, AppState>,
    id: String,
    input: UpdateCompanyInput,
) -> AppResult<Company> {
    let updated = companies::update(&state.db, &id, input).await?;
    let _ =
        crate::db::audit::log_user_action(&state.db, "company_updated", "company", &id, None).await;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_company(state: State<'_, AppState>, id: String) -> AppResult<()> {
    companies::soft_delete(&state.db, &id).await
}

#[tauri::command]
pub async fn get_next_invoice_number(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<i64> {
    companies::next_invoice_number(&state.db, &company_id).await
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnafCompanyData {
    pub cui: String,
    pub legal_name: String,
    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub registry_number: Option<String>,
    pub phone: Option<String>,
    pub vat_payer: bool,
    /// TVA la încasare (cash VAT) — `inregistrare_RTVAI.statusTvaIncasare`. The buyer's VAT
    /// deduction is deferred to payment (art. 297 Cod fiscal) when the supplier is on cash VAT.
    pub cash_vat: bool,
    /// Registered in "Registrul RO e-Factura" — `date_generale.statusRO_e_Factura`.
    pub efactura_registered: bool,
    /// False when the company is an inactive contributor (`stare_inactiv.statusInactivi`) — its
    /// invoices carry restricted deductibility for the buyer (art. 11 Cod fiscal).
    pub active: bool,
}

#[tauri::command]
pub async fn fetch_anaf_company_data(cui: String) -> AppResult<AnafCompanyData> {
    // Clean CUI: strip "RO" prefix (case-insensitive), strip spaces, parse as u64
    let cleaned = cui.trim().to_uppercase();
    let cleaned = cleaned.strip_prefix("RO").unwrap_or(&cleaned);
    let cleaned = cleaned.replace(' ', "");
    let cui_number: u64 = cleaned
        .parse()
        .map_err(|_| AppError::Validation(format!("CUI invalid: {cui}")))?;

    // Today's date as "YYYY-MM-DD"
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // POST to ANAF API
    let body = serde_json::json!([{"cui": cui_number, "data": today}]);
    let client = reqwest::Client::new();
    let response = client
        .post("https://webservicesp.anaf.ro/api/PlatitorTvaRest/v9/tva")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Check if not found
    let not_found = json.get("notFound").and_then(|v| v.as_array());
    let found = json.get("found").and_then(|v| v.as_array());

    let found_list = match found {
        Some(list) if !list.is_empty() => list,
        _ => {
            return Err(AppError::NotFound);
        }
    };

    // Also check notFound array contains our cui
    if let Some(nf) = not_found {
        if nf.iter().any(|v| {
            v.as_u64() == Some(cui_number)
                || v.as_str().map(|s| s == cleaned.as_str()).unwrap_or(false)
        }) {
            return Err(AppError::NotFound);
        }
    }

    let entry = &found_list[0];
    map_anaf_entry(entry, cleaned).ok_or(AppError::NotFound)
}

/// Map one `found[i]` entry from the ANAF v9 PlatitorTvaRest reply to `AnafCompanyData`. Pure +
/// unit-tested so the (subtle) field keys stay correct: `inregistrare_scop_Tva` (capital T),
/// the structured `adresa_sediu_social` for city/county, cash-VAT, e-Factura, and inactive status.
/// Returns `None` if `date_generale` is missing.
fn map_anaf_entry(entry: &serde_json::Value, cleaned: String) -> Option<AnafCompanyData> {
    let date_gen = entry.get("date_generale")?;
    // NB: the v9 key is `inregistrare_scop_Tva` (capital T) — the lowercase spelling never matches.
    let inreg_tva = entry.get("inregistrare_scop_Tva");
    // Structured registered-office address (date_generale has no localitate/judet — only a flat
    // `adresa` string); use the `s`-prefixed fields for a clean city + 2-letter county code.
    let sediu = entry.get("adresa_sediu_social");
    let sediu_field = |field: &str| -> String {
        sediu
            .and_then(|s| s.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let str_field = |field: &str| -> String {
        date_gen
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let opt_str_field = |field: &str| -> Option<String> {
        let s = str_field(field);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    let legal_name = str_field("denumire");
    let address = str_field("adresa");
    // City from the structured office (e.g. "Mun. Bacău"), stripping the admin prefix; county as
    // the 2-letter auto code ("BC") the app uses. Fall back to nothing if the office is absent.
    let raw_city = sediu_field("sdenumire_Localitate");
    let city = if raw_city.contains("ucure") {
        // The capital arrives as "Sector N Mun. Bucureşti" (note the ş) — normalise to "București".
        "București".to_string()
    } else {
        raw_city
            .strip_prefix("Mun. ")
            .or_else(|| raw_city.strip_prefix("Municipiul "))
            .or_else(|| raw_city.strip_prefix("Oraș "))
            .or_else(|| raw_city.strip_prefix("Com. "))
            .or_else(|| raw_city.strip_prefix("Sat "))
            .unwrap_or(&raw_city)
            .trim()
            .to_string()
    };
    let county = sediu_field("scod_JudetAuto").to_uppercase();
    let postal_code = opt_str_field("codPostal");
    let registry_number = opt_str_field("nrRegCom");
    let phone = opt_str_field("telefon");

    // VAT scope, cash VAT, e-Factura registration, inactive status.
    let bool_at = |obj: Option<&serde_json::Value>, field: &str| -> bool {
        obj.and_then(|v| v.get(field))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let vat_payer = bool_at(inreg_tva, "scpTVA");
    let cash_vat = bool_at(entry.get("inregistrare_RTVAI"), "statusTvaIncasare");
    let efactura_registered = date_gen
        .get("statusRO_e_Factura")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let active = !bool_at(entry.get("stare_inactiv"), "statusInactivi");

    Some(AnafCompanyData {
        cui: cleaned,
        legal_name,
        address,
        city,
        county,
        postal_code,
        registry_number,
        phone,
        vat_payer,
        cash_vat,
        efactura_registered,
        active,
    })
}

#[cfg(test)]
mod anaf_tests {
    use super::map_anaf_entry;

    #[test]
    fn maps_v9_entry_with_correct_keys() {
        // Shape mirrors the real ANAF v9 reply (verified against a live lookup).
        let entry = serde_json::json!({
            "date_generale": {
                "denumire": "DEDEMAN SRL",
                "adresa": "JUD. BACĂU, MUN. BACĂU, STR. ALEXEI TOLSTOI, NR.8",
                "nrRegCom": "J04/123/1993",
                "telefon": "0234513330",
                "codPostal": "600093",
                "statusRO_e_Factura": true
            },
            "inregistrare_scop_Tva": { "scpTVA": true },
            "inregistrare_RTVAI": { "statusTvaIncasare": false },
            "stare_inactiv": { "statusInactivi": false },
            "adresa_sediu_social": {
                "sdenumire_Localitate": "Mun. Bacău",
                "sdenumire_Judet": "BACĂU",
                "scod_JudetAuto": "BC",
                "scod_Postal": "600093"
            }
        });
        let d = map_anaf_entry(&entry, "2816464".into()).expect("maps");
        assert_eq!(d.legal_name, "DEDEMAN SRL");
        assert!(d.vat_payer, "scpTVA under the capital-T key must be read");
        assert!(!d.cash_vat);
        assert!(d.efactura_registered);
        assert!(d.active, "statusInactivi=false → active");
        assert_eq!(d.city, "Bacău", "admin prefix stripped");
        assert_eq!(d.county, "BC", "2-letter auto code");
    }

    #[test]
    fn inactive_and_cash_vat_are_flagged() {
        let entry = serde_json::json!({
            "date_generale": { "denumire": "X SRL", "adresa": "" },
            "inregistrare_scop_Tva": { "scpTVA": true },
            "inregistrare_RTVAI": { "statusTvaIncasare": true },
            "stare_inactiv": { "statusInactivi": true }
        });
        let d = map_anaf_entry(&entry, "123".into()).expect("maps");
        assert!(d.cash_vat, "statusTvaIncasare=true");
        assert!(!d.active, "statusInactivi=true → inactive");
    }

    #[test]
    fn bucharest_locality_normalises_to_city_and_sector_county() {
        // The capital arrives as "Sector N Mun. Bucureşti" with county auto-code "B".
        let entry = serde_json::json!({
            "date_generale": { "denumire": "Y SRL", "adresa": "" },
            "adresa_sediu_social": {
                "sdenumire_Localitate": "Sector 6 Mun. Bucureşti",
                "scod_JudetAuto": "B"
            }
        });
        let d = map_anaf_entry(&entry, "1".into()).expect("maps");
        assert_eq!(d.city, "București");
        assert_eq!(d.county, "B");
    }

    #[test]
    fn missing_date_generale_returns_none() {
        assert!(map_anaf_entry(&serde_json::json!({}), "1".into()).is_none());
    }
}
