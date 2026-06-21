//! User management + Argon2id password hashing.
//!
//! - `needs_setup` — true when the users table is empty (first launch).
//! - `setup_admin` — create the very first admin; refuses if any user exists.
//! - `login` — verify password, enforce lockout, return `CurrentUser`.
//! - `list_users`, `create_user`, `update_user`, `reset_password` — admin operations.
//!
//! Password policy: Argon2id (default = v0x13, m=19456, t=2, p=1) per OWASP floor.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::models::new_id;
use crate::error::{AppError, AppResult};

// ─── Lockout policy ──────────────────────────────────────────────────────────

const MAX_ATTEMPTS: i64 = 5;
/// 15 minutes in seconds.
const LOCKOUT_SECS: i64 = 15 * 60;

// ─── Types ────────────────────────────────────────────────────────────────────

/// The subset of user data stored in AppState session (cheap to clone).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: String,
    pub username: String,
    pub role: String,
}

/// Full user row returned by `list_users`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub role: String,
    pub is_active: bool,
    pub failed_attempts: i64,
    pub locked_until: Option<i64>,
    pub created_at: i64,
    pub last_login: Option<i64>,
}

// ─── Password helpers ─────────────────────────────────────────────────────────

/// Hash a plaintext password with Argon2id. Returns the PHC string.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default(); // Argon2id v0x13 m=19456 t=2 p=1
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Other(format!("password hash failed: {e}")))
}

/// Verify a plaintext password against a stored PHC string.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// ─── Setup check ──────────────────────────────────────────────────────────────

/// Returns `true` when there are no users in the DB (first-launch setup).
pub async fn needs_setup(pool: &SqlitePool) -> AppResult<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count == 0)
}

// ─── Setup admin ─────────────────────────────────────────────────────────────

/// Create the very first admin account.
/// Errors if any user already exists (prevent privilege escalation).
pub async fn setup_admin(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> AppResult<CurrentUser> {
    if !needs_setup(pool).await? {
        return Err(AppError::Conflict(
            "Un utilizator admin există deja. Folosiți pagina de administrare utilizatori."
                .to_string(),
        ));
    }
    let username = username.trim().to_lowercase();
    if username.is_empty() {
        return Err(AppError::Validation(
            "Numele de utilizator este obligatoriu.".to_string(),
        ));
    }
    if password.len() < 6 {
        return Err(AppError::Validation(
            "Parola trebuie să aibă minim 6 caractere.".to_string(),
        ));
    }
    let id = new_id();
    let hash = hash_password(password)?;
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, is_active, failed_attempts, created_at)
         VALUES (?1, ?2, ?3, 'admin', 1, 0, ?4)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&hash)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(CurrentUser {
        id,
        username,
        role: "admin".to_string(),
    })
}

// ─── Login ────────────────────────────────────────────────────────────────────

/// Attempt a login.
/// On success: resets `failed_attempts`, sets `last_login`, returns `CurrentUser`.
/// On wrong password: increments `failed_attempts`; after `MAX_ATTEMPTS` sets `locked_until`.
/// Errors if user not found, inactive, or locked.
pub async fn login(pool: &SqlitePool, username: &str, password: &str) -> AppResult<CurrentUser> {
    let username_lower = username.trim().to_lowercase();
    let now = chrono::Utc::now().timestamp();

    use sqlx::Row as _;

    let maybe_row = sqlx::query(
        "SELECT id, password_hash, role, is_active, failed_attempts, locked_until
         FROM users WHERE username = ?1",
    )
    .bind(&username_lower)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    let Some(row) = maybe_row else {
        // Don't reveal whether user exists.
        return Err(AppError::Validation(
            "Nume de utilizator sau parolă incorectă.".to_string(),
        ));
    };

    let id: String = row.try_get("id").unwrap_or_default();
    let password_hash: String = row.try_get("password_hash").unwrap_or_default();
    let role: String = row.try_get("role").unwrap_or_default();
    let is_active: i64 = row.try_get("is_active").unwrap_or(1);
    let failed_attempts: i64 = row.try_get("failed_attempts").unwrap_or(0);
    let locked_until: Option<i64> = row.try_get("locked_until").unwrap_or(None);

    if is_active == 0 {
        return Err(AppError::Validation(
            "Contul este dezactivat. Contactați administratorul.".to_string(),
        ));
    }

    if let Some(lock_ts) = locked_until {
        if lock_ts > now {
            let remaining = (lock_ts - now + 59) / 60;
            return Err(AppError::Validation(format!(
                "Contul este blocat temporar. Încercați din nou în {remaining} minut(e)."
            )));
        }
    }

    // Verify password (slow — Argon2id).
    if !verify_password(password, &password_hash) {
        // Increment failed_attempts; lock if threshold hit.
        let new_attempts = failed_attempts + 1;
        let new_locked: Option<i64> = if new_attempts >= MAX_ATTEMPTS {
            Some(now + LOCKOUT_SECS)
        } else {
            None
        };
        sqlx::query("UPDATE users SET failed_attempts = ?1, locked_until = ?2 WHERE id = ?3")
            .bind(new_attempts)
            .bind(new_locked)
            .bind(&id)
            .execute(pool)
            .await?;
        return Err(AppError::Validation(
            "Nume de utilizator sau parolă incorectă.".to_string(),
        ));
    }

    // Success: reset failed_attempts, clear lock, set last_login.
    sqlx::query(
        "UPDATE users SET failed_attempts = 0, locked_until = NULL, last_login = ?1 WHERE id = ?2",
    )
    .bind(now)
    .bind(&id)
    .execute(pool)
    .await?;

    Ok(CurrentUser {
        id,
        username: username_lower,
        role,
    })
}

// ─── User management ─────────────────────────────────────────────────────────

pub async fn list_users(pool: &SqlitePool) -> AppResult<Vec<UserRow>> {
    let rows = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, role, is_active, failed_attempts, locked_until, created_at, last_login
         FROM users ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserInput {
    pub username: String,
    pub password: String,
    pub role: String,
}

pub async fn create_user(pool: &SqlitePool, input: CreateUserInput) -> AppResult<UserRow> {
    let username = input.username.trim().to_lowercase();
    if username.is_empty() {
        return Err(AppError::Validation(
            "Numele de utilizator este obligatoriu.".to_string(),
        ));
    }
    if input.password.len() < 6 {
        return Err(AppError::Validation(
            "Parola trebuie să aibă minim 6 caractere.".to_string(),
        ));
    }
    let valid_roles = ["admin", "contabil", "operator", "viewer"];
    if !valid_roles.contains(&input.role.as_str()) {
        return Err(AppError::Validation(format!(
            "Rol invalid: '{}'. Roluri acceptate: admin, contabil, operator, viewer.",
            input.role
        )));
    }
    let id = new_id();
    let hash = hash_password(&input.password)?;
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, is_active, failed_attempts, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, 0, ?5)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&hash)
    .bind(&input.role)
    .bind(now)
    .execute(pool)
    .await?;

    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, role, is_active, failed_attempts, locked_until, created_at, last_login
         FROM users WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserInput {
    pub role: Option<String>,
    pub is_active: Option<bool>,
}

pub async fn update_user(
    pool: &SqlitePool,
    user_id: &str,
    input: UpdateUserInput,
) -> AppResult<UserRow> {
    // Validate role if provided.
    if let Some(ref role) = input.role {
        let valid_roles = ["admin", "contabil", "operator", "viewer"];
        if !valid_roles.contains(&role.as_str()) {
            return Err(AppError::Validation(format!("Rol invalid: '{role}'.")));
        }
    }
    // Apply updates.
    if let Some(ref role) = input.role {
        sqlx::query("UPDATE users SET role = ?1 WHERE id = ?2")
            .bind(role)
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    if let Some(is_active) = input.is_active {
        sqlx::query("UPDATE users SET is_active = ?1 WHERE id = ?2")
            .bind(is_active as i64)
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, role, is_active, failed_attempts, locked_until, created_at, last_login
         FROM users WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn reset_password(pool: &SqlitePool, user_id: &str, new_password: &str) -> AppResult<()> {
    if new_password.len() < 6 {
        return Err(AppError::Validation(
            "Parola trebuie să aibă minim 6 caractere.".to_string(),
        ));
    }
    let hash = hash_password(new_password)?;
    let result =
        sqlx::query("UPDATE users SET password_hash = ?1, failed_attempts = 0, locked_until = NULL WHERE id = ?2")
            .bind(&hash)
            .bind(user_id)
            .execute(pool)
            .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn make_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();
        // Minimal schema (mirrors migration 0070).
        sqlx::query(
            "CREATE TABLE users (
                id            TEXT PRIMARY KEY,
                username      TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role          TEXT NOT NULL,
                is_active     INTEGER NOT NULL DEFAULT 1,
                failed_attempts INTEGER NOT NULL DEFAULT 0,
                locked_until  INTEGER,
                created_at    INTEGER NOT NULL,
                last_login    INTEGER
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    // ── Hash / verify ───────────────────────────────────────────────────

    #[test]
    fn hash_verify_roundtrip() {
        let hash = hash_password("correct-horse-battery").unwrap();
        assert!(verify_password("correct-horse-battery", &hash));
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("secret").unwrap();
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn hash_is_phc_string() {
        let hash = hash_password("test1234").unwrap();
        // PHC string starts with "$argon2id$"
        assert!(
            hash.starts_with("$argon2id$"),
            "Password hash must be a PHC string, got: {hash}"
        );
    }

    #[test]
    fn two_hashes_of_same_password_differ() {
        let h1 = hash_password("password").unwrap();
        let h2 = hash_password("password").unwrap();
        assert_ne!(h1, h2, "Salts must differ across calls");
    }

    // ── needs_setup / setup_admin ────────────────────────────────────────

    #[tokio::test]
    async fn needs_setup_true_when_empty() {
        let pool = make_pool().await;
        assert!(needs_setup(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn needs_setup_false_after_setup_admin() {
        let pool = make_pool().await;
        setup_admin(&pool, "admin", "password123").await.unwrap();
        assert!(!needs_setup(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn setup_admin_refuses_when_user_exists() {
        let pool = make_pool().await;
        setup_admin(&pool, "admin", "password123").await.unwrap();
        let result = setup_admin(&pool, "admin2", "password456").await;
        assert!(result.is_err(), "setup_admin must refuse when users exist");
    }

    // ── login ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn login_success_resets_failed_attempts_and_returns_role() {
        let pool = make_pool().await;
        setup_admin(&pool, "alice", "pass1234").await.unwrap();
        // Simulate a previous failed attempt.
        sqlx::query("UPDATE users SET failed_attempts = 2")
            .execute(&pool)
            .await
            .unwrap();
        let user = login(&pool, "alice", "pass1234").await.unwrap();
        assert_eq!(user.role, "admin");
        let attempts: i64 =
            sqlx::query_scalar("SELECT failed_attempts FROM users WHERE username = 'alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, 0, "failed_attempts must reset on success");
    }

    #[tokio::test]
    async fn wrong_password_increments_failed_attempts() {
        let pool = make_pool().await;
        setup_admin(&pool, "bob", "pass1234").await.unwrap();
        let _ = login(&pool, "bob", "wrong").await;
        let attempts: i64 =
            sqlx::query_scalar("SELECT failed_attempts FROM users WHERE username = 'bob'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn after_5_fails_locked_until_is_set_and_login_rejected() {
        let pool = make_pool().await;
        setup_admin(&pool, "charlie", "pass1234").await.unwrap();
        // 5 wrong attempts.
        for _ in 0..5 {
            let _ = login(&pool, "charlie", "wrong").await;
        }
        let locked: Option<i64> =
            sqlx::query_scalar("SELECT locked_until FROM users WHERE username = 'charlie'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            locked.is_some() && locked.unwrap() > chrono::Utc::now().timestamp(),
            "locked_until must be set in the future after 5 fails"
        );
        // Even the correct password is rejected while locked.
        let result = login(&pool, "charlie", "pass1234").await;
        assert!(
            result.is_err(),
            "Locked account must reject even correct password"
        );
    }

    #[tokio::test]
    async fn inactive_user_rejected() {
        let pool = make_pool().await;
        setup_admin(&pool, "diana", "pass1234").await.unwrap();
        sqlx::query("UPDATE users SET is_active = 0 WHERE username = 'diana'")
            .execute(&pool)
            .await
            .unwrap();
        let result = login(&pool, "diana", "pass1234").await;
        assert!(result.is_err(), "Inactive user must be rejected");
    }
}
