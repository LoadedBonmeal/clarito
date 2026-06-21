//! P2 Wave 7: payroll GL account map + diurnă — company-scoped override layer.
//!
//! Standard defaults are code-authoritative (OMFP 1802/2014 monograph). Any NULL
//! column in the DB row falls back to its default here → `post_payroll` is
//! byte-identical when no override row exists (existing golden tests pass unchanged).
//! Diurnă plafoane: CF art.76(2)(k) + art.142(g): 57,50 = 2,5 × 23 lei/zi.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::models::{new_id, now_unix};
use crate::error::AppResult;

// ─── Defaults ─────────────────────────────────────────────────────────────────

pub const DEFAULT_CHELTUIELI_SALARII: &str = "641";
pub const DEFAULT_SALARII_DATORATE: &str = "421";
pub const DEFAULT_CAS: &str = "4315";
pub const DEFAULT_CASS: &str = "4316";
pub const DEFAULT_IMPOZIT: &str = "444";
pub const DEFAULT_CHELTUIELI_CAM: &str = "646";
pub const DEFAULT_CAM: &str = "436";
pub const DEFAULT_CONCEDII: &str = "4373";
pub const DEFAULT_CHELTUIELI_CONCEDII: &str = "6458";
pub const DEFAULT_NET_CASA: &str = "5311";
pub const DEFAULT_NET_BANCA: &str = "5121";

// Diurnă plafoane (CF art.76(2)(k) + art.142(g)):
pub const DEFAULT_DIURNA_INTERNA: &str = "23.00";
pub const DEFAULT_DIURNA_PLAFON_NEIMPOZABIL: &str = "57.50"; // 2.5 × 23
pub const DEFAULT_DIURNA_CAZARE: &str = "265.00";

// ─── Structs ──────────────────────────────────────────────────────────────────

/// Effective payroll account map + diurnă constants for a company.
/// All fields are always populated (defaults applied on retrieval).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PayrollAccountMap {
    pub cheltuieli_salarii: String,
    pub salarii_datorate: String,
    pub cas: String,
    pub cass: String,
    pub impozit: String,
    pub cheltuieli_cam: String,
    pub cam: String,
    pub concedii: String,
    pub cheltuieli_concedii: String,
    pub net_casa: String,
    pub net_banca: String,
    pub diurna_interna: String,
    pub diurna_plafon_neimpozabil: String,
    pub diurna_cazare: String,
    /// True when a company override row exists in payroll_config.
    pub is_override: bool,
}

impl Default for PayrollAccountMap {
    fn default() -> Self {
        Self {
            cheltuieli_salarii: DEFAULT_CHELTUIELI_SALARII.into(),
            salarii_datorate: DEFAULT_SALARII_DATORATE.into(),
            cas: DEFAULT_CAS.into(),
            cass: DEFAULT_CASS.into(),
            impozit: DEFAULT_IMPOZIT.into(),
            cheltuieli_cam: DEFAULT_CHELTUIELI_CAM.into(),
            cam: DEFAULT_CAM.into(),
            concedii: DEFAULT_CONCEDII.into(),
            cheltuieli_concedii: DEFAULT_CHELTUIELI_CONCEDII.into(),
            net_casa: DEFAULT_NET_CASA.into(),
            net_banca: DEFAULT_NET_BANCA.into(),
            diurna_interna: DEFAULT_DIURNA_INTERNA.into(),
            diurna_plafon_neimpozabil: DEFAULT_DIURNA_PLAFON_NEIMPOZABIL.into(),
            diurna_cazare: DEFAULT_DIURNA_CAZARE.into(),
            is_override: false,
        }
    }
}

/// Input for set_payroll_config (all fields optional — None = keep/use default).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPayrollConfigInput {
    pub cont_cheltuieli_salarii: Option<String>,
    pub cont_salarii_datorate: Option<String>,
    pub cont_cas: Option<String>,
    pub cont_cass: Option<String>,
    pub cont_impozit: Option<String>,
    pub cont_cheltuieli_cam: Option<String>,
    pub cont_cam: Option<String>,
    pub cont_concedii: Option<String>,
    pub cont_cheltuieli_concedii: Option<String>,
    pub cont_net_casa: Option<String>,
    pub cont_net_banca: Option<String>,
    pub diurna_interna: Option<String>,
    pub diurna_plafon_neimpozabil: Option<String>,
    pub diurna_cazare: Option<String>,
}

// ─── DB row (private) ─────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
struct PayrollConfigRow {
    cont_cheltuieli_salarii: Option<String>,
    cont_salarii_datorate: Option<String>,
    cont_cas: Option<String>,
    cont_cass: Option<String>,
    cont_impozit: Option<String>,
    cont_cheltuieli_cam: Option<String>,
    cont_cam: Option<String>,
    cont_concedii: Option<String>,
    cont_cheltuieli_concedii: Option<String>,
    cont_net_casa: Option<String>,
    cont_net_banca: Option<String>,
    diurna_interna: Option<String>,
    diurna_plafon_neimpozabil: Option<String>,
    diurna_cazare: Option<String>,
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Merge a DB override row onto the code defaults; any NULL column falls back.
fn effective_account_map(row: Option<PayrollConfigRow>) -> PayrollAccountMap {
    match row {
        None => PayrollAccountMap::default(),
        Some(r) => PayrollAccountMap {
            cheltuieli_salarii: r
                .cont_cheltuieli_salarii
                .unwrap_or_else(|| DEFAULT_CHELTUIELI_SALARII.into()),
            salarii_datorate: r
                .cont_salarii_datorate
                .unwrap_or_else(|| DEFAULT_SALARII_DATORATE.into()),
            cas: r.cont_cas.unwrap_or_else(|| DEFAULT_CAS.into()),
            cass: r.cont_cass.unwrap_or_else(|| DEFAULT_CASS.into()),
            impozit: r.cont_impozit.unwrap_or_else(|| DEFAULT_IMPOZIT.into()),
            cheltuieli_cam: r
                .cont_cheltuieli_cam
                .unwrap_or_else(|| DEFAULT_CHELTUIELI_CAM.into()),
            cam: r.cont_cam.unwrap_or_else(|| DEFAULT_CAM.into()),
            concedii: r.cont_concedii.unwrap_or_else(|| DEFAULT_CONCEDII.into()),
            cheltuieli_concedii: r
                .cont_cheltuieli_concedii
                .unwrap_or_else(|| DEFAULT_CHELTUIELI_CONCEDII.into()),
            net_casa: r.cont_net_casa.unwrap_or_else(|| DEFAULT_NET_CASA.into()),
            net_banca: r.cont_net_banca.unwrap_or_else(|| DEFAULT_NET_BANCA.into()),
            diurna_interna: r
                .diurna_interna
                .unwrap_or_else(|| DEFAULT_DIURNA_INTERNA.into()),
            diurna_plafon_neimpozabil: r
                .diurna_plafon_neimpozabil
                .unwrap_or_else(|| DEFAULT_DIURNA_PLAFON_NEIMPOZABIL.into()),
            diurna_cazare: r
                .diurna_cazare
                .unwrap_or_else(|| DEFAULT_DIURNA_CAZARE.into()),
            is_override: true,
        },
    }
}

// ─── Queries ──────────────────────────────────────────────────────────────────

/// Return the effective payroll config for a company (override merged onto defaults).
pub async fn get_payroll_config(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<PayrollAccountMap> {
    let row = sqlx::query_as::<_, PayrollConfigRow>(
        "SELECT cont_cheltuieli_salarii, cont_salarii_datorate, cont_cas, cont_cass, \
         cont_impozit, cont_cheltuieli_cam, cont_cam, cont_concedii, cont_cheltuieli_concedii, \
         cont_net_casa, cont_net_banca, diurna_interna, diurna_plafon_neimpozabil, diurna_cazare \
         FROM payroll_config WHERE company_id = ?1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(effective_account_map(row))
}

/// Upsert a company override row. All fields are individually nullable.
pub async fn set_payroll_config(
    pool: &SqlitePool,
    company_id: &str,
    input: SetPayrollConfigInput,
) -> AppResult<PayrollAccountMap> {
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO payroll_config \
         (id, company_id, cont_cheltuieli_salarii, cont_salarii_datorate, cont_cas, cont_cass, \
          cont_impozit, cont_cheltuieli_cam, cont_cam, cont_concedii, cont_cheltuieli_concedii, \
          cont_net_casa, cont_net_banca, diurna_interna, diurna_plafon_neimpozabil, diurna_cazare, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17) \
         ON CONFLICT(company_id) DO UPDATE SET \
         cont_cheltuieli_salarii   = excluded.cont_cheltuieli_salarii, \
         cont_salarii_datorate     = excluded.cont_salarii_datorate, \
         cont_cas                  = excluded.cont_cas, \
         cont_cass                 = excluded.cont_cass, \
         cont_impozit              = excluded.cont_impozit, \
         cont_cheltuieli_cam       = excluded.cont_cheltuieli_cam, \
         cont_cam                  = excluded.cont_cam, \
         cont_concedii             = excluded.cont_concedii, \
         cont_cheltuieli_concedii  = excluded.cont_cheltuieli_concedii, \
         cont_net_casa             = excluded.cont_net_casa, \
         cont_net_banca            = excluded.cont_net_banca, \
         diurna_interna            = excluded.diurna_interna, \
         diurna_plafon_neimpozabil = excluded.diurna_plafon_neimpozabil, \
         diurna_cazare             = excluded.diurna_cazare, \
         updated_at                = excluded.updated_at",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.cont_cheltuieli_salarii)
    .bind(&input.cont_salarii_datorate)
    .bind(&input.cont_cas)
    .bind(&input.cont_cass)
    .bind(&input.cont_impozit)
    .bind(&input.cont_cheltuieli_cam)
    .bind(&input.cont_cam)
    .bind(&input.cont_concedii)
    .bind(&input.cont_cheltuieli_concedii)
    .bind(&input.cont_net_casa)
    .bind(&input.cont_net_banca)
    .bind(&input.diurna_interna)
    .bind(&input.diurna_plafon_neimpozabil)
    .bind(&input.diurna_cazare)
    .bind(now)
    .execute(pool)
    .await?;

    get_payroll_config(pool, company_id).await
}

/// Delete the company override → reverts to code defaults.
pub async fn reset_payroll_config(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<PayrollAccountMap> {
    sqlx::query("DELETE FROM payroll_config WHERE company_id = ?1")
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(PayrollAccountMap::default())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn setup() -> SqlitePool {
        let pool = test_pool().await;
        // Ensure company exists (payroll_config has FK → companies).
        sqlx::query(
            "INSERT OR IGNORE INTO companies \
             (id, cui, legal_name, address, city, county, country, created_at, updated_at) \
             VALUES ('co1','RO1','Test SRL','Str. 1','Cluj','CJ','RO',0,0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn returns_defaults_when_no_override() {
        let pool = setup().await;
        let cfg = get_payroll_config(&pool, "co1").await.unwrap();
        assert_eq!(cfg.cheltuieli_salarii, "641");
        assert_eq!(cfg.salarii_datorate, "421");
        assert_eq!(cfg.cas, "4315");
        assert_eq!(cfg.cass, "4316");
        assert_eq!(cfg.impozit, "444");
        assert_eq!(cfg.cheltuieli_cam, "646");
        assert_eq!(cfg.cam, "436");
        assert_eq!(cfg.concedii, "4373");
        assert_eq!(cfg.cheltuieli_concedii, "6458");
        assert_eq!(cfg.net_casa, "5311");
        assert_eq!(cfg.net_banca, "5121");
        assert_eq!(cfg.diurna_interna, "23.00");
        assert_eq!(cfg.diurna_plafon_neimpozabil, "57.50");
        assert_eq!(cfg.diurna_cazare, "265.00");
        assert!(!cfg.is_override);
    }

    #[tokio::test]
    async fn returns_override_when_set() {
        let pool = setup().await;
        let cfg = set_payroll_config(
            &pool,
            "co1",
            SetPayrollConfigInput {
                cont_cheltuieli_salarii: Some("641.1".into()),
                cont_salarii_datorate: Some("421.1".into()),
                cont_cas: None,
                cont_cass: None,
                cont_impozit: None,
                cont_cheltuieli_cam: None,
                cont_cam: None,
                cont_concedii: None,
                cont_cheltuieli_concedii: None,
                cont_net_casa: None,
                cont_net_banca: None,
                diurna_interna: None,
                diurna_plafon_neimpozabil: None,
                diurna_cazare: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(cfg.cheltuieli_salarii, "641.1");
        assert_eq!(cfg.salarii_datorate, "421.1");
        // Unset columns fall back to defaults:
        assert_eq!(cfg.cas, "4315");
        assert_eq!(cfg.cam, "436");
        assert!(cfg.is_override);
    }

    #[tokio::test]
    async fn reset_reverts_to_defaults() {
        let pool = setup().await;
        set_payroll_config(
            &pool,
            "co1",
            SetPayrollConfigInput {
                cont_cheltuieli_salarii: Some("641.9".into()),
                cont_salarii_datorate: None,
                cont_cas: None,
                cont_cass: None,
                cont_impozit: None,
                cont_cheltuieli_cam: None,
                cont_cam: None,
                cont_concedii: None,
                cont_cheltuieli_concedii: None,
                cont_net_casa: None,
                cont_net_banca: None,
                diurna_interna: None,
                diurna_plafon_neimpozabil: None,
                diurna_cazare: None,
            },
        )
        .await
        .unwrap();

        let cfg = reset_payroll_config(&pool, "co1").await.unwrap();
        assert_eq!(cfg.cheltuieli_salarii, "641");
        assert!(!cfg.is_override);
    }

    #[tokio::test]
    async fn single_override_column_falls_back_others() {
        let pool = setup().await;
        set_payroll_config(
            &pool,
            "co1",
            SetPayrollConfigInput {
                cont_salarii_datorate: Some("421.2".into()),
                cont_cheltuieli_salarii: None,
                cont_cas: None,
                cont_cass: None,
                cont_impozit: None,
                cont_cheltuieli_cam: None,
                cont_cam: None,
                cont_concedii: None,
                cont_cheltuieli_concedii: None,
                cont_net_casa: None,
                cont_net_banca: None,
                diurna_interna: None,
                diurna_plafon_neimpozabil: None,
                diurna_cazare: None,
            },
        )
        .await
        .unwrap();
        let cfg = get_payroll_config(&pool, "co1").await.unwrap();
        // Overridden:
        assert_eq!(cfg.salarii_datorate, "421.2");
        // Others still at default:
        assert_eq!(cfg.cheltuieli_salarii, "641");
        assert_eq!(cfg.cas, "4315");
        assert_eq!(cfg.cass, "4316");
        assert_eq!(cfg.impozit, "444");
        assert_eq!(cfg.cheltuieli_cam, "646");
        assert_eq!(cfg.cam, "436");
        assert_eq!(cfg.concedii, "4373");
        assert_eq!(cfg.cheltuieli_concedii, "6458");
        assert_eq!(cfg.net_casa, "5311");
        assert_eq!(cfg.net_banca, "5121");
        assert!(cfg.is_override);
    }

    #[test]
    fn effective_account_map_none_gives_defaults() {
        let m = effective_account_map(None);
        assert_eq!(m, PayrollAccountMap::default());
        assert!(!m.is_override);
    }

    // ─── post_payroll integration tests ───────────────────────────────────────

    /// Helper: create a company + single employee and run payroll for 2026-06.
    async fn setup_payroll(pool: &SqlitePool) {
        sqlx::query(
            "INSERT OR IGNORE INTO companies \
             (id, cui, legal_name, address, city, county, country, created_at, updated_at) \
             VALUES ('co_pay','RO999','Pay SRL','Str. 1','Cluj','CJ','RO',0,0)",
        )
        .execute(pool)
        .await
        .unwrap();
        crate::db::payroll::create(
            pool,
            crate::db::payroll::CreateEmployeeInput {
                company_id: "co_pay".into(),
                cnp: "9".into(),
                full_name: "Test Salariat".into(),
                gross_salary: "4000".into(),
                personal_deduction: Some("0".into()),
                employment_date: None,
                contract_end_date: None,
                tip_asigurat: None,
                pensionar: None,
                tip_contract: None,
                ore_norma: None,
                exceptie_cas_min: None,
                sediu_cif: None,
                beneficiar_suma_netaxabila: None,
            },
        )
        .await
        .unwrap();
    }

    /// No config override → post_payroll uses standard accounts (641/421/4315/4316/444/646/436).
    #[tokio::test]
    async fn post_payroll_no_config_uses_default_accounts() {
        let pool = test_pool().await;
        setup_payroll(&pool).await;

        crate::db::payroll::run_payroll(&pool, "co_pay", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        let tb = crate::db::gl::trial_balance(&pool, "co_pay", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| -> bool { tb.rows.iter().any(|r| r.account_code == code) };
        // Standard accounts must appear.
        assert!(bal("641"), "641 cheltuieli salarii expected");
        assert!(bal("421"), "421 salarii datorate expected");
        assert!(bal("4315"), "4315 CAS expected");
        assert!(bal("4316"), "4316 CASS expected");
        assert!(bal("444"), "444 impozit expected");
        assert!(bal("646"), "646 cheltuieli CAM expected");
        assert!(bal("436"), "436 CAM expected");
        assert!(tb.balanced, "GL must be balanced");
    }

    /// With an account override → post_payroll routes to the analytic account.
    #[tokio::test]
    async fn post_payroll_with_override_routes_to_analytic_account() {
        let pool = test_pool().await;
        setup_payroll(&pool).await;

        // Override 421 → 421.1 (analytic for employee salaries).
        set_payroll_config(
            &pool,
            "co_pay",
            SetPayrollConfigInput {
                cont_salarii_datorate: Some("421.1".into()),
                cont_cheltuieli_salarii: None,
                cont_cas: None,
                cont_cass: None,
                cont_impozit: None,
                cont_cheltuieli_cam: None,
                cont_cam: None,
                cont_concedii: None,
                cont_cheltuieli_concedii: None,
                cont_net_casa: None,
                cont_net_banca: None,
                diurna_interna: None,
                diurna_plafon_neimpozabil: None,
                diurna_cazare: None,
            },
        )
        .await
        .unwrap();

        crate::db::payroll::run_payroll(&pool, "co_pay", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        let tb = crate::db::gl::trial_balance(&pool, "co_pay", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| -> bool { tb.rows.iter().any(|r| r.account_code == code) };
        // Overridden account must appear; original 421 must NOT.
        assert!(bal("421.1"), "421.1 (override) expected in GL");
        assert!(!bal("421"), "421 (default) must NOT appear when overridden");
        // Non-overridden accounts remain at defaults.
        assert!(bal("641"), "641 still default");
        assert!(bal("4315"), "4315 still default");
        assert!(tb.balanced, "GL must be balanced with analytic account");
    }
}
