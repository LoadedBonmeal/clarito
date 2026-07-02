//! Note contabile manuale (cod 14-6-2A) — Tauri commands.
//!
//! Expune trei comenzi:
//! - `create_manual_journal` — validează, generează UUID, apelează `post_manual_journal`.
//! - `list_manual_journals`  — returnează notele MANUAL pentru o perioadă.
//! - `delete_manual_journal` — șterge DOAR notele MANUAL (nu le atinge pe cele auto).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::accounts;
use crate::db::gl::{
    delete_manual_journal as db_delete, list_manual_journals as db_list, post_manual_journal,
    ManualJournal, ManualJournalView,
};
use crate::db::models::new_id;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── Input types ──────────────────────────────────────────────────────────────

/// O linie a notei contabile trimisă de frontend.
/// `debit` și `credit` sunt String (nu f64) — parsăm în Decimal pe server.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualLineInput {
    pub account_code: String,
    /// Suma debit — String parsat în Decimal; "0" sau "" → zero.
    pub debit: String,
    /// Suma credit — String parsat în Decimal; "0" sau "" → zero.
    pub credit: String,
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Creează o notă contabilă manuală (cod 14-6-2A).
///
/// Validări la intrare:
/// - cel puțin 2 linii;
/// - fiecare linie are EXACT una dintre debit / credit > 0 (nu ambele, nu niciuna);
/// - fiecare `account_code` există în planul de conturi al companiei;
/// - data nenulă / validă (AAAA-LL-ZZ);
/// - nota echilibrată (Σdebit == Σcredit, toleranță 0,005 — verificat de `post_manual_journal`).
///
/// Returnează `source_id`-ul generat (UUID) al notei.
///
/// Validează perechea (debit, credit) a unei linii: sume NE-negative + exact una nenulă (debit XOR
/// credit). Pură + testabilă — `idx` e numărul liniei (1-based) pentru mesaje.
fn validate_manual_line_amounts(idx: usize, d: Decimal, c: Decimal) -> AppResult<()> {
    // Ne-negative: altfel {debit:-50, credit:100} ar trece de XOR (debitul negativ pare zero) și ar
    // persista o înregistrare malformată cu ambele coloane completate.
    if d.is_sign_negative() || c.is_sign_negative() {
        return Err(AppError::Validation(format!(
            "Linia {idx}: sumele debit/credit nu pot fi negative."
        )));
    }
    let tol = Decimal::new(1, 3); // 0.001
    match (d > tol, c > tol) {
        (true, true) => Err(AppError::Validation(format!(
            "Linia {idx}: o linie contabilă nu poate avea atât debit cât și credit completate."
        ))),
        (false, false) => Err(AppError::Validation(format!(
            "Linia {idx}: completați suma debit SAU suma credit (nu ambele zero)."
        ))),
        _ => Ok(()),
    }
}

#[tauri::command]
pub async fn create_manual_journal(
    state: State<'_, AppState>,
    company_id: String,
    date: String,
    description: String,
    lines: Vec<ManualLineInput>,
) -> AppResult<String> {
    // ── Validare dată ────────────────────────────────────────────────────────
    crate::commands::require_valid_date("Data notei", &date)?;

    // ── Validare număr minim de linii ─────────────────────────────────────────
    if lines.len() < 2 {
        return Err(AppError::Validation(
            "Nota contabilă trebuie să aibă cel puțin 2 linii.".into(),
        ));
    }

    // ── Parsare sume + validare debit XOR credit ──────────────────────────────
    let mut parsed: Vec<(String, Decimal, Decimal)> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let code = line.account_code.trim().to_string();
        if code.is_empty() {
            return Err(AppError::Validation(format!(
                "Linia {} nu are un cod de cont selectat.",
                i + 1
            )));
        }
        let d = parse_amount(&line.debit).map_err(|_| {
            AppError::Validation(format!(
                "Linia {}: suma debit invalidă «{}».",
                i + 1,
                line.debit
            ))
        })?;
        let c = parse_amount(&line.credit).map_err(|_| {
            AppError::Validation(format!(
                "Linia {}: suma credit invalidă «{}».",
                i + 1,
                line.credit
            ))
        })?;

        validate_manual_line_amounts(i + 1, d, c)?;
        parsed.push((code, d, c));
    }

    // ── Validare conturi în planul de conturi ─────────────────────────────────
    let accounts = accounts::list(&state.db, &company_id).await?;
    for (code, _, _) in &parsed {
        if !accounts.iter().any(|a| &a.account_code == code) {
            return Err(AppError::Validation(format!(
                "Contul «{code}» nu există în planul de conturi al companiei. \
                 Verificați codul sau adăugați contul mai întâi."
            )));
        }
    }

    // ── Postare ───────────────────────────────────────────────────────────────
    let source_id = new_id();
    let desc_trimmed = description.trim();

    let line_refs: Vec<(&str, Decimal, Decimal)> = parsed
        .iter()
        .map(|(code, d, c)| (code.as_str(), *d, *c))
        .collect();

    post_manual_journal(
        &state.db,
        &ManualJournal {
            company_id: &company_id,
            journal_id: "NC",
            journal_type: "MANUAL",
            source_type: "MANUAL",
            source_id: &source_id,
            date: &date,
            description: desc_trimmed,
            partner_cui: None,
        },
        &line_refs,
    )
    .await
    .map_err(|e| {
        // Surface assert_balanced errors as a friendly Romanian message.
        let msg = e.to_string();
        if msg.contains("GL dezechilibrat") || msg.contains("dezechilibrat") {
            // Extract sums from error if possible for a cleaner message, else generic.
            AppError::Validation(format!(
                "Nota nu este echilibrată — verificați că Σdebit = Σcredit. Detaliu: {msg}"
            ))
        } else {
            e
        }
    })?;

    Ok(source_id)
}

/// Listează notele contabile manuale (source_type='MANUAL') dintr-o perioadă.
#[tauri::command]
pub async fn list_manual_journals(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<Vec<ManualJournalView>> {
    crate::commands::require_valid_date("Data de început", &period_from)?;
    crate::commands::require_valid_date("Data de sfârșit", &period_to)?;
    db_list(&state.db, &company_id, &period_from, &period_to).await
}

/// Șterge o notă contabilă manuală identificată prin `source_id`.
/// **Protecție**: șterge DOAR jurnale cu source_type='MANUAL' — nu poate atinge
/// jurnale generate automat (INVOICE, PAYMENT, VAT_CLOSE etc.).
/// Returnează `true` dacă nota a fost găsită și ștearsă, `false` dacă nu exista.
#[tauri::command]
pub async fn delete_manual_journal(
    state: State<'_, AppState>,
    company_id: String,
    source_id: String,
) -> AppResult<bool> {
    let rows = db_delete(&state.db, &company_id, &source_id).await?;
    Ok(rows > 0)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn parse_amount(s: &str) -> Result<Decimal, rust_decimal::Error> {
    let trimmed = s.trim().replace(',', ".");
    if trimmed.is_empty() || trimmed == "0" || trimmed == "0.00" {
        return Ok(Decimal::ZERO);
    }
    trimmed.parse::<Decimal>()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::validate_manual_line_amounts;
    use crate::db::accounts;
    use crate::db::gl::{
        delete_manual_journal as db_delete_journal, list_manual_journals, post_manual_journal,
        ManualJournal,
    };
    use crate::db::models::new_id;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec as rdec;
    use sqlx::SqlitePool;

    // ── Helpers ────────────────────────────────────────────────────────────────

    async fn setup_pool() -> SqlitePool {
        // foreign_keys(true) so the gl_journal→gl_entry ON DELETE CASCADE actually fires in tests
        // (sqlx leaves PRAGMA foreign_keys OFF by default; production enables it in db/pool.rs).
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("sqlite url")
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(opts)
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    async fn insert_company(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .bind(id)
        .execute(pool)
        .await
        .expect("insert company");
    }

    async fn insert_account(pool: &SqlitePool, company_id: &str, code: &str, name: &str) {
        let id = new_id();
        sqlx::query(
            "INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, active, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,1,1,1)",
        )
        .bind(&id)
        .bind(company_id)
        .bind(code)
        .bind(name)
        .execute(pool)
        .await
        .expect("insert account");
    }

    // ── Test 1: nota echilibrată 2 linii → creare + listare ───────────────────

    #[tokio::test]
    async fn balanced_note_creates_and_lists_correctly() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        // Postăm direct cu post_manual_journal (conturile nu se verifică la nivel DB).
        let src = new_id();
        post_manual_journal(
            &pool,
            &ManualJournal {
                company_id: "co1",
                journal_id: "NC",
                journal_type: "MANUAL",
                source_type: "MANUAL",
                source_id: &src,
                date: "2026-06-01",
                description: "Test echilibrat",
                partner_cui: None,
            },
            &[
                ("5311", rdec!(100), Decimal::ZERO),
                ("7588", Decimal::ZERO, rdec!(100)),
            ],
        )
        .await
        .expect("post_manual_journal OK");

        // list_manual_journals returnează nota cu totaluri corecte.
        let views = list_manual_journals(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .expect("list OK");

        assert_eq!(views.len(), 1, "o notă în perioadă");
        let v = &views[0];
        assert_eq!(v.source_id, src);
        assert_eq!(v.total_debit, "100.00");
        assert_eq!(v.total_credit, "100.00");
        assert_eq!(v.lines.len(), 2);

        // Verificăm că rândurile gl_entry există cu source_type='MANUAL'.
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id='co1' AND j.source_type='MANUAL' AND j.source_id=?1",
        )
        .bind(&src)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 2, "2 linii gl_entry cu source_type=MANUAL");
    }

    // ── Test 2: notă dezechilibrată → eroare, nimic persistat ─────────────────

    #[tokio::test]
    async fn unbalanced_note_returns_error_and_nothing_persisted() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        let src = new_id();
        let result = post_manual_journal(
            &pool,
            &ManualJournal {
                company_id: "co1",
                journal_id: "NC",
                journal_type: "MANUAL",
                source_type: "MANUAL",
                source_id: &src,
                date: "2026-06-05",
                description: "Dezechilibrat",
                partner_cui: None,
            },
            &[
                ("5311", rdec!(100), Decimal::ZERO),
                ("7588", Decimal::ZERO, rdec!(90)), // 100 ≠ 90
            ],
        )
        .await;

        assert!(
            result.is_err(),
            "nota dezechilibrată trebuie să returneze eroare"
        );

        // Nimic nu a fost persistat.
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id='co1' AND source_type='MANUAL' AND source_id=?1",
        )
        .bind(&src)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 0, "nimic persistat pentru nota dezechilibrată");
    }

    // ── Test 3: delete elimină jurnalul + intrările ────────────────────────────

    #[tokio::test]
    async fn delete_removes_journal_and_entries() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        let src = new_id();
        post_manual_journal(
            &pool,
            &ManualJournal {
                company_id: "co1",
                journal_id: "NC",
                journal_type: "MANUAL",
                source_type: "MANUAL",
                source_id: &src,
                date: "2026-06-10",
                description: "De șters",
                partner_cui: None,
            },
            &[
                ("5311", rdec!(200), Decimal::ZERO),
                ("7588", Decimal::ZERO, rdec!(200)),
            ],
        )
        .await
        .expect("post OK");

        // Verificăm că există înainte de ștergere.
        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id='co1' AND source_type='MANUAL' AND source_id=?1",
        )
        .bind(&src)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, 1);

        // Capturăm journal_pk + numărăm intrările DIRECT (fără JOIN) ca să dovedim că ștergerea
        // chiar cascadează intrările, nu le orfanizează (FK ON în setup_pool).
        let jpk: String = sqlx::query_scalar("SELECT id FROM gl_journal WHERE source_id=?1")
            .bind(&src)
            .fetch_one(&pool)
            .await
            .unwrap();
        let entries_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM gl_entry WHERE journal_pk=?1")
                .bind(&jpk)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(entries_before, 2, "2 intrări înainte de ștergere");

        let rows = db_delete_journal(&pool, "co1", &src)
            .await
            .expect("delete OK");
        assert_eq!(rows, 1u64, "un rând șters din gl_journal");

        // Verificăm că jurnalul + intrările au dispărut (CASCADE).
        let after_j: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gl_journal WHERE source_id=?1")
            .bind(&src)
            .fetch_one(&pool)
            .await
            .unwrap();
        let after_e: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gl_entry WHERE journal_pk=?1")
            .bind(&jpk)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after_j, 0, "gl_journal șters");
        assert_eq!(after_e, 0, "gl_entry șterse prin CASCADE (FK ON)");
    }

    // ── Test 3b (Wave 4 audit): delete refuzat când luna notei este BLOCATĂ ───

    #[tokio::test]
    async fn delete_refused_on_locked_period() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        let src = new_id();
        post_manual_journal(
            &pool,
            &ManualJournal {
                company_id: "co1",
                journal_id: "NC",
                journal_type: "MANUAL",
                source_type: "MANUAL",
                source_id: &src,
                date: "2026-06-10",
                description: "În lună blocată",
                partner_cui: None,
            },
            &[
                ("5311", rdec!(200), Decimal::ZERO),
                ("7588", Decimal::ZERO, rdec!(200)),
            ],
        )
        .await
        .expect("post OK");

        // Blocăm luna notei (declarație depusă pentru 2026-06).
        crate::db::period_locks::lock_period(
            &pool,
            "co1",
            "2026-06",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        let r = db_delete_journal(&pool, "co1", &src).await;
        assert!(
            matches!(r, Err(crate::error::AppError::Validation(_))),
            "delete într-o lună blocată trebuie refuzat cu Validation, got {r:?}"
        );
        // Nota trebuie să existe în continuare.
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co1' AND source_type='MANUAL' AND source_id=?1",
        )
        .bind(&src)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 1, "nota supraviețuiește ștergerii refuzate");

        // Deblocare → ștergerea reușește.
        crate::db::period_locks::unlock_period(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let rows = db_delete_journal(&pool, "co1", &src).await.unwrap();
        assert_eq!(rows, 1u64);
    }

    // ── Test 4: cont inexistent → validate_line_account respinge ──────────────

    #[tokio::test]
    async fn unknown_account_code_is_rejected() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;
        // Adăugăm 7588 dar NU și 9999.
        insert_account(&pool, "co1", "7588", "Alte venituri").await;

        // Simulăm validarea din create_manual_journal: contul 9999 nu există.
        let accounts = accounts::list(&pool, "co1").await.unwrap();
        let code = "9999";
        let found = accounts.iter().any(|a| a.account_code == code);
        assert!(!found, "9999 trebuie să fie respins");
    }

    // ── Test 5: delete NU atinge notele auto-generate (source_type ≠ MANUAL) ─

    #[tokio::test]
    async fn delete_only_targets_manual_source_type() {
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        // Inserăm un jurnal INVOICE (simulat) cu sursa separată.
        let invoice_src = new_id();
        let j_id = new_id();
        sqlx::query(
            "INSERT INTO gl_journal (id, company_id, journal_id, journal_type, \
             transaction_id, transaction_date, source_type, source_id, created_at) \
             VALUES (?1,'co1','VANZARI','INVOICE',?2,'2026-06-01','INVOICE',?2,1)",
        )
        .bind(&j_id)
        .bind(&invoice_src)
        .execute(&pool)
        .await
        .unwrap();

        // Încercăm să ștergem cu source_id-ul facturii — nu trebuie să atingă nimic.
        let rows = db_delete_journal(&pool, "co1", &invoice_src)
            .await
            .expect("delete OK");
        assert_eq!(rows, 0u64, "db_delete_journal nu atinge surse non-MANUAL");

        // Jurnalul INVOICE rămâne intact.
        let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gl_journal WHERE source_id=?1")
            .bind(&invoice_src)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(cnt, 1, "jurnalul INVOICE rămâne intact");
    }

    // ── Test 6: MANUAL supraviețuiește unui generate_gl_entries (surse diferite)

    #[tokio::test]
    async fn manual_journal_survives_generate_gl_entries_idempotency() {
        use crate::db::gl::generate_gl_entries;
        let pool = setup_pool().await;
        insert_company(&pool, "co1").await;

        // Postăm o notă MANUAL.
        let src = new_id();
        post_manual_journal(
            &pool,
            &ManualJournal {
                company_id: "co1",
                journal_id: "NC",
                journal_type: "MANUAL",
                source_type: "MANUAL",
                source_id: &src,
                date: "2026-06-15",
                description: "Survives regen",
                partner_cui: None,
            },
            &[
                ("5311", rdec!(50), Decimal::ZERO),
                ("7588", Decimal::ZERO, rdec!(50)),
            ],
        )
        .await
        .expect("post OK");

        // generate_gl_entries pe aceeași perioadă (fără documente → 0 jurnale auto).
        // Nu ar trebui să atingă source_type='MANUAL'.
        let _result = generate_gl_entries(&pool, "co1", "2026-06-01", "2026-06-30", false)
            .await
            .expect("generate OK");

        // Nota MANUAL trebuie să fie în continuare prezentă.
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal \
             WHERE company_id='co1' AND source_type='MANUAL' AND source_id=?1",
        )
        .bind(&src)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 1, "nota MANUAL supraviețuiește generate_gl_entries");
    }

    // ── Test 6: validarea perechii debit/credit (inclusiv respingerea sumelor negative) ──
    #[test]
    fn line_amount_validation_rejects_negative_and_double_sided() {
        // Valid: exact una nenulă.
        assert!(validate_manual_line_amounts(1, rdec!(100), Decimal::ZERO).is_ok());
        assert!(validate_manual_line_amounts(1, Decimal::ZERO, rdec!(100)).is_ok());
        // Negativ pe oricare parte → respins (chiar dacă cealaltă pare o linie validă).
        assert!(validate_manual_line_amounts(1, rdec!(-50), rdec!(100)).is_err());
        assert!(validate_manual_line_amounts(1, rdec!(100), rdec!(-50)).is_err());
        assert!(validate_manual_line_amounts(1, rdec!(-1), Decimal::ZERO).is_err());
        // Ambele completate → respins.
        assert!(validate_manual_line_amounts(1, rdec!(100), rdec!(100)).is_err());
        // Ambele zero → respins.
        assert!(validate_manual_line_amounts(1, Decimal::ZERO, Decimal::ZERO).is_err());
    }
}
