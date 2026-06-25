//! Tip unic de eroare pentru tot backend-ul.
//!
//! Toate funcțiile fallible returnează `AppResult<T>`. Conversiile din erorile
//! upstream (sqlx, io, json) sunt automate via `#[from]`.
//!
//! `AppError` implementează `Serialize` ca să poată traversa boundary-ul
//! Rust → JavaScript prin Tauri commands. Forma JSON e:
//! `{ "kind": "Database", "message": "..." }`.

use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

// CLEAN-02: the error taxonomy is intentionally complete — some variants are constructed only on
// rarely-hit paths (or kept for forthcoming callers), so not every variant has a live constructor at
// all times. `allow(dead_code)` keeps the full, self-documenting set without clippy churn as call
// sites come and go.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Înregistrare inexistentă")]
    NotFound,

    #[error("Date invalide: {0}")]
    Validation(String),

    #[error("Eroare bază de date: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migrație eșuată: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Eroare I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serializare JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tauri: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("XML: {0}")]
    Xml(String),

    #[error("PDF: {0}")]
    Pdf(String),

    #[error("Excel: {0}")]
    Xlsx(String),

    #[error("Arhivă: {0}")]
    Archive(String),

    #[error("{0}")]
    Other(String),

    /// Session expired due to inactivity — distinct from a normal permission
    /// denial so the frontend can redirect to login with a "sesiune expirată" notice.
    #[error("{0}")]
    SessionExpired(String),
}

impl From<rust_xlsxwriter::XlsxError> for AppError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        AppError::Xlsx(e.to_string())
    }
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::NotFound => "NotFound",
            AppError::Validation(_) => "Validation",
            AppError::Database(_) => "Database",
            AppError::Migration(_) => "Migration",
            AppError::Io(_) => "Io",
            AppError::Json(_) => "Json",
            AppError::Tauri(_) => "Tauri",
            AppError::Conflict(_) => "Conflict",
            AppError::Xml(_) => "Xml",
            AppError::Pdf(_) => "Pdf",
            AppError::Xlsx(_) => "Xlsx",
            AppError::Archive(_) => "Archive",
            AppError::Other(_) => "Other",
            AppError::SessionExpired(_) => "SessionExpired",
        }
    }

    /// True for infrastructure errors whose raw detail must NOT reach the client
    /// (sqlx/migration text can leak the DB schema; io can leak filesystem paths).
    fn is_internal(&self) -> bool {
        matches!(
            self,
            AppError::Database(_)
                | AppError::Migration(_)
                | AppError::Io(_)
                | AppError::Json(_)
                | AppError::Tauri(_)
        )
    }

    /// Client-safe message: a generic string for internal errors (the real detail
    /// is logged server-side), the real message for domain / user-facing errors
    /// (Validation, NotFound, Conflict, Xml, Pdf, …) which are meant to be shown.
    fn client_message(&self) -> String {
        match self {
            AppError::Database(_) | AppError::Migration(_) => {
                "Eroare la accesarea bazei de date.".to_string()
            }
            AppError::Io(_) => "Eroare de intrare/ieșire.".to_string(),
            AppError::Json(_) => "Eroare la procesarea datelor.".to_string(),
            AppError::Tauri(_) => "Eroare internă a aplicației.".to_string(),
            other => other.to_string(),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // Log the full internal detail server-side BEFORE masking it for the client,
        // so diagnostics are preserved without leaking schema/paths to the frontend.
        if self.is_internal() {
            tracing::error!(kind = self.kind(), detail = %self, "internal error (masked for client)");
        }
        let mut s = ser.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.client_message())?;
        s.end()
    }
}
