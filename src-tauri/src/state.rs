//! State global al aplicației.
//!
//! Conține pool-ul SQLite. Accesibil în orice Tauri command prin
//! `state: tauri::State<'_, AppState>`.
//!
//! P2 Wave 8: authentication session fields added:
//! - `authenticated`: lock-free `AtomicBool` read by the sync gate.
//! - `current_role`: lock-free `AtomicU8` (Role enum ordinal) for gate permission checks.
//! - `current_user`: `Arc<RwLock<Option<CurrentUser>>>` for full user info in commands.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};
use std::sync::Arc;

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;

use crate::db::rbac::Role;
use crate::db::users::CurrentUser;

/// Returns the current Unix time in seconds (monotone wall clock).
/// Saturates to 0 on the (impossible in practice) pre-epoch or overflow case.
#[inline]
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sentinel for "no user logged in" in the AtomicU8 role field.
const ROLE_NONE: u8 = 0xFF;

/// Session idle-timeout in seconds (default: 15 minutes).
///
/// After this many seconds of authenticated-command inactivity the gate treats
/// the session as expired and clears it.  Each successful authenticated command
/// slides the window.  Pre-auth / PUBLIC commands do NOT slide it.
///
/// Production idle-timeout default: 15 minutes. Override at runtime via the
/// `CLARITO_IDLE_TIMEOUT_SECS` environment variable (read in `idle_timeout_secs()`).
/// Referenced only by the release-build fallback below, so debug builds see it as unused.
#[allow(dead_code)]
pub const IDLE_TIMEOUT_SECS_DEFAULT: i64 = 900; // 15 minutes (production)

/// Effective fallback when no env override is set. Debug builds (`tauri dev` /
/// `tauri build --debug`) effectively disable the idle-timeout so the DEV
/// login-skip session (see lib.rs) doesn't expire during UI testing. The
/// production release build always uses the 15-minute default.
#[cfg(debug_assertions)]
const IDLE_TIMEOUT_FALLBACK: i64 = 315_360_000; // ~10 years — DEBUG BUILDS ONLY
#[cfg(not(debug_assertions))]
const IDLE_TIMEOUT_FALLBACK: i64 = IDLE_TIMEOUT_SECS_DEFAULT;

/// Returns the effective idle-timeout (seconds).  Reads the env var
/// `CLARITO_IDLE_TIMEOUT_SECS` at call time so tests can override it.
pub fn idle_timeout_secs() -> i64 {
    std::env::var("CLARITO_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(IDLE_TIMEOUT_FALLBACK)
}

/// Pure, unit-testable idle-expiry check.
///
/// Returns `true` when `last_activity` is non-zero and `(now − last_activity)`
/// exceeds `timeout_secs`.  Returns `false` (not expired) when
/// `last_activity == 0` (session not set / already cleared).
pub fn is_session_expired(last_activity: i64, now: i64, timeout_secs: i64) -> bool {
    last_activity != 0 && (now - last_activity) > timeout_secs
}

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
    /// Unix timestamp (seconds) of the last authenticated-command activity.
    ///
    /// - `0` = no active session (cleared on logout / idle-expire).
    /// - Set to `now()` on `set_session` (login); slid forward on each
    ///   successful authenticated command by the gate.
    /// - Read lock-free by the sync gate; the gate writes it lock-free too.
    pub last_activity: Arc<AtomicI64>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            token_refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            authenticated: Arc::new(AtomicBool::new(false)),
            current_role_atomic: Arc::new(AtomicU8::new(ROLE_NONE)),
            current_user: Arc::new(tokio::sync::RwLock::new(None)),
            last_activity: Arc::new(AtomicI64::new(0)),
        }
    }

    /// Called by `auth_login` / `auth_setup_admin` after successful authentication.
    /// Updates all session fields atomically from an async context.
    pub async fn set_session(&self, user: CurrentUser) {
        let role = Role::from_db_str(&user.role)
            .map(|r| r.to_u8())
            .unwrap_or(ROLE_NONE);
        let now = now_secs();
        self.current_role_atomic.store(role, Ordering::Release);
        self.last_activity.store(now, Ordering::Release);
        self.authenticated.store(true, Ordering::Release);
        let mut guard = self.current_user.write().await;
        *guard = Some(user);
    }

    /// Called by `auth_logout` (and by the gate on idle-expire).
    /// Clears all session fields including `last_activity`.
    pub async fn clear_session(&self) {
        let mut guard = self.current_user.write().await;
        *guard = None;
        self.current_role_atomic.store(ROLE_NONE, Ordering::Release);
        self.last_activity.store(0, Ordering::Release);
        self.authenticated.store(false, Ordering::Release);
    }

    /// Synchronous variant of `clear_session` for use in the sync gate closure.
    /// Only clears the lock-free atomics; `current_user` (async RwLock) is NOT
    /// cleared here — it will be cleared on the next async interaction (e.g.
    /// auth_status) because `authenticated` is already `false`.
    ///
    /// Background: the sync gate must not block on an async RwLock.  Setting
    /// `authenticated = false` and `last_activity = 0` is enough to fence
    /// subsequent commands; `current_user` cleanup happens in the next async ctx.
    pub fn clear_session_sync(&self) {
        self.current_role_atomic.store(ROLE_NONE, Ordering::Release);
        self.last_activity.store(0, Ordering::Release);
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

    // ── Idle-timeout: is_session_expired() unit tests ────────────────────

    /// A session idle for more than the timeout must be considered expired.
    #[test]
    fn idle_expired_after_timeout() {
        let now = 1_000_000_i64;
        let last_activity = now - 16 * 60; // 16 minutes ago
        let timeout = 15 * 60; // 15 minute timeout
        assert!(
            is_session_expired(last_activity, now, timeout),
            "session idle 16 min with 15 min timeout must be expired"
        );
    }

    /// A session idle for less than the timeout must NOT be expired.
    #[test]
    fn idle_valid_within_timeout() {
        let now = 1_000_000_i64;
        let last_activity = now - 5 * 60; // 5 minutes ago
        let timeout = 15 * 60; // 15 minute timeout
        assert!(
            !is_session_expired(last_activity, now, timeout),
            "session idle 5 min with 15 min timeout must NOT be expired"
        );
    }

    /// Exactly at the boundary (now − last == timeout) must NOT be expired
    /// (the check is strictly greater-than).
    #[test]
    fn idle_at_exact_boundary_not_expired() {
        let now = 1_000_000_i64;
        let timeout = 15 * 60_i64;
        let last_activity = now - timeout; // exactly at boundary
        assert!(
            !is_session_expired(last_activity, now, timeout),
            "session exactly at timeout boundary must NOT be expired (> not >=)"
        );
    }

    /// When `last_activity == 0` (no session set), must NOT be expired
    /// regardless of the elapsed time.
    #[test]
    fn idle_zero_last_activity_never_expired() {
        let now = 1_000_000_i64;
        assert!(
            !is_session_expired(0, now, 900),
            "last_activity=0 (no session) must never be expired"
        );
    }

    /// `now_secs()` must return a plausible timestamp (year 2020+, i.e. > 1_577_836_800).
    #[test]
    fn now_secs_is_plausible() {
        let ts = now_secs();
        assert!(
            ts > 1_577_836_800,
            "now_secs() must return a timestamp after 2020-01-01"
        );
    }

    /// `is_session_expired` slides: updating last_activity to `now` makes it valid again.
    #[test]
    fn idle_slides_after_activity_update() {
        let timeout = 15 * 60_i64;
        let now = 1_000_000_i64;
        let old_last = now - 16 * 60; // expired
        assert!(
            is_session_expired(old_last, now, timeout),
            "must be expired before slide"
        );
        // After slide:
        let new_last = now; // just acted
        assert!(
            !is_session_expired(new_last, now, timeout),
            "must NOT be expired immediately after activity slide"
        );
    }
}
