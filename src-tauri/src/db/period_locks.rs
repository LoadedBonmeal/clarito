//! Blocare perioade fiscale — previne re-postarea GL în luni deja declarate.
//!
//! O perioadă blocată (YYYY-MM) nu mai permite `generate_gl_entries` să suprascrie
//! cifrele declarate fără confirmare explicită (`allow_locked = true`).
//! Blocarea se face automat la depunerea unei declarații lunare (D300, D112 etc.)
//! sau manual de contabil/admin.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::models::{new_id, now_unix};
use crate::error::AppResult;

// ─── Tipuri publice ───────────────────────────────────────────────────────────

/// Rândul din `period_locks`, serializat camelCase spre frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodLock {
    pub id: String,
    pub company_id: String,
    /// Luna blocată: "YYYY-MM".
    pub period: String,
    /// Unix timestamp (secunde) al blocării.
    pub locked_at: i64,
    /// Sursa blocării: "declaration:D300" | "declaration:D112" | "manual" | …
    pub source: String,
    pub locked_by: Option<String>,
    pub note: Option<String>,
}

// ─── Operații DB ─────────────────────────────────────────────────────────────

/// Blochează o perioadă (idempotent — ON CONFLICT DO NOTHING păstrează prima blocare).
///
/// Aceeași (company_id, period) poate fi apelată de mai multe ori fără eroare;
/// înregistrarea originală (prima declarație depusă) rămâne intactă.
pub async fn lock_period(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
    source: &str,
    locked_by: Option<&str>,
    note: Option<&str>,
) -> AppResult<()> {
    let id = new_id();
    let locked_at = now_unix();

    sqlx::query(
        "INSERT INTO period_locks (id, company_id, period, locked_at, source, locked_by, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(company_id, period) DO NOTHING",
    )
    .bind(&id)
    .bind(company_id)
    .bind(period)
    .bind(locked_at)
    .bind(source)
    .bind(locked_by)
    .bind(note)
    .execute(pool)
    .await?;

    Ok(())
}

/// Deblochează o perioadă (DELETE scoped pe companie).
pub async fn unlock_period(pool: &SqlitePool, company_id: &str, period: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM period_locks WHERE company_id = ?1 AND period = ?2")
        .bind(company_id)
        .bind(period)
        .execute(pool)
        .await?;

    Ok(())
}

/// Returnează `true` dacă perioada este blocată.
/// Înghite erorile DB (returnează `false` în caz de eroare).
pub async fn is_period_locked(pool: &SqlitePool, company_id: &str, period: &str) -> bool {
    let result: Result<Option<i64>, _> = sqlx::query_scalar(
        "SELECT 1 FROM period_locks WHERE company_id = ?1 AND period = ?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(period)
    .fetch_optional(pool)
    .await;

    matches!(result, Ok(Some(_)))
}

/// Listează toate perioadele blocate pentru o firmă, cele mai recente primele.
pub async fn list_period_locks(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<PeriodLock>> {
    let rows = sqlx::query_as::<_, PeriodLockRow>(
        "SELECT id, company_id, period, locked_at, source, locked_by, note \
         FROM period_locks \
         WHERE company_id = ?1 \
         ORDER BY period DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(PeriodLock::from).collect())
}

/// Returnează lista de luni blocate (YYYY-MM) în intervalul [period_from, period_to].
///
/// `period_from` și `period_to` sunt date ISO (YYYY-MM-DD); comparația se face
/// pe primele 7 caractere (YYYY-MM).
pub async fn locked_months_in_range(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT period FROM period_locks \
         WHERE company_id = ?1 \
           AND period >= SUBSTR(?2, 1, 7) \
           AND period <= SUBSTR(?3, 1, 7) \
         ORDER BY period",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// ─── Row intern ───────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct PeriodLockRow {
    id: String,
    company_id: String,
    period: String,
    locked_at: i64,
    source: String,
    locked_by: Option<String>,
    note: Option<String>,
}

impl From<PeriodLockRow> for PeriodLock {
    fn from(r: PeriodLockRow) -> Self {
        PeriodLock {
            id: r.id,
            company_id: r.company_id,
            period: r.period,
            locked_at: r.locked_at,
            source: r.source,
            locked_by: r.locked_by,
            note: r.note,
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
        sqlx::query("CREATE TABLE companies (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO companies VALUES ('co-A'), ('co-B')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE period_locks ( \
              id TEXT PRIMARY KEY NOT NULL, \
              company_id TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE, \
              period TEXT NOT NULL, \
              locked_at INTEGER NOT NULL, \
              source TEXT NOT NULL, \
              locked_by TEXT, \
              note TEXT, \
              UNIQUE(company_id, period) \
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_lock_unlock_round_trip() {
        let pool = setup().await;

        lock_period(&pool, "co-A", "2026-05", "declaration:D300", None, None)
            .await
            .unwrap();
        assert!(
            is_period_locked(&pool, "co-A", "2026-05").await,
            "trebuie să fie blocat după lock"
        );

        unlock_period(&pool, "co-A", "2026-05").await.unwrap();
        assert!(
            !is_period_locked(&pool, "co-A", "2026-05").await,
            "trebuie să fie deblocat după unlock"
        );
    }

    #[tokio::test]
    async fn test_lock_idempotent_upsert() {
        let pool = setup().await;

        // Prima blocare
        lock_period(&pool, "co-A", "2026-04", "declaration:D300", None, None)
            .await
            .unwrap();
        // A doua blocare (aceeași companie + perioadă) — nu trebuie să dea eroare
        lock_period(
            &pool,
            "co-A",
            "2026-04",
            "declaration:D112",
            None,
            Some("rectificativa"),
        )
        .await
        .unwrap();

        // Trebuie să existe un singur rând (prima blocare)
        let locks = list_period_locks(&pool, "co-A").await.unwrap();
        assert_eq!(locks.len(), 1, "ON CONFLICT DO NOTHING → exact 1 rând");
        assert_eq!(
            locks[0].source, "declaration:D300",
            "prima blocare este păstrată"
        );
    }

    #[tokio::test]
    async fn test_locked_months_in_range() {
        let pool = setup().await;

        lock_period(&pool, "co-A", "2026-01", "declaration:D300", None, None)
            .await
            .unwrap();
        lock_period(&pool, "co-A", "2026-03", "declaration:D112", None, None)
            .await
            .unwrap();

        let months = locked_months_in_range(&pool, "co-A", "2026-01-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(months, vec!["2026-01", "2026-03"]);
        assert!(
            !months.contains(&"2026-02".to_string()),
            "2026-02 nu este blocat"
        );
    }

    #[tokio::test]
    async fn test_no_locks_default() {
        let pool = setup().await;

        assert!(
            !is_period_locked(&pool, "co-A", "2026-01").await,
            "fără rânduri → deblocat"
        );

        let locks = list_period_locks(&pool, "co-A").await.unwrap();
        assert!(locks.is_empty(), "fără rânduri → listă goală");

        let months = locked_months_in_range(&pool, "co-A", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        assert!(months.is_empty(), "fără rânduri → luni goale");
    }

    #[tokio::test]
    async fn test_company_scoped() {
        let pool = setup().await;

        // Blochează pentru co-A
        lock_period(&pool, "co-A", "2026-06", "manual", None, None)
            .await
            .unwrap();

        // co-B nu trebuie să vadă blocarea co-A
        assert!(
            !is_period_locked(&pool, "co-B", "2026-06").await,
            "blocarea co-A nu este vizibilă pentru co-B"
        );
        let locks_b = list_period_locks(&pool, "co-B").await.unwrap();
        assert!(locks_b.is_empty(), "co-B nu are blocare");

        let months_b = locked_months_in_range(&pool, "co-B", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        assert!(months_b.is_empty(), "co-B: nicio lună blocată");
    }
}
