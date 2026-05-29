use tauri::State;

use crate::db::companies::{self, Company, CreateCompanyInput, UpdateCompanyInput};
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
    companies::create(&state.db, input).await
}

#[tauri::command]
pub async fn update_company(
    state: State<'_, AppState>,
    id: String,
    input: UpdateCompanyInput,
) -> AppResult<Company> {
    companies::update(&state.db, &id, input).await
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
        .post("https://webservicesp.anaf.ro/PlatitorTvaRest/api/v9/ws/tva")
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
    let date_gen = entry.get("date_generale").ok_or(AppError::NotFound)?;
    let inreg_tva = entry.get("inregistrare_scop_tva");

    let str_field = |field: &str| -> String {
        date_gen
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let opt_str_field = |field: &str| -> Option<String> {
        let s = date_gen
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    let legal_name = str_field("denumire");
    let address = str_field("adresa");
    let city = str_field("localitate");
    let raw_county = str_field("judet");
    let county = if raw_county.len() > 2 {
        raw_county[..2].to_uppercase()
    } else {
        raw_county.to_uppercase()
    };
    let postal_code = opt_str_field("codPostal");
    let registry_number = opt_str_field("nrRegCom");
    let phone = opt_str_field("telefon");

    let vat_payer = inreg_tva
        .and_then(|v| v.get("scpTVA"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(AnafCompanyData {
        cui: cleaned,
        legal_name,
        address,
        city,
        county,
        postal_code,
        registry_number,
        phone,
        vat_payer,
        active: true,
    })
}
