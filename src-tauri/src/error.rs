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
}

impl From<rust_xlsxwriter::XlsxError> for AppError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        AppError::Xlsx(e.to_string())
    }
}

impl AppError {
    /// Determină dacă eroarea reprezintă "nimic găsit" (vs eroare reală).
    #[allow(dead_code)]
    pub fn is_not_found(&self) -> bool {
        matches!(self, AppError::NotFound)
            || matches!(self, AppError::Database(sqlx::Error::RowNotFound))
    }

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
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}
