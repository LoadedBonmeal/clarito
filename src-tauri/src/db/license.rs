//! Licență (o singură înregistrare, CHECK id = 1).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::now_unix;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub id: i64,
    pub license_key: Option<String>,
    pub tier: String,
    pub activated_at: Option<i64>,
    pub expires_at: i64,
    pub machine_id: String,
    pub email: Option<String>,
    pub last_validated_at: Option<i64>,
}

const SELECT_COLUMNS: &str =
    "id, license_key, tier, activated_at, expires_at, machine_id, email, last_validated_at";

pub async fn get(pool: &SqlitePool) -> AppResult<Option<License>> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM license WHERE id = 1");
    Ok(sqlx::query_as::<_, License>(&sql).fetch_optional(pool).await?)
}

pub async fn start_trial(
    pool: &SqlitePool,
    email: &str,
    machine_id: &str,
    days: i64,
) -> AppResult<License> {
    let now = now_unix();
    let expires_at = now + days * 86_400;

    sqlx::query(
        "INSERT INTO license (id, tier, activated_at, expires_at, machine_id, email, last_validated_at)
         VALUES (1, 'TRIAL', ?1, ?2, ?3, ?4, ?1)
         ON CONFLICT(id) DO UPDATE SET
             tier              = 'TRIAL',
             activated_at      = excluded.activated_at,
             expires_at        = excluded.expires_at,
             email             = excluded.email,
             last_validated_at = excluded.last_validated_at",
    )
    .bind(now)
    .bind(expires_at)
    .bind(machine_id)
    .bind(email)
    .execute(pool)
    .await?;

    Ok(get(pool).await?.ok_or_else(|| crate::error::AppError::Other("license not found after insert".into()))?)
}

pub async fn activate(
    pool: &SqlitePool,
    license_key: &str,
    tier: &str,
    expires_at: i64,
    email: &str,
    machine_id: &str,
) -> AppResult<License> {
    let now = now_unix();
    sqlx::query(
        "INSERT INTO license (id, license_key, tier, activated_at, expires_at, machine_id, email, last_validated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?3)
         ON CONFLICT(id) DO UPDATE SET
             license_key       = excluded.license_key,
             tier              = excluded.tier,
             activated_at      = excluded.activated_at,
             expires_at        = excluded.expires_at,
             machine_id        = excluded.machine_id,
             email             = excluded.email,
             last_validated_at = excluded.last_validated_at",
    )
    .bind(license_key)
    .bind(tier)
    .bind(now)
    .bind(expires_at)
    .bind(machine_id)
    .bind(email)
    .execute(pool)
    .await?;

    Ok(get(pool).await?.ok_or_else(|| crate::error::AppError::Other("license not found after insert".into()))?)
}
