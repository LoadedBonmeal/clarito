//! Audit log helpers.
//!
//! Logging is best-effort: failures are logged at warn level and never
//! propagate to the caller. The audit log must NEVER block a user action.

use crate::db::models::new_id;
use crate::error::AppResult;
use sqlx::SqlitePool;

/// Persist a user-initiated action into the audit_log table.
/// Errors are swallowed (logged at warn level) so an unwritable audit_log
/// never blocks the actual operation.
pub async fn log_user_action(
    pool: &SqlitePool,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    metadata: Option<&str>,
) -> AppResult<()> {
    let id = new_id();
    let result = sqlx::query(
        "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())",
    )
    .bind(&id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(metadata)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            action = %action,
            entity_type = %entity_type,
            entity_id = %entity_id,
            error = ?e,
            "Failed to write audit_log entry (non-fatal)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE audit_log (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn log_user_action_inserts_row() {
        let pool = setup_pool().await;
        log_user_action(
            &pool,
            "test_action",
            "invoice",
            "test-id-123",
            Some("metadata-x"),
        )
        .await
        .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE entity_id = ?1")
            .bind("test-id-123")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn log_user_action_silent_on_db_error() {
        // Pool with no audit_log table — INSERT will fail; helper must NOT panic
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();
        // Call should return Ok despite the underlying error
        let result = log_user_action(&pool, "x", "y", "z", None).await;
        assert!(result.is_ok());
    }
}
