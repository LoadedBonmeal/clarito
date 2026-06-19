//! Istoricul depunerilor de declarații fiscale.
//!
//! Fiecare export reușit înregistrează un rând (best-effort — erorile sunt înghițite la apelant).
//! Lista e scopată pe firmă (`company_id`), ordonată desc după `filed_at`.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::models::{new_id, now_unix};
use crate::error::AppResult;

// ─── Tipuri publice ───────────────────────────────────────────────────────────

/// Rândul de istoric al unei depuneri, serializat camelCase spre frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Filing {
    pub id: String,
    pub company_id: String,
    /// Tipul declarației: "D300", "D390", "D394", "D112", "D205", "D207", "SAFT", "BILANT".
    pub kind: String,
    /// Perioada: "YYYY-MM" pentru lunar, "YYYY" pentru anual.
    pub period: String,
    /// `true` → declarație rectificativă.
    pub is_rectificative: bool,
    /// Calea pe disc unde a fost scris XML-ul (None dacă nu e disponibilă).
    pub file_path: Option<String>,
    /// Starea curentă: "EXPORTED" | "SUBMITTED" | "ACCEPTED" | "REJECTED".
    pub anaf_status: String,
    /// Timestamp Unix (secunde) al momentului exportului.
    pub filed_at: i64,
}

/// Datele necesare pentru a înregistra o nouă depunere.
#[derive(Debug, Deserialize)]
pub struct FilingInput {
    pub company_id: String,
    pub kind: String,
    pub period: String,
    pub is_rectificative: bool,
    pub file_path: Option<String>,
}

// ─── Operații DB ─────────────────────────────────────────────────────────────

/// Înregistrează o nouă depunere cu status implicit "EXPORTED".
/// Apelantul trebuie să înghită erorile: `let _ = record(...).await;`
pub async fn record(pool: &SqlitePool, input: FilingInput) -> AppResult<()> {
    let id = new_id();
    let filed_at = now_unix();
    let is_rect_i = i64::from(input.is_rectificative);

    sqlx::query(
        "INSERT INTO declaration_filings \
         (id, company_id, kind, period, is_rectificative, file_path, anaf_status, filed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'EXPORTED', ?7)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.kind)
    .bind(&input.period)
    .bind(is_rect_i)
    .bind(&input.file_path)
    .bind(filed_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Listează depunerile pentru o firmă, cele mai recente primele.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Filing>> {
    let rows = sqlx::query_as::<_, FilingRow>(
        "SELECT id, company_id, kind, period, is_rectificative, file_path, anaf_status, filed_at \
         FROM declaration_filings \
         WHERE company_id = ?1 \
         ORDER BY filed_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Filing::from).collect())
}

/// Șterge o depunere după id, cu verificare de firmă (company-scoped).
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM declaration_filings WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ─── Row intern (FromRow intermediar pentru is_rectificative: i64 → bool) ────

#[derive(sqlx::FromRow)]
struct FilingRow {
    id: String,
    company_id: String,
    kind: String,
    period: String,
    is_rectificative: i64,
    file_path: Option<String>,
    anaf_status: String,
    filed_at: i64,
}

impl From<FilingRow> for Filing {
    fn from(r: FilingRow) -> Self {
        Filing {
            id: r.id,
            company_id: r.company_id,
            kind: r.kind,
            period: r.period,
            is_rectificative: r.is_rectificative != 0,
            file_path: r.file_path,
            anaf_status: r.anaf_status,
            filed_at: r.filed_at,
        }
    }
}

// ─── Teste ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE declaration_filings ( \
              id TEXT PRIMARY KEY NOT NULL, \
              company_id TEXT NOT NULL, \
              kind TEXT NOT NULL, \
              period TEXT NOT NULL, \
              is_rectificative INTEGER NOT NULL DEFAULT 0, \
              file_path TEXT, \
              anaf_status TEXT NOT NULL DEFAULT 'EXPORTED', \
              filed_at INTEGER NOT NULL \
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_record_list_delete_scoped() {
        let pool = setup().await;

        // Firmă A: 2 depuneri
        record(
            &pool,
            FilingInput {
                company_id: "co-A".into(),
                kind: "D300".into(),
                period: "2026-05".into(),
                is_rectificative: false,
                file_path: Some("/tmp/d300.xml".into()),
            },
        )
        .await
        .unwrap();

        // Mică pauză (1 secundă) între înregistrări ca filed_at să difere.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        record(
            &pool,
            FilingInput {
                company_id: "co-A".into(),
                kind: "D112".into(),
                period: "2026-04".into(),
                is_rectificative: true,
                file_path: None,
            },
        )
        .await
        .unwrap();

        // Firmă B: 1 depunere
        record(
            &pool,
            FilingInput {
                company_id: "co-B".into(),
                kind: "D205".into(),
                period: "2025".into(),
                is_rectificative: false,
                file_path: None,
            },
        )
        .await
        .unwrap();

        // list(A) trebuie să returneze 2 rânduri, cele mai recente primele, fără B
        let a_filings = list(&pool, "co-A").await.unwrap();
        assert_eq!(a_filings.len(), 2, "co-A trebuie să aibă 2 depuneri");
        assert_eq!(a_filings[0].kind, "D112", "cel mai recent = D112");
        assert!(a_filings[0].is_rectificative);
        assert_eq!(a_filings[1].kind, "D300");
        assert!(!a_filings[1].is_rectificative);

        // list(B) = 1 rând, nu include A
        let b_filings = list(&pool, "co-B").await.unwrap();
        assert_eq!(b_filings.len(), 1);
        assert_eq!(b_filings[0].kind, "D205");

        // delete cu company_id greșit nu șterge nimic
        let id_a0 = a_filings[0].id.clone();
        delete(&pool, &id_a0, "co-B").await.unwrap(); // company greșită → noop
        let a_after = list(&pool, "co-A").await.unwrap();
        assert_eq!(
            a_after.len(),
            2,
            "delete cu firmă greșită nu trebuie să șteargă"
        );

        // delete cu company_id corect șterge
        delete(&pool, &id_a0, "co-A").await.unwrap();
        let a_final = list(&pool, "co-A").await.unwrap();
        assert_eq!(a_final.len(), 1, "după delete rămâne 1 depunere");
        assert_eq!(a_final[0].kind, "D300");
    }
}
