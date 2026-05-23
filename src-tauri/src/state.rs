//! State global al aplicației.
//!
//! Conține pool-ul SQLite. Accesibil în orice Tauri command prin
//! `state: tauri::State<'_, AppState>`.

use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}
