//! Facturi de avans — art. 282 Cod Fiscal.
//!
//! TVA devine exigibilă la data primirii avansului (emitent) sau a plății (beneficiar).
//!
//! **Conturi folosite:**
//!   - 419 „Clienți-creditori"     — avansuri încasate de la clienți (factură de avans EMISĂ)
//!   - 4091 „Furnizori-debitori"   — avansuri plătite furnizorilor (factură de avans PRIMITĂ)
//!
//! **Monografie (art.282):**
//!   EMITENT:
//!     Factură de avans  → D 4111 = C 419 (bază) + C 4427 (TVA)   [NU 707!]
//!     Regularizare finală → storno avans: D 4111 = C (−419) + C (−4427)
//!                          + factura de livrare normală: D 4111 = C 707 + C 4427
//!   BENEFICIAR:
//!     Factură de avans primită → D 4091 (bază) + D 4426 (TVA) = C 401
//!     Regularizare finală      → storno: D (−4091) + D (−4426) = C 401 (−)
//!                                + factura de livrare normală
//!
//! **Storno (art.282 alin.11):** rata TVA folosită la storno este RATA AVANSULUI,
//! NU rata de la livrare (important pentru avansuri transfrontaliere de perioadă, ex.
//! avans la 19% înainte de 01.08.2025 și livrare la 21% după).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Models ─────────────────────────────────────────────────────────────────

/// Legătură factură finală ↔ factură de avans emisă (settlement).
/// Storno-ul avansului este generat automat de GL la regularizare.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceInvoiceSettlement {
    pub id: String,
    pub company_id: String,
    pub final_invoice_id: String,
    pub advance_invoice_id: String,
    /// Baza avansului (TEXT Decimal) — valoarea netă.
    pub advance_base: String,
    /// TVA avansului (TEXT Decimal).
    pub advance_vat: String,
    /// Cota TVA a avansului (TEXT, ex. "21.00") — folosită la storno.
    pub advance_vat_rate: String,
    pub created_at: i64,
}

/// Legătură factură finală primită ↔ factură de avans primită.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceReceivedSettlement {
    pub id: String,
    pub company_id: String,
    pub final_received_id: String,
    pub advance_received_id: String,
    pub advance_base: String,
    pub advance_vat: String,
    pub advance_vat_rate: String,
    pub created_at: i64,
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdvanceSettlementInput {
    pub company_id: String,
    pub final_invoice_id: String,
    pub advance_invoice_id: String,
    /// Baza avansului (valoare pozitivă — storno-ul folosește semnul negativ automat).
    pub advance_base: String,
    pub advance_vat: String,
    pub advance_vat_rate: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdvanceReceivedSettlementInput {
    pub company_id: String,
    pub final_received_id: String,
    pub advance_received_id: String,
    pub advance_base: String,
    pub advance_vat: String,
    pub advance_vat_rate: String,
}

// ─── DB operations ──────────────────────────────────────────────────────────

/// Creează o legătură de regularizare (settlement) între o factură finală și un avans emis.
/// Idempotent per (final_invoice_id, advance_invoice_id).
pub async fn create_advance_settlement(
    pool: &SqlitePool,
    input: CreateAdvanceSettlementInput,
) -> AppResult<AdvanceInvoiceSettlement> {
    // Validare baze numerice
    let base = Decimal::from_str_exact(&input.advance_base)
        .map_err(|_| AppError::Validation("advance_base nu este un număr valid".into()))?;
    let vat = Decimal::from_str_exact(&input.advance_vat)
        .map_err(|_| AppError::Validation("advance_vat nu este un număr valid".into()))?;
    let rate = Decimal::from_str_exact(&input.advance_vat_rate)
        .map_err(|_| AppError::Validation("advance_vat_rate nu este un număr valid".into()))?;

    if base < Decimal::ZERO {
        return Err(AppError::Validation(
            "advance_base trebuie să fie pozitiv".into(),
        ));
    }
    if vat < Decimal::ZERO {
        return Err(AppError::Validation(
            "advance_vat trebuie să fie pozitiv".into(),
        ));
    }
    if rate < Decimal::ZERO {
        return Err(AppError::Validation(
            "advance_vat_rate trebuie să fie pozitiv".into(),
        ));
    }

    // Verificare că factura de avans aparține companiei și are invoice_kind='advance'
    let advance_kind: Option<String> =
        sqlx::query_scalar("SELECT invoice_kind FROM invoices WHERE id = ?1 AND company_id = ?2")
            .bind(&input.advance_invoice_id)
            .bind(&input.company_id)
            .fetch_optional(pool)
            .await?;

    match advance_kind.as_deref() {
        None => return Err(AppError::NotFound),
        Some(k) if k != "advance" => {
            return Err(AppError::Validation(
                "Factura indicată nu este de tip avans (invoice_kind != 'advance')".into(),
            ));
        }
        _ => {}
    }

    // Verificare că factura finală aparține companiei și este standard (nu avans)
    let final_kind: Option<String> =
        sqlx::query_scalar("SELECT invoice_kind FROM invoices WHERE id = ?1 AND company_id = ?2")
            .bind(&input.final_invoice_id)
            .bind(&input.company_id)
            .fetch_optional(pool)
            .await?;

    match final_kind.as_deref() {
        None => return Err(AppError::NotFound),
        Some("advance") => {
            return Err(AppError::Validation(
                "Factura finală nu poate fi ea însăși o factură de avans".into(),
            ));
        }
        _ => {}
    }

    // Idempotency check
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM advance_invoice_settlements \
         WHERE company_id = ?1 AND final_invoice_id = ?2 AND advance_invoice_id = ?3",
    )
    .bind(&input.company_id)
    .bind(&input.final_invoice_id)
    .bind(&input.advance_invoice_id)
    .fetch_optional(pool)
    .await?;

    if let Some(existing_id) = existing {
        return get_advance_settlement(pool, &existing_id, &input.company_id).await;
    }

    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO advance_invoice_settlements \
         (id, company_id, final_invoice_id, advance_invoice_id, \
          advance_base, advance_vat, advance_vat_rate, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.final_invoice_id)
    .bind(&input.advance_invoice_id)
    .bind(format!("{:.2}", base))
    .bind(format!("{:.2}", vat))
    .bind(format!("{:.2}", rate))
    .bind(now)
    .execute(pool)
    .await?;

    get_advance_settlement(pool, &id, &input.company_id).await
}

/// Preia un settlement emis după id.
pub async fn get_advance_settlement(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<AdvanceInvoiceSettlement> {
    sqlx::query_as::<_, AdvanceInvoiceSettlement>(
        "SELECT id, company_id, final_invoice_id, advance_invoice_id, \
         advance_base, advance_vat, advance_vat_rate, created_at \
         FROM advance_invoice_settlements WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Listează settlement-urile pentru o factură finală emisă.
pub async fn list_advance_settlements_for_final(
    pool: &SqlitePool,
    final_invoice_id: &str,
    company_id: &str,
) -> AppResult<Vec<AdvanceInvoiceSettlement>> {
    Ok(sqlx::query_as::<_, AdvanceInvoiceSettlement>(
        "SELECT id, company_id, final_invoice_id, advance_invoice_id, \
         advance_base, advance_vat, advance_vat_rate, created_at \
         FROM advance_invoice_settlements \
         WHERE company_id = ?1 AND final_invoice_id = ?2 \
         ORDER BY created_at",
    )
    .bind(company_id)
    .bind(final_invoice_id)
    .fetch_all(pool)
    .await?)
}

/// Șterge un settlement emis (doar pre-GL, DRAFT invoices).
pub async fn delete_advance_settlement(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<()> {
    let rows =
        sqlx::query("DELETE FROM advance_invoice_settlements WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .execute(pool)
            .await?
            .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Received advance settlements ────────────────────────────────────────────

/// Creează o legătură de regularizare pentru facturi primite de avans.
pub async fn create_advance_received_settlement(
    pool: &SqlitePool,
    input: CreateAdvanceReceivedSettlementInput,
) -> AppResult<AdvanceReceivedSettlement> {
    let base = Decimal::from_str_exact(&input.advance_base)
        .map_err(|_| AppError::Validation("advance_base nu este un număr valid".into()))?;
    let vat = Decimal::from_str_exact(&input.advance_vat)
        .map_err(|_| AppError::Validation("advance_vat nu este un număr valid".into()))?;
    let rate = Decimal::from_str_exact(&input.advance_vat_rate)
        .map_err(|_| AppError::Validation("advance_vat_rate nu este un număr valid".into()))?;

    if base < Decimal::ZERO || vat < Decimal::ZERO || rate < Decimal::ZERO {
        return Err(AppError::Validation(
            "Valorile avansului trebuie să fie pozitive".into(),
        ));
    }

    // Verificare că factura de avans primită aparține companiei și este marcată ca avans
    let is_advance: Option<i64> = sqlx::query_scalar(
        "SELECT is_advance FROM received_invoices WHERE id = ?1 AND company_id = ?2",
    )
    .bind(&input.advance_received_id)
    .bind(&input.company_id)
    .fetch_optional(pool)
    .await?;

    match is_advance {
        None => return Err(AppError::NotFound),
        Some(0) => {
            return Err(AppError::Validation(
                "Factura primită indicată nu este marcată ca avans (is_advance = 0)".into(),
            ));
        }
        _ => {}
    }

    // Idempotency check
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM advance_received_settlements \
         WHERE company_id = ?1 AND final_received_id = ?2 AND advance_received_id = ?3",
    )
    .bind(&input.company_id)
    .bind(&input.final_received_id)
    .bind(&input.advance_received_id)
    .fetch_optional(pool)
    .await?;

    if let Some(existing_id) = existing {
        return get_advance_received_settlement(pool, &existing_id, &input.company_id).await;
    }

    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO advance_received_settlements \
         (id, company_id, final_received_id, advance_received_id, \
          advance_base, advance_vat, advance_vat_rate, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.final_received_id)
    .bind(&input.advance_received_id)
    .bind(format!("{:.2}", base))
    .bind(format!("{:.2}", vat))
    .bind(format!("{:.2}", rate))
    .bind(now)
    .execute(pool)
    .await?;

    get_advance_received_settlement(pool, &id, &input.company_id).await
}

/// Preia un settlement primit după id.
pub async fn get_advance_received_settlement(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<AdvanceReceivedSettlement> {
    sqlx::query_as::<_, AdvanceReceivedSettlement>(
        "SELECT id, company_id, final_received_id, advance_received_id, \
         advance_base, advance_vat, advance_vat_rate, created_at \
         FROM advance_received_settlements WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Listează settlement-urile pentru o factură finală primită.
pub async fn list_advance_received_settlements_for_final(
    pool: &SqlitePool,
    final_received_id: &str,
    company_id: &str,
) -> AppResult<Vec<AdvanceReceivedSettlement>> {
    Ok(sqlx::query_as::<_, AdvanceReceivedSettlement>(
        "SELECT id, company_id, final_received_id, advance_received_id, \
         advance_base, advance_vat, advance_vat_rate, created_at \
         FROM advance_received_settlements \
         WHERE company_id = ?1 AND final_received_id = ?2 \
         ORDER BY created_at",
    )
    .bind(company_id)
    .bind(final_received_id)
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // Company
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Contact
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES ('ct','co','CUSTOMER','Client SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn seed_invoice(pool: &SqlitePool, id: &str, kind: &str, number: i64) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, subtotal_amount, vat_amount, total_amount, \
              status, invoice_kind) \
             VALUES (?1,'co','ct','AV',?2,?3,'2026-06-01','2026-06-01','1000','210','1210','VALIDATED',?4)",
        )
        .bind(id)
        .bind(number)
        .bind(format!("AV-{number:04}"))
        .bind(kind)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_received_invoice(pool: &SqlitePool, id: &str, is_advance: i64) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, currency, issue_date, xml_path, status, is_advance, \
              downloaded_at, created_at) \
             VALUES (?1,'co',?2,'RO999','Furnizor','1210','RON','2026-06-01','/x.xml','NEW',?3,1,1)",
        )
        .bind(id)
        .bind(format!("dl-{id}"))
        .bind(is_advance)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_advance_settlement_roundtrip() {
        let pool = pool().await;
        seed_invoice(&pool, "adv1", "advance", 1).await;
        seed_invoice(&pool, "fin1", "standard", 2).await;

        let settlement = create_advance_settlement(
            &pool,
            CreateAdvanceSettlementInput {
                company_id: "co".into(),
                final_invoice_id: "fin1".into(),
                advance_invoice_id: "adv1".into(),
                advance_base: "1000".into(),
                advance_vat: "210".into(),
                advance_vat_rate: "21".into(),
            },
        )
        .await
        .unwrap();

        assert_eq!(settlement.advance_invoice_id, "adv1");
        assert_eq!(settlement.final_invoice_id, "fin1");
        assert_eq!(settlement.advance_base, "1000.00");
        assert_eq!(settlement.advance_vat, "210.00");
        assert_eq!(settlement.advance_vat_rate, "21.00");
    }

    #[tokio::test]
    async fn create_settlement_rejects_non_advance_invoice() {
        let pool = pool().await;
        // Both are standard
        seed_invoice(&pool, "s1", "standard", 1).await;
        seed_invoice(&pool, "s2", "standard", 2).await;

        let err = create_advance_settlement(
            &pool,
            CreateAdvanceSettlementInput {
                company_id: "co".into(),
                final_invoice_id: "s2".into(),
                advance_invoice_id: "s1".into(), // not an advance!
                advance_base: "100".into(),
                advance_vat: "21".into(),
                advance_vat_rate: "21".into(),
            },
        )
        .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn create_settlement_is_idempotent() {
        let pool = pool().await;
        seed_invoice(&pool, "adv2", "advance", 3).await;
        seed_invoice(&pool, "fin2", "standard", 4).await;

        let input = CreateAdvanceSettlementInput {
            company_id: "co".into(),
            final_invoice_id: "fin2".into(),
            advance_invoice_id: "adv2".into(),
            advance_base: "500".into(),
            advance_vat: "105".into(),
            advance_vat_rate: "21".into(),
        };

        let s1 = create_advance_settlement(&pool, input.clone())
            .await
            .unwrap();
        let s2 = create_advance_settlement(&pool, input).await.unwrap();
        assert_eq!(s1.id, s2.id, "settlement creation must be idempotent");
    }

    #[tokio::test]
    async fn list_settlements_for_final() {
        let pool = pool().await;
        seed_invoice(&pool, "adv3", "advance", 5).await;
        seed_invoice(&pool, "adv4", "advance", 6).await;
        seed_invoice(&pool, "fin3", "standard", 7).await;

        for (adv, base, vat, n) in [("adv3", "600", "126", "21"), ("adv4", "400", "84", "21")] {
            create_advance_settlement(
                &pool,
                CreateAdvanceSettlementInput {
                    company_id: "co".into(),
                    final_invoice_id: "fin3".into(),
                    advance_invoice_id: adv.into(),
                    advance_base: base.into(),
                    advance_vat: vat.into(),
                    advance_vat_rate: n.into(),
                },
            )
            .await
            .unwrap();
        }

        let list = list_advance_settlements_for_final(&pool, "fin3", "co")
            .await
            .unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn received_advance_settlement_roundtrip() {
        let pool = pool().await;
        seed_received_invoice(&pool, "ra1", 1).await; // is_advance = 1
        seed_received_invoice(&pool, "rf1", 0).await; // final, is_advance = 0

        // Mark rf1 as final (not advance) — already done by seeding with 0.

        let s = create_advance_received_settlement(
            &pool,
            CreateAdvanceReceivedSettlementInput {
                company_id: "co".into(),
                final_received_id: "rf1".into(),
                advance_received_id: "ra1".into(),
                advance_base: "1000".into(),
                advance_vat: "210".into(),
                advance_vat_rate: "21".into(),
            },
        )
        .await
        .unwrap();

        assert_eq!(s.advance_received_id, "ra1");
        assert_eq!(s.final_received_id, "rf1");
    }
}
