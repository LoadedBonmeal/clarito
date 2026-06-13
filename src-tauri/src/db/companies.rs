//! Companii (entitățile pe care utilizatorul le administrează).
//!
//! Suport multi-tenant: un user poate avea N companii. Tier-ul licenței
//! limitează numărul (verificarea se face în layer-ul de comenzi).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// SQL-01: the canonical Company column list — shared by every `SELECT … FROM companies`
/// so the 23-column projection (and FromRow mapping) lives in exactly one place.
const COMPANY_SELECT: &str = "SELECT id, cui, legal_name, trade_name, registry_number, vat_payer, \
    cash_vat, address, city, county, postal_code, country, email, phone, iban, bank_name, \
    is_active, spv_enabled, tax_regime, invoice_series, last_invoice_number, logo_path, \
    created_at, updated_at FROM companies";

/// 2026 micro-enterprise turnover ceiling in EUR (OUG 89/2025). Above its lei-equivalent (at the
/// year-end BNR rate) the company owes profit tax (16%) from the quarter the ceiling was exceeded.
pub const MICRO_CEILING_EUR: i64 = 100_000;

/// 2026 cash-VAT (TVA la încasare) eligibility plafon in lei (OUG 8/2026, art. 282 alin. (3) lit. a):
/// 5.000.000 lei from 1 March 2026. Above it the company leaves cash VAT at the end of the period
/// FOLLOWING the one in which the plafon was exceeded.
pub const CASH_VAT_PLAFON_RON: i64 = 5_000_000;

/// Micro-ceiling + cash-VAT-plafon monitoring result for a company.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxRegimeStatus {
    pub tax_regime: String,
    pub ytd_turnover_ron: String,
    pub ceiling_ron: String,
    pub pct: i64,
    /// "ok" | "approaching" (≥80%) | "exceeded" (>100%) | "na" (profit regime).
    pub level: String,
    pub note: Option<String>,
    /// Cash-VAT plafon (5.000.000 lei) status. "na" when the company isn't on cash VAT.
    pub cash_vat_plafon_ron: String,
    pub cash_vat_level: String,
    pub cash_vat_note: Option<String>,
}

/// Cash-VAT plafon status (level, note) from the YTD turnover. "na" when not on cash VAT.
fn cash_vat_plafon_status(
    cash_vat: bool,
    ytd_turnover_ron: Decimal,
) -> (&'static str, Option<String>) {
    if !cash_vat {
        return ("na", None);
    }
    let plafon = Decimal::from(CASH_VAT_PLAFON_RON);
    if ytd_turnover_ron > plafon {
        (
            "exceeded",
            Some(
                "Ați depășit plafonul TVA la încasare de 5.000.000 lei. Treceți la exigibilitate \
             normală de la sfârșitul perioadei fiscale următoare celei în care l-ați depășit; \
             depuneți declarația de mențiuni până pe 20 (OUG 8/2026)."
                    .to_string(),
            ),
        )
    } else if ytd_turnover_ron * Decimal::from(100) >= plafon * Decimal::from(80) {
        ("approaching", Some(
            "Vă apropiați de plafonul TVA la încasare de 5.000.000 lei — monitorizați cifra de \
             afaceri."
                .to_string(),
        ))
    } else {
        ("ok", None)
    }
}

/// Compute the micro-ceiling + cash-VAT status from the YTD turnover (RON) and the EUR→RON rate.
/// Pure + testable. For a profit-tax company the micro ceiling does not apply (level "na").
pub fn micro_ceiling_status(
    tax_regime: &str,
    cash_vat: bool,
    ytd_turnover_ron: Decimal,
    eur_ron: Decimal,
) -> TaxRegimeStatus {
    let (cv_level, cv_note) = cash_vat_plafon_status(cash_vat, ytd_turnover_ron);
    let cash_vat_plafon_ron = format!("{}.00", CASH_VAT_PLAFON_RON);
    let ceiling = Decimal::from(MICRO_CEILING_EUR) * eur_ron;
    if tax_regime != "micro" {
        return TaxRegimeStatus {
            tax_regime: tax_regime.to_string(),
            ytd_turnover_ron: format!("{:.2}", ytd_turnover_ron),
            ceiling_ron: format!("{:.2}", ceiling),
            pct: 0,
            level: "na".into(),
            note: None,
            cash_vat_plafon_ron,
            cash_vat_level: cv_level.into(),
            cash_vat_note: cv_note,
        };
    }
    let pct = if ceiling.is_zero() {
        0
    } else {
        (ytd_turnover_ron / ceiling * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap_or(0)
    };
    let (level, note) =
        if ytd_turnover_ron > ceiling {
            ("exceeded", Some(
            "Ați depășit plafonul micro de 100.000 EUR. Treceți la impozit pe profit (16%) \
             începând cu trimestrul în care a fost depășit plafonul (OUG 89/2025)."
                .to_string(),
        ))
        } else if pct >= 80 {
            ("approaching", Some(
            "Vă apropiați de plafonul micro de 100.000 EUR — monitorizați cifra de afaceri pentru \
             a anticipa trecerea la impozit pe profit.".to_string(),
        ))
        } else {
            ("ok", None)
        };
    TaxRegimeStatus {
        tax_regime: tax_regime.to_string(),
        ytd_turnover_ron: format!("{:.2}", ytd_turnover_ron),
        ceiling_ron: format!("{:.2}", ceiling),
        pct,
        level: level.into(),
        note,
        cash_vat_plafon_ron,
        cash_vat_level: cv_level.into(),
        cash_vat_note: cv_note,
    }
}

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: String,
    pub cui: String,
    pub legal_name: String,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: bool,
    /// TVA la încasare (cash-VAT regime). When true, VAT exigibility is deferred to
    /// collection — see src-tauri/CASH_VAT_DESIGN.md.
    pub cash_vat: bool,

    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub country: String,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub is_active: bool,
    pub spv_enabled: bool,

    /// Tax regime: "micro" (impozit pe venit 1%, ceiling 100.000 EUR — 2026) or "profit"
    /// (impozit pe profit 16%). Drives the micro-ceiling warning + which declarations apply.
    pub tax_regime: String,

    pub invoice_series: String,
    pub last_invoice_number: i64,

    pub logo_path: Option<String>,

    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCompanyInput {
    pub cui: String,
    pub legal_name: String,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: Option<bool>,

    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub invoice_series: Option<String>,
    /// "micro" (default) or "profit" — the tax regime, settable at creation.
    pub tax_regime: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCompanyInput {
    pub legal_name: Option<String>,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: Option<bool>,
    pub cash_vat: Option<bool>,

    pub address: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub is_active: Option<bool>,
    pub spv_enabled: Option<bool>,
    pub tax_regime: Option<String>,

    pub invoice_series: Option<String>,
    pub logo_path: Option<String>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<Company>> {
    let rows = sqlx::query_as::<_, Company>(&format!(
        "{COMPANY_SELECT} WHERE is_active = 1 ORDER BY legal_name"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Company> {
    let row = sqlx::query_as::<_, Company>(&format!("{COMPANY_SELECT} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row)
}

// No non-test caller exists yet; retained for future command-layer use and tested directly.
#[allow(dead_code)]
pub async fn get_by_cui(pool: &SqlitePool, cui: &str) -> AppResult<Option<Company>> {
    // Task 5: only active companies block re-registration of the same CUI.
    // A soft-deleted (is_active = 0) company must not prevent re-adding the same CUI.
    let row = sqlx::query_as::<_, Company>(&format!(
        "{COMPANY_SELECT} WHERE cui = ?1 AND is_active = 1"
    ))
    .bind(cui)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Look up a company by CUI regardless of is_active status.
/// Used by `create` to detect soft-deleted rows that can be reactivated.
async fn get_by_cui_any_status(pool: &SqlitePool, cui: &str) -> AppResult<Option<Company>> {
    let row = sqlx::query_as::<_, Company>(&format!("{COMPANY_SELECT} WHERE cui = ?1"))
        .bind(cui)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create(pool: &SqlitePool, input: CreateCompanyInput) -> AppResult<Company> {
    validate_cui(&input.cui)?;

    // Task 5 (production fix): the schema has a TABLE-LEVEL UNIQUE on cui over ALL rows
    // (including soft-deleted ones), so a plain INSERT after the active-only check still
    // hits the DB constraint.  Instead we look up any existing row for this CUI:
    //  • active    → reject as duplicate (same behaviour as before)
    //  • soft-deleted → REACTIVATE: update the existing row in-place so the company's
    //    id, historical invoices and contacts are preserved; avoid the UNIQUE conflict.
    //  • none      → insert as normal.
    if let Some(existing) = get_by_cui_any_status(pool, &input.cui).await? {
        if existing.is_active {
            return Err(AppError::Conflict(format!(
                "Există deja o companie cu CUI {}",
                input.cui
            )));
        }
        // Reactivate the soft-deleted record.
        let now = now_unix();
        sqlx::query(
            "UPDATE companies SET
                legal_name      = ?2,
                trade_name      = ?3,
                registry_number = ?4,
                vat_payer       = ?5,
                address         = ?6,
                city            = ?7,
                county          = ?8,
                postal_code     = ?9,
                country         = ?10,
                email           = ?11,
                phone           = ?12,
                iban            = ?13,
                bank_name       = ?14,
                invoice_series  = ?15,
                tax_regime      = ?17,
                is_active       = 1,
                updated_at      = ?16
            WHERE id = ?1",
        )
        .bind(&existing.id)
        .bind(&input.legal_name)
        .bind(&input.trade_name)
        .bind(&input.registry_number)
        .bind(input.vat_payer.unwrap_or(true))
        .bind(&input.address)
        .bind(&input.city)
        .bind(&input.county)
        .bind(&input.postal_code)
        .bind(input.country.as_deref().unwrap_or("RO"))
        .bind(&input.email)
        .bind(&input.phone)
        .bind(&input.iban)
        .bind(&input.bank_name)
        .bind(input.invoice_series.as_deref().unwrap_or("FACT"))
        .bind(now)
        .bind(input.tax_regime.as_deref().unwrap_or("micro"))
        .execute(pool)
        .await?;
        return get(pool, &existing.id).await;
    }

    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO companies (
            id, cui, legal_name, trade_name, registry_number, vat_payer,
            address, city, county, postal_code, country,
            email, phone, iban, bank_name,
            invoice_series, tax_regime, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11,
            ?12, ?13, ?14, ?15,
            ?16, ?18, ?17, ?17
        )",
    )
    .bind(&id)
    .bind(&input.cui)
    .bind(&input.legal_name)
    .bind(&input.trade_name)
    .bind(&input.registry_number)
    .bind(input.vat_payer.unwrap_or(true))
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.county)
    .bind(&input.postal_code)
    .bind(input.country.as_deref().unwrap_or("RO"))
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.iban)
    .bind(&input.bank_name)
    .bind(input.invoice_series.as_deref().unwrap_or("FACT"))
    .bind(now)
    .bind(input.tax_regime.as_deref().unwrap_or("micro"))
    .execute(pool)
    .await?;

    get(pool, &id).await
}

pub async fn update(pool: &SqlitePool, id: &str, input: UpdateCompanyInput) -> AppResult<Company> {
    // Asigură existența + colectează vechile valori.
    let current = get(pool, id).await?;
    let now = now_unix();

    sqlx::query(
        "UPDATE companies SET
            legal_name      = ?2,
            trade_name      = ?3,
            registry_number = ?4,
            vat_payer       = ?5,
            address         = ?6,
            city            = ?7,
            county          = ?8,
            postal_code     = ?9,
            country         = ?10,
            email           = ?11,
            phone           = ?12,
            iban            = ?13,
            bank_name       = ?14,
            is_active       = ?15,
            spv_enabled     = ?16,
            invoice_series  = ?17,
            logo_path       = ?18,
            updated_at      = ?19,
            cash_vat        = ?20,
            tax_regime      = ?21
        WHERE id = ?1",
    )
    .bind(id)
    .bind(input.legal_name.unwrap_or(current.legal_name))
    .bind(input.trade_name.or(current.trade_name))
    .bind(input.registry_number.or(current.registry_number))
    .bind(input.vat_payer.unwrap_or(current.vat_payer))
    .bind(input.address.unwrap_or(current.address))
    .bind(input.city.unwrap_or(current.city))
    .bind(input.county.unwrap_or(current.county))
    .bind(input.postal_code.or(current.postal_code))
    .bind(input.country.unwrap_or(current.country))
    .bind(input.email.or(current.email))
    .bind(input.phone.or(current.phone))
    .bind(input.iban.or(current.iban))
    .bind(input.bank_name.or(current.bank_name))
    .bind(input.is_active.unwrap_or(current.is_active))
    .bind(input.spv_enabled.unwrap_or(current.spv_enabled))
    .bind(input.invoice_series.unwrap_or(current.invoice_series))
    .bind(input.logo_path.or(current.logo_path))
    .bind(now)
    .bind(input.cash_vat.unwrap_or(current.cash_vat))
    .bind(input.tax_regime.unwrap_or(current.tax_regime))
    .execute(pool)
    .await?;

    get(pool, id).await
}

/// Soft-delete (is_active = 0). Hard delete necesită confirmare separată
/// pentru a păstra integritatea referențială cu facturile istorice.
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let now = now_unix();
    let res = sqlx::query("UPDATE companies SET is_active = 0, updated_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Returnează `last_invoice_number + 1` fără a modifica baza de date.
/// Folosit doar pentru afișarea previzualizată a numărului pe formulare.
/// Numărul real este alocat atomic de `allocate_invoice_number` la salvare.
pub async fn next_invoice_number(pool: &SqlitePool, company_id: &str) -> AppResult<i64> {
    let current: i64 =
        sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id = ?1")
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;
    Ok(current + 1)
}

// ─── Validation ────────────────────────────────────────────────────────────

/// Pondere cheie pentru algoritmul mod-11 de validare a CUI-ului românesc.
/// Aplicată de la dreapta spre stânga pe cifrele corpului (fără cifra de control).
const CUI_KEY: [u32; 9] = [7, 5, 3, 2, 1, 7, 5, 3, 2];

/// CIF românesc: opțional prefix "RO"/"ro" + spații + 2-10 cifre.
/// Verifică și cifra de control (algoritm mod-11 oficial ANAF).
pub fn validate_cui(cui: &str) -> AppResult<()> {
    let trimmed = cui.trim().to_uppercase();
    let digits = trimmed.strip_prefix("RO").unwrap_or(&trimmed).trim();

    // Lungime și conținut.
    if digits.len() < 2 || digits.len() > 10 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(format!(
            "CUI invalid: '{cui}'. Format așteptat: 2-10 cifre, cu sau fără prefix RO."
        )));
    }

    // Cifra de control (mod-11).
    let (body, ctrl_char) = digits.split_at(digits.len() - 1);
    let ctrl_digit = ctrl_char.chars().next().unwrap() as u32 - b'0' as u32;

    // Aliniem cheia la dreapta față de body.
    let key_slice = &CUI_KEY[CUI_KEY.len() - body.len()..];
    let sum: u32 = body
        .chars()
        .zip(key_slice.iter())
        .map(|(c, &k)| (c as u32 - b'0' as u32) * k)
        .sum();

    let expected = {
        let v = (sum * 10) % 11;
        if v == 10 {
            0
        } else {
            v
        }
    };

    if expected != ctrl_digit {
        return Err(AppError::Validation(
            "CUI invalid (cifra de control nu corespunde).".to_string(),
        ));
    }

    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::str::FromStr;

    #[test]
    fn micro_ceiling_status_levels() {
        let eur = Decimal::from_str("5.00").unwrap(); // 100.000 EUR → 500.000 RON ceiling
        let dec = |s: &str| Decimal::from_str(s).unwrap();
        // ok: 100k/500k = 20%
        let ok = micro_ceiling_status("micro", false, dec("100000"), eur);
        assert_eq!(ok.level, "ok");
        assert_eq!(ok.pct, 20);
        assert!(ok.note.is_none());
        assert_eq!(ok.cash_vat_level, "na"); // not on cash VAT
                                             // approaching at ≥80%: 450k/500k = 90%
        assert_eq!(
            micro_ceiling_status("micro", false, dec("450000"), eur).level,
            "approaching"
        );
        // exceeded: 600k > 500k → advises profit tax
        let ex = micro_ceiling_status("micro", false, dec("600000"), eur);
        assert_eq!(ex.level, "exceeded");
        assert!(ex.note.unwrap().contains("profit"));
        // profit-tax company → ceiling not applicable
        assert_eq!(
            micro_ceiling_status("profit", false, dec("600000"), eur).level,
            "na"
        );
    }

    #[test]
    fn cash_vat_plafon_levels() {
        let eur = Decimal::from_str("5.00").unwrap();
        let dec = |s: &str| Decimal::from_str(s).unwrap();
        // On cash VAT, below 80% of 5M → ok.
        assert_eq!(
            micro_ceiling_status("profit", true, dec("1000000"), eur).cash_vat_level,
            "ok"
        );
        // ≥80% of 5M (4.000.000) → approaching.
        assert_eq!(
            micro_ceiling_status("profit", true, dec("4200000"), eur).cash_vat_level,
            "approaching"
        );
        // > 5M → exceeded, advises switching to normal exigibility.
        let ex = micro_ceiling_status("profit", true, dec("5500000"), eur);
        assert_eq!(ex.cash_vat_level, "exceeded");
        assert!(ex.cash_vat_note.unwrap().contains("încasare"));
        // Not on cash VAT → na regardless of turnover.
        assert_eq!(
            micro_ceiling_status("micro", false, dec("9000000"), eur).cash_vat_level,
            "na"
        );
    }

    // ── validate_cui unit tests ──────────────────────────────────────────────

    /// CUI-uri cu prefix RO valide (checksum mod-11 corect).
    /// RO12345674: body=1234567 (7 digits), key right-aligned [7,5,3,2,1,7,5],
    ///             sum=1*7+2*5+3*3+4*2+5*1+6*7+7*5=7+10+9+8+5+42+35=116… wait —
    ///             actual right-aligned pairing with CUI_KEY[2..]: key=[3,2,1,7,5,3,2],
    ///             sum=1*3+2*2+3*1+4*7+5*5+6*3+7*2=3+4+3+28+25+18+14=95,
    ///             (95*10)%11=950%11=4 → ctrl=4. ✓
    /// Verificate cu algoritmul Python.
    #[test]
    fn validates_cui_with_ro_prefix() {
        // RO12345674 = prefix RO + body 1234567 + ctrl 4 (valid)
        assert!(validate_cui("RO12345674").is_ok());
        assert!(validate_cui("ro12345674").is_ok());
        assert!(validate_cui(" RO12345674 ").is_ok());
    }

    #[test]
    fn validates_cui_without_prefix() {
        // 12345674 = body 1234567 + ctrl 4 (valid, no prefix)
        assert!(validate_cui("12345674").is_ok());
        // 94 = body 9 + ctrl 4 (valid 2-digit CUI)
        assert!(validate_cui("94").is_ok());
    }

    #[test]
    fn rejects_invalid_cui() {
        assert!(validate_cui("").is_err());
        assert!(validate_cui("RO").is_err());
        assert!(validate_cui("RO123456789012").is_err());
        assert!(validate_cui("RO123abc").is_err());
        // Wrong check digit: RO12345678 has ctrl=4 not 8 → must fail
        assert!(validate_cui("RO12345678").is_err());
        // Wrong check digit: RO98765432 has ctrl=8 not 2 → must fail
        assert!(validate_cui("RO98765432").is_err());
    }

    /// Known-valid: RO12345674 passes; wrong ctrl digit (RO12345678) fails.
    #[test]
    fn cui_mod11_known_valid_passes_known_invalid_fails() {
        assert!(
            validate_cui("RO12345674").is_ok(),
            "RO12345674 should pass mod-11"
        );
        assert!(
            validate_cui("RO12345678").is_err(),
            "RO12345678 has wrong control digit and must fail"
        );
    }

    // ── get_by_cui: soft-deleted company does not block re-registration ──────

    async fn setup_companies_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        // UNIQUE on cui matches the production schema (0001_initial.sql).
        // This ensures the tests exercise the real DB constraint instead of
        // a fixture that silently omits it.
        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY NOT NULL,
                cui TEXT NOT NULL UNIQUE,
                legal_name TEXT NOT NULL DEFAULT '',
                trade_name TEXT,
                registry_number TEXT,
                vat_payer INTEGER NOT NULL DEFAULT 1,
                cash_vat INTEGER NOT NULL DEFAULT 0,
                address TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                county TEXT NOT NULL DEFAULT '',
                postal_code TEXT,
                country TEXT NOT NULL DEFAULT 'RO',
                email TEXT,
                phone TEXT,
                iban TEXT,
                bank_name TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                spv_enabled INTEGER NOT NULL DEFAULT 0,
                tax_regime TEXT NOT NULL DEFAULT 'micro',
                invoice_series TEXT NOT NULL DEFAULT 'FACT',
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                logo_path TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    // ── Helpers for Task-5 create() input ───────────────────────────────────

    fn make_create_input(cui: &str, legal_name: &str) -> CreateCompanyInput {
        CreateCompanyInput {
            cui: cui.to_string(),
            legal_name: legal_name.to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: Some(true),
            address: "Str. Test 1".to_string(),
            city: "București".to_string(),
            county: "B".to_string(),
            postal_code: None,
            country: Some("RO".to_string()),
            email: None,
            phone: None,
            iban: None,
            bank_name: None,
            invoice_series: None,
            tax_regime: None,
        }
    }

    // ── Task 5: reactivation via create() — exercises UNIQUE constraint ──────

    /// Task 5a: re-adding a soft-deleted company's CUI succeeds via reactivation.
    /// The UNIQUE fixture ensures the INSERT path would fail if we didn't reactivate.
    /// Checks: same id preserved, is_active=1, fields updated.
    #[tokio::test]
    async fn task5_readd_soft_deleted_cui_reactivates_and_reuses_id() {
        let pool = setup_companies_pool().await;

        // Seed a soft-deleted company.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, is_active) \
             VALUES ('old-id', 'RO12345674', 'Old Name SRL', 'Str. 1', 'Cluj', 'CJ', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Re-add via create() — must succeed, not hit the UNIQUE constraint.
        let input = make_create_input("RO12345674", "New Name SRL");
        let result = create(&pool, input).await;
        assert!(
            result.is_ok(),
            "re-adding a soft-deleted CUI must succeed: {:?}",
            result.err()
        );

        let company = result.unwrap();
        // Same id — historical data is preserved.
        assert_eq!(company.id, "old-id", "id must be reused (same row)");
        // Now active.
        assert!(company.is_active, "reactivated company must be is_active=1");
        // Fields updated from input.
        assert_eq!(
            company.legal_name, "New Name SRL",
            "legal_name must be updated"
        );
    }

    /// Task 5b: re-adding an ACTIVE company's CUI still returns a Conflict error.
    #[tokio::test]
    async fn task5_readd_active_cui_returns_conflict() {
        let pool = setup_companies_pool().await;

        // Seed an active company.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, is_active) \
             VALUES ('active-id', 'RO12345674', 'Active Company SRL', 'Str. 1', 'Cluj', 'CJ', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let input = make_create_input("RO12345674", "Duplicate Company SRL");
        let result = create(&pool, input).await;
        assert!(
            matches!(result, Err(AppError::Conflict(_))),
            "re-adding an active CUI must return Conflict, got: {:?}",
            result
        );
    }

    /// Task 5c: get_by_cui still returns None for soft-deleted (unchanged behaviour).
    #[tokio::test]
    async fn task5_get_by_cui_returns_none_for_soft_deleted() {
        let pool = setup_companies_pool().await;

        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, is_active) \
             VALUES ('soft-del', 'RO12345674', 'Old Company', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = get_by_cui(&pool, "RO12345674").await.unwrap();
        assert!(
            result.is_none(),
            "get_by_cui must return None for soft-deleted company"
        );
    }

    /// Task 5d: get_by_cui returns Some for active company (unchanged behaviour).
    #[tokio::test]
    async fn task5_get_by_cui_returns_some_for_active() {
        let pool = setup_companies_pool().await;

        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, is_active) \
             VALUES ('active-co', 'RO12345674', 'Active Company', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = get_by_cui(&pool, "RO12345674").await.unwrap();
        assert!(
            result.is_some(),
            "get_by_cui must return Some for active company"
        );
    }
}
