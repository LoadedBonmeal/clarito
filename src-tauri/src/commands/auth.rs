//! Authentication & user management Tauri commands.
//!
//! All auth flow commands (auth_status, auth_setup_admin, auth_login, auth_logout)
//! are in PUBLIC_COMMANDS and bypass the gate — they ARE the pre-auth flow.
//!
//! User management commands (list_users, create_user, update_user, reset_password)
//! are behind the ManageUsers permission gate (admin-only).

use serde::Serialize;
use tauri::State;

use crate::db::audit::log_user_action_attributed;
use crate::db::users::{self, CreateUserInput, CurrentUser, UpdateUserInput, UserRow};
use crate::error::AppResult;
use crate::state::AppState;

// ─── Auth status ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub needs_setup: bool,
    pub authenticated: bool,
    pub current_user: Option<CurrentUser>,
}

/// Called on app start to determine which screen to show:
/// - `needs_setup = true` → show SetupAdmin screen
/// - `needs_setup = false, authenticated = false` → show Login screen
/// - `authenticated = true` → show main app
///
/// PUBLIC command — called before any authentication.
#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> AppResult<AuthStatus> {
    let needs_setup = users::needs_setup(&state.db).await?;
    let authenticated = state
        .authenticated
        .load(std::sync::atomic::Ordering::Acquire);
    let current_user = if authenticated {
        state.current_user.read().await.clone()
    } else {
        None
    };
    Ok(AuthStatus {
        needs_setup,
        authenticated,
        current_user,
    })
}

// ─── Setup admin ─────────────────────────────────────────────────────────────

/// Create the first admin account (only allowed when no users exist).
/// Immediately establishes a session for the creator.
///
/// PUBLIC command — called from the SetupAdmin screen before any users exist.
#[tauri::command]
pub async fn auth_setup_admin(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> AppResult<CurrentUser> {
    let user = users::setup_admin(&state.db, &username, &password).await?;
    state.set_session(user.clone()).await;
    log_user_action_attributed(
        &state.db,
        "SETUP_ADMIN",
        "user",
        &user.id,
        None,
        Some(&format!("First admin created: {}", user.username)),
        Some(&user.id),
        Some(&user.username),
    )
    .await?;
    Ok(user)
}

// ─── Login ────────────────────────────────────────────────────────────────────

/// Authenticate with username + password.
/// On success: establishes a session and returns the current user.
/// On failure: returns a Validation error (lockout message if locked).
///
/// PUBLIC command — called from the Login screen before authentication.
#[tauri::command]
pub async fn auth_login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> AppResult<CurrentUser> {
    let user = users::login(&state.db, &username, &password).await?;
    state.set_session(user.clone()).await;
    log_user_action_attributed(
        &state.db,
        "LOGIN",
        "user",
        &user.id,
        None,
        None,
        Some(&user.id),
        Some(&user.username),
    )
    .await?;
    Ok(user)
}

// ─── Logout ───────────────────────────────────────────────────────────────────

/// Clear the current session.
///
/// PUBLIC command — so that logout always works even if the gate is stricter.
#[tauri::command]
pub async fn auth_logout(state: State<'_, AppState>) -> AppResult<()> {
    // Capture user info before clearing session (for audit log).
    let user_snapshot = state.current_user.read().await.clone();
    state.clear_session().await;
    if let Some(user) = user_snapshot {
        log_user_action_attributed(
            &state.db,
            "LOGOUT",
            "user",
            &user.id,
            None,
            None,
            Some(&user.id),
            Some(&user.username),
        )
        .await?;
    }
    Ok(())
}

// ─── User management (admin-only via gate) ────────────────────────────────────

/// List all users. Gate: ManageUsers (admin only).
#[tauri::command]
pub async fn list_users(state: State<'_, AppState>) -> AppResult<Vec<UserRow>> {
    users::list_users(&state.db).await
}

/// Create a new user. Gate: ManageUsers (admin only).
#[tauri::command]
pub async fn create_user(state: State<'_, AppState>, input: CreateUserInput) -> AppResult<UserRow> {
    let actor = current_actor(&state).await;
    let row = users::create_user(&state.db, input).await?;
    log_user_action_attributed(
        &state.db,
        "USER_CREATE",
        "user",
        &row.id,
        None,
        Some(&format!("Created user: {}", row.username)),
        actor.as_ref().map(|u| u.id.as_str()),
        actor.as_ref().map(|u| u.username.as_str()),
    )
    .await?;
    Ok(row)
}

/// Update a user's role or active status. Gate: ManageUsers (admin only).
#[tauri::command]
pub async fn update_user(
    state: State<'_, AppState>,
    user_id: String,
    input: UpdateUserInput,
) -> AppResult<UserRow> {
    let actor = current_actor(&state).await;
    let row = users::update_user(&state.db, &user_id, input).await?;
    log_user_action_attributed(
        &state.db,
        "USER_UPDATE",
        "user",
        &row.id,
        None,
        None,
        actor.as_ref().map(|u| u.id.as_str()),
        actor.as_ref().map(|u| u.username.as_str()),
    )
    .await?;
    Ok(row)
}

/// Reset a user's password. Gate: ManageUsers (admin only).
#[tauri::command]
pub async fn reset_password(
    state: State<'_, AppState>,
    user_id: String,
    new_password: String,
) -> AppResult<()> {
    let actor = current_actor(&state).await;
    users::reset_password(&state.db, &user_id, &new_password).await?;
    log_user_action_attributed(
        &state.db,
        "PASSWORD_RESET",
        "user",
        &user_id,
        None,
        None,
        actor.as_ref().map(|u| u.id.as_str()),
        actor.as_ref().map(|u| u.username.as_str()),
    )
    .await?;
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Snapshot of the current user for audit log attribution.
async fn current_actor(state: &State<'_, AppState>) -> Option<CurrentUser> {
    state.current_user.read().await.clone()
}
