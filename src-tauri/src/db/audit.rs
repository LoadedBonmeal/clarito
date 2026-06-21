//! Audit log helpers.
//!
//! Logging is best-effort: failures are logged at warn level and never
//! propagate to the caller. The audit log must NEVER block a user action.
//!
//! P2 Wave 8: `log_user_action_attributed` adds user_id + user_label columns
//! (migration 0071). The original `log_user_action` is preserved unchanged so
//! all existing call sites compile without modification.

use crate::db::models::new_id;
use crate::error::AppResult;
use sqlx::SqlitePool;

/// Persist a user-initiated action into the audit_log table.
/// Errors are swallowed (logged at warn level) so an unwritable audit_log
/// never blocks the actual operation.
///
/// `company_id` scopes the row to its owning company (SEC-07/08) so the activity log
/// can't leak one tenant's activity to another. Pass `Some(company_id)` for any
/// company-specific event; `None` only for genuinely global/system events.
pub async fn log_user_action(
    pool: &SqlitePool,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    company_id: Option<&str>,
    metadata: Option<&str>,
) -> AppResult<()> {
    let id = new_id();
    let result = sqlx::query(
        "INSERT INTO audit_log (id, action, entity_type, entity_id, company_id, metadata, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch())",
    )
    .bind(&id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(company_id)
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

/// P2 Wave 8 variant: same as `log_user_action` but also records which
/// application user performed the action (user_id + user_label columns added
/// by migration 0071).
///
/// Call this from auth commands (LOGIN/LOGOUT/SETUP_ADMIN) and from any
/// sensitive mutation where `CurrentUser` is available in the handler.
/// The original `log_user_action` remains for call sites that have no
/// session context (background jobs, non-session paths).
#[allow(clippy::too_many_arguments)]
pub async fn log_user_action_attributed(
    pool: &SqlitePool,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    company_id: Option<&str>,
    metadata: Option<&str>,
    user_id: Option<&str>,
    user_label: Option<&str>,
) -> AppResult<()> {
    let id = new_id();
    let result = sqlx::query(
        "INSERT INTO audit_log \
         (id, action, entity_type, entity_id, company_id, metadata, user_id, user_label, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
    )
    .bind(&id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(company_id)
    .bind(metadata)
    .bind(user_id)
    .bind(user_label)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            action = %action,
            entity_type = %entity_type,
            entity_id = %entity_id,
            error = ?e,
            "Failed to write attributed audit_log entry (non-fatal)"
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
                company_id TEXT,
                metadata TEXT,
                user_id TEXT,
                user_label TEXT,
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
            Some("comp-1"),
            Some("metadata-x"),
        )
        .await
        .unwrap();
        let company_id: Option<String> =
            sqlx::query_scalar("SELECT company_id FROM audit_log WHERE entity_id = ?1")
                .bind("test-id-123")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(company_id.as_deref(), Some("comp-1"));
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
        let result = log_user_action(&pool, "x", "y", "z", None, None).await;
        assert!(result.is_ok());
    }
}
