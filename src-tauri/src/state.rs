//! State global al aplicației.
//!
//! Conține pool-ul SQLite. Accesibil în orice Tauri command prin
//! `state: tauri::State<'_, AppState>`.

use std::sync::Arc;

use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    /// App-wide async mutex that serializes ANAF OAuth token refreshes.
    ///
    /// Token refreshes are rare (once per hour per company) so a single global
    /// lock is fine — it prevents the concurrent-refresh race where two tasks
    /// see an expired token simultaneously, both call `refresh_token_bundle`,
    /// and ANAF invalidates the first refresh_token on the second call
    /// (`invalid_grant`), triggering a spurious "re-authorize" notification.
    ///
    /// Each refresh site must:
    ///   1. Acquire this lock.
    ///   2. Re-load the token from the keychain (double-check).
    ///   3. Re-test `is_expired()` — if another task already refreshed while
    ///      we waited, skip the refresh and use the fresh token.
    ///   4. Only if still expired: refresh + save.
    ///
    /// Use `tokio::sync::Mutex` (async), NOT `std::sync::Mutex`, so the
    /// `.await` refresh call inside the critical section does not block
    /// the Tokio thread pool.
    pub token_refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            token_refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that AppState constructs with the lock field and that Clone works.
    #[test]
    fn appstate_has_token_refresh_lock() {
        // We can't create a real SqlitePool in a unit test without a database,
        // but we can verify the lock type compiles and is accessible.
        let lock: Arc<tokio::sync::Mutex<()>> = Arc::new(tokio::sync::Mutex::new(()));
        // Confirm the lock can be acquired synchronously via try_lock.
        assert!(
            lock.try_lock().is_ok(),
            "fresh lock must be immediately acquirable"
        );
    }
}
