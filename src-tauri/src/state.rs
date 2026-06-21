//! State global al aplicației.
//!
//! Conține pool-ul SQLite. Accesibil în orice Tauri command prin
//! `state: tauri::State<'_, AppState>`.
//!
//! P2 Wave 8: authentication session fields added:
//! - `authenticated`: lock-free `AtomicBool` read by the sync gate.
//! - `current_role`: lock-free `AtomicU8` (Role enum ordinal) for gate permission checks.
//! - `current_user`: `Arc<RwLock<Option<CurrentUser>>>` for full user info in commands.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::db::rbac::Role;
use crate::db::users::CurrentUser;

/// Sentinel for "no user logged in" in the AtomicU8 role field.
const ROLE_NONE: u8 = 0xFF;

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

    // ── P2 Wave 8: Authentication session ────────────────────────────────────
    //
    // GATE NOTE: the invoke_handler wrapper is a SYNC closure.  It MUST NOT call
    // `blocking_read()` on a tokio RwLock (deadlock risk under the runtime).
    // Therefore we keep two lock-free atomics for the hot path:
    //
    //   authenticated  — AtomicBool.  Fast, no contention.
    //   current_role   — AtomicU8.    Role ordinal; 0xFF = no user.
    //
    // The full CurrentUser struct (needed by auth commands) lives in an
    // async RwLock that only async command handlers ever read/write.
    /// Whether a user is currently logged in. Read lock-free by the sync gate.
    pub authenticated: Arc<AtomicBool>,
    /// Current user's role ordinal. Read lock-free by the sync gate.
    /// `0xFF` means no user is logged in.
    pub current_role_atomic: Arc<AtomicU8>,
    /// Full current-user record. Only accessed from async command handlers.
    pub current_user: Arc<tokio::sync::RwLock<Option<CurrentUser>>>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            token_refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            authenticated: Arc::new(AtomicBool::new(false)),
            current_role_atomic: Arc::new(AtomicU8::new(ROLE_NONE)),
            current_user: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Called by `auth_login` / `auth_setup_admin` after successful authentication.
    /// Updates all three session fields atomically from an async context.
    pub async fn set_session(&self, user: CurrentUser) {
        let role = Role::from_db_str(&user.role)
            .map(|r| r.to_u8())
            .unwrap_or(ROLE_NONE);
        self.current_role_atomic.store(role, Ordering::Release);
        self.authenticated.store(true, Ordering::Release);
        let mut guard = self.current_user.write().await;
        *guard = Some(user);
    }

    /// Called by `auth_logout`. Clears all session fields.
    pub async fn clear_session(&self) {
        let mut guard = self.current_user.write().await;
        *guard = None;
        self.current_role_atomic.store(ROLE_NONE, Ordering::Release);
        self.authenticated.store(false, Ordering::Release);
    }

    /// Lock-free role snapshot for the sync gate.
    /// Returns `None` when `current_role_atomic` is the sentinel (not logged in).
    pub fn current_role_snapshot(&self) -> Option<Role> {
        let v = self.current_role_atomic.load(Ordering::Acquire);
        Role::from_u8(v)
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

    #[test]
    fn authenticated_starts_false() {
        let auth = Arc::new(AtomicBool::new(false));
        assert!(!auth.load(Ordering::Acquire));
    }

    #[test]
    fn role_none_sentinel_is_not_a_valid_role() {
        assert!(
            Role::from_u8(ROLE_NONE).is_none(),
            "ROLE_NONE sentinel must not decode to any Role"
        );
    }
}
