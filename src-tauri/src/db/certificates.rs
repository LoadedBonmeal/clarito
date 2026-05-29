//! Certificate ANAF (metadata).
//!
//! Token-urile OAuth efective (access_token, refresh_token) NU sunt stocate
//! aici — sunt în OS Keychain. Aici păstrăm doar `keychain_ref` (cheia sub
//! care găsim token-ul) și termenele de expirare.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::now_unix;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Certificate {
    pub id: String,
    pub company_id: String,
    pub keychain_ref: String,

    pub issued_at: i64,
    pub expires_at: i64,
    pub refreshable_until: i64,

    pub is_active: bool,
    pub last_refreshed_at: Option<i64>,
    pub last_used_at: Option<i64>,

    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn list_for_company(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Certificate>> {
    Ok(sqlx::query_as::<_, Certificate>(
        "SELECT id, company_id, keychain_ref, issued_at, expires_at, \
         refreshable_until, is_active, last_refreshed_at, last_used_at, created_at, updated_at \
         FROM certificates WHERE company_id = ?1 ORDER BY created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Returnează certificatele care expiră în următoarele `days` zile și sunt
/// încă active. Folosit de background task pentru notificări.
pub async fn list_expiring(pool: &SqlitePool, days: i64) -> AppResult<Vec<Certificate>> {
    let cutoff = now_unix() + days * 86_400;
    Ok(sqlx::query_as::<_, Certificate>(
        "SELECT id, company_id, keychain_ref, issued_at, expires_at, \
         refreshable_until, is_active, last_refreshed_at, last_used_at, created_at, updated_at \
         FROM certificates WHERE is_active = 1 AND expires_at < ?1 \
         ORDER BY expires_at",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?)
}
