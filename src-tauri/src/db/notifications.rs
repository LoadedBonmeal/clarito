//! Notificări in-app (persistente). Notificările OS native sunt declanșate
//! separat de modulul `notifications::os`.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub data: Option<String>,
    pub is_read: bool,
    pub read_at: Option<i64>,
    pub os_notification_shown: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNotificationInput {
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub data: Option<String>,
}

pub async fn list(pool: &SqlitePool, only_unread: bool) -> AppResult<Vec<Notification>> {
    // ?1 only_unread flag: when 1, filter is_read = 0; when 0, all rows pass.
    Ok(sqlx::query_as::<_, Notification>(
        "SELECT id, notification_type, title, body, data, is_read, read_at, \
         os_notification_shown, created_at \
         FROM notifications \
         WHERE (NOT ?1 OR is_read = 0) \
         ORDER BY created_at DESC LIMIT 200",
    )
    .bind(only_unread as i64)
    .fetch_all(pool)
    .await?)
}

pub async fn count_unread(pool: &SqlitePool) -> AppResult<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notifications WHERE is_read = 0",
    )
    .fetch_one(pool)
    .await?)
}

pub async fn create(pool: &SqlitePool, input: CreateNotificationInput) -> AppResult<Notification> {
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO notifications (id, notification_type, title, body, data, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&id)
    .bind(&input.notification_type)
    .bind(&input.title)
    .bind(&input.body)
    .bind(&input.data)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(sqlx::query_as::<_, Notification>(
        "SELECT id, notification_type, title, body, data, is_read, read_at, \
         os_notification_shown, created_at \
         FROM notifications WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(pool)
    .await?)
}

pub async fn mark_read(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let now = now_unix();
    sqlx::query("UPDATE notifications SET is_read = 1, read_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_all_read(pool: &SqlitePool) -> AppResult<()> {
    let now = now_unix();
    sqlx::query("UPDATE notifications SET is_read = 1, read_at = ?1 WHERE is_read = 0")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_notification(pool: &SqlitePool, id: &str) -> AppResult<()> {
    use crate::error::AppError;
    let rows = sqlx::query("DELETE FROM notifications WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_all_read(pool: &SqlitePool) -> AppResult<u64> {
    let rows = sqlx::query("DELETE FROM notifications WHERE is_read = 1")
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows)
}
