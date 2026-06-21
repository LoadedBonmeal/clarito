//! NIR (Notă de Intrare Recepție) — formular 14-3-1A, OMFP 2634/2015.
//!
//! ## GL flow (fără dublă-contare)
//! O factură primită postează deja D607=C401 per linie TVA (source_type='RECEIVED_INVOICE').
//! La finalizarea NIR, `record_movement(Dir::In)` adaugă D{stock_account}=C{expense_account}
//! (source_type='STOCK', per rând stock_ledger). Pentru marfă: D371=C607. Cele două note NET
//! la D371=C401 (stoc capitalizat). NICIO dublare — creditul 607 din nota STOCK anulează
//! debitul 607 din factura primită.
//!
//! ## Modul amănunt (global-valorică 371)
//! Când retail_mode=1, după mișcarea de stoc se postează o notă manuală idempotentă
//! (source_type='NIR', source_id=nir_id) via post_manual_journal:
//!   D371 = C378   (Σ adaos comercial)
//!   D371 = C4428  (Σ TVA neexigibilă)
//! Soldul 371 = cost + adaos + tva_neex = Σ preț amănunt.
//!
//! ## TVA
//! Cotele TVA sunt validate prin tabela `vat_rates` (numai ratele active).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::models::{new_id, now_unix};
use crate::db::products::resolve_accounts;
use crate::db::stock_valuation::{record_movement, Dir, StockMovementInput};
use crate::error::{AppError, AppResult};

// ─── Structs DB ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NirDocument {
    pub id: String,
    pub company_id: String,
    pub gestiune_id: String,
    pub received_invoice_id: Option<String>,
    pub supplier_name: Option<String>,
    pub supplier_cui: Option<String>,
    pub nir_series: Option<String>,
    pub nir_number: i64,
    pub nir_date: String,
    /// SQLite INTEGER 0/1 → bool via sqlx FromRow
    pub retail_mode: bool,
    pub status: String,
    pub comisie_receptie: Option<String>,
    pub observatii: Option<String>,
    pub created_at: i64,
    pub finalized_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NirLine {
    pub id: String,
    pub nir_id: String,
    pub product_id: Option<String>,
    pub denumire: String,
    pub um: Option<String>,
    pub qty: String,
    pub unit_cost: String,
    pub vat_rate: String,
    pub adaos_pct: Option<String>,
    pub value_cost: String,
    pub value_adaos: String,
    pub value_tva_neex: String,
    pub pret_amanunt: String,
    pub line_no: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NirWithLines {
    pub document: NirDocument,
    pub lines: Vec<NirLine>,
}

// ─── Input types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NirLineInput {
    pub product_id: Option<String>,
    pub denumire: String,
    pub um: Option<String>,
    pub qty: String,
    pub unit_cost: String,
    pub vat_rate: String,
    pub adaos_pct: Option<String>,
    pub line_no: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NirInput {
    pub gestiune_id: String,
    pub received_invoice_id: Option<String>,
    pub supplier_name: Option<String>,
    pub supplier_cui: Option<String>,
    pub nir_date: String,
    pub retail_mode: Option<bool>,
    pub comisie_receptie: Option<String>,
    pub observatii: Option<String>,
    pub lines: Vec<NirLineInput>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn round2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

fn parse_dec(s: &str, label: &str) -> AppResult<Decimal> {
    Decimal::from_str(s.trim())
        .map_err(|_| AppError::Validation(format!("{label}: valoare numerică invalidă: '{s}'")))
}

/// Validates that the given VAT rate string is present in `vat_rates` and is active.
/// Returns the parsed Decimal value.
pub async fn validate_active_vat_rate(pool: &SqlitePool, rate_str: &str) -> AppResult<Decimal> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT rate FROM vat_rates WHERE rate=?1 AND active=1")
            .bind(rate_str)
            .fetch_optional(pool)
            .await?;
    match found {
        Some(r) => Decimal::from_str(r.trim())
            .map_err(|_| AppError::Validation(format!("Cotă TVA invalidă: '{rate_str}'"))),
        None => Err(AppError::Validation(format!(
            "Cotă TVA invalidă sau inactivă: '{rate_str}'"
        ))),
    }
}

// ─── CRUD ─────────────────────────────────────────────────────────────────────

/// Creează un NIR nou cu status 'draft'. Alocă numărul secvențial per companie.
pub async fn create_nir(
    pool: &SqlitePool,
    company_id: &str,
    input: NirInput,
) -> AppResult<NirDocument> {
    // Validare gestiune (aparține companiei)
    if input.gestiune_id.trim().is_empty() {
        return Err(AppError::Validation("gestiune_id este obligatoriu.".into()));
    }
    let gest_ok: Option<String> =
        sqlx::query_scalar("SELECT id FROM gestiune WHERE id=?1 AND company_id=?2")
            .bind(&input.gestiune_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if gest_ok.is_none() {
        return Err(AppError::Validation(
            "Gestiunea nu există sau nu aparține companiei.".into(),
        ));
    }

    // Validare date
    if chrono::NaiveDate::parse_from_str(input.nir_date.trim(), "%Y-%m-%d").is_err() {
        return Err(AppError::Validation(format!(
            "Data NIR invalidă: '{}' — folosiți AAAA-LL-ZZ.",
            input.nir_date
        )));
    }

    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "NIR-ul trebuie să aibă cel puțin o linie.".into(),
        ));
    }

    let retail_mode = input.retail_mode.unwrap_or(false);

    // Validare linii + pre-computare valori
    struct ComputedLine {
        id: String,
        product_id: Option<String>,
        denumire: String,
        um: Option<String>,
        qty: Decimal,
        unit_cost: Decimal,
        vat_rate: Decimal,
        adaos_pct: Option<Decimal>,
        value_cost: Decimal,
        value_adaos: Decimal,
        value_tva_neex: Decimal,
        pret_amanunt: Decimal,
        line_no: i64,
    }

    let mut computed_lines: Vec<ComputedLine> = Vec::with_capacity(input.lines.len());
    for (i, ln) in input.lines.iter().enumerate() {
        if ln.denumire.trim().is_empty() {
            return Err(AppError::Validation(format!(
                "Linia {}: denumirea este obligatorie.",
                i + 1
            )));
        }
        let qty = parse_dec(&ln.qty, &format!("Linia {} cantitate", i + 1))?;
        if qty <= Decimal::ZERO {
            return Err(AppError::Validation(format!(
                "Linia {}: cantitatea trebuie să fie > 0.",
                i + 1
            )));
        }
        let unit_cost = parse_dec(&ln.unit_cost, &format!("Linia {} preț unitar", i + 1))?;
        if unit_cost.is_sign_negative() {
            return Err(AppError::Validation(format!(
                "Linia {}: prețul unitar nu poate fi negativ.",
                i + 1
            )));
        }

        let vat_rate = validate_active_vat_rate(pool, &ln.vat_rate).await?;

        let adaos_pct = match &ln.adaos_pct {
            Some(s) if !s.trim().is_empty() => {
                Some(parse_dec(s, &format!("Linia {} adaos %", i + 1))?)
            }
            _ => None,
        };

        let value_cost = round2(qty * unit_cost);

        let (value_adaos, value_tva_neex, pret_amanunt) = if retail_mode {
            if let Some(adaos) = adaos_pct {
                let v_adaos = round2(value_cost * adaos / Decimal::ONE_HUNDRED);
                let baza = value_cost + v_adaos;
                let v_tva = round2(baza * vat_rate / Decimal::ONE_HUNDRED);
                let pret = baza + v_tva;
                (v_adaos, v_tva, pret)
            } else {
                (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO)
            }
        } else {
            (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO)
        };

        computed_lines.push(ComputedLine {
            id: new_id(),
            product_id: ln.product_id.clone().filter(|s| !s.trim().is_empty()),
            denumire: ln.denumire.trim().to_string(),
            um: ln.um.clone().filter(|s| !s.trim().is_empty()),
            qty,
            unit_cost,
            vat_rate,
            adaos_pct,
            value_cost,
            value_adaos,
            value_tva_neex,
            pret_amanunt,
            line_no: ln.line_no,
        });
    }

    // Tranzacție atomică: alocare număr + inserare
    let nir_id = new_id();
    let now = now_unix();

    let mut tx = pool.begin().await?;

    // Citim și incrementăm last_nir_number pe rândul companiei (SELECT cu lock implicit în SQLite)
    let (last_num, nir_series): (i64, Option<String>) =
        sqlx::query_as("SELECT last_nir_number, nir_series FROM companies WHERE id=?1")
            .bind(company_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::NotFound)?;

    let nir_number = last_num + 1;

    sqlx::query("UPDATE companies SET last_nir_number=?1 WHERE id=?2")
        .bind(nir_number)
        .bind(company_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO nir_documents \
         (id, company_id, gestiune_id, received_invoice_id, supplier_name, supplier_cui, \
          nir_series, nir_number, nir_date, retail_mode, status, comisie_receptie, \
          observatii, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'draft',?11,?12,?13)",
    )
    .bind(&nir_id)
    .bind(company_id)
    .bind(&input.gestiune_id)
    .bind(&input.received_invoice_id)
    .bind(&input.supplier_name)
    .bind(&input.supplier_cui)
    .bind(&nir_series)
    .bind(nir_number)
    .bind(&input.nir_date)
    .bind(retail_mode as i64)
    .bind(&input.comisie_receptie)
    .bind(&input.observatii)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for ln in &computed_lines {
        sqlx::query(
            "INSERT INTO nir_lines \
             (id, nir_id, product_id, denumire, um, qty, unit_cost, vat_rate, adaos_pct, \
              value_cost, value_adaos, value_tva_neex, pret_amanunt, line_no) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        )
        .bind(&ln.id)
        .bind(&nir_id)
        .bind(&ln.product_id)
        .bind(&ln.denumire)
        .bind(&ln.um)
        .bind(ln.qty.to_string())
        .bind(ln.unit_cost.to_string())
        .bind(ln.vat_rate.to_string())
        .bind(ln.adaos_pct.map(|d| d.to_string()))
        .bind(ln.value_cost.to_string())
        .bind(ln.value_adaos.to_string())
        .bind(ln.value_tva_neex.to_string())
        .bind(ln.pret_amanunt.to_string())
        .bind(ln.line_no)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Returnăm documentul inserat
    fetch_nir_doc(pool, company_id, &nir_id).await
}

/// Prefill un NirInput din datele unei facturi primite (un rând per grupă TVA).
pub async fn nir_from_received_invoice(
    pool: &SqlitePool,
    company_id: &str,
    received_invoice_id: &str,
) -> AppResult<NirInput> {
    // Verifică că factura aparține companiei
    let inv: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT id, issuer_name, issuer_cui, issue_date \
         FROM received_invoices WHERE id=?1 AND company_id=?2",
    )
    .bind(received_invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let issue_date = inv
        .3
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    // Citim liniile TVA
    let vat_lines: Vec<(String, String)> = sqlx::query_as(
        "SELECT vat_rate, base_amount FROM received_invoice_vat_lines \
         WHERE received_invoice_id=?1 ORDER BY vat_rate",
    )
    .bind(received_invoice_id)
    .fetch_all(pool)
    .await?;

    let lines: Vec<NirLineInput> = vat_lines
        .into_iter()
        .enumerate()
        .map(|(i, (vat_rate, base_amount))| NirLineInput {
            product_id: None,
            denumire: format!("Marfă recepție (TVA {}%)", vat_rate),
            um: Some("buc".into()),
            qty: "1.000000".into(),
            unit_cost: base_amount,
            vat_rate,
            adaos_pct: None,
            line_no: (i + 1) as i64,
        })
        .collect();

    if lines.is_empty() {
        return Err(AppError::Validation(
            "Factura primită nu are linii TVA cu care să se prefill-uiască NIR-ul.".into(),
        ));
    }

    Ok(NirInput {
        gestiune_id: String::new(), // caller must set
        received_invoice_id: Some(received_invoice_id.to_string()),
        supplier_name: Some(inv.1),
        supplier_cui: Some(inv.2),
        nir_date: issue_date,
        retail_mode: Some(false),
        comisie_receptie: None,
        observatii: None,
        lines,
    })
}

/// Finalizează un NIR (status draft → finalized):
/// 1. Curăță orice mișcări de stoc anterioare cu doc_ref=nir_id (idempotență la retry).
/// 2. Pre-validează toate liniile stocabile înainte de a înregistra vreo mișcare.
/// 3. Postează mișcarea de stoc IN pentru fiecare linie cu produs.
/// 4. Postează o singură notă 'NIR' (source_type='NIR') care conține:
///    - dacă standalone (fără factură): D{expense}=Σcost per cont de cheltuială + C408=Σtotal_cost
///      (netează creditele 6xx din STOCK → D{stoc}=C408 net)
///    - dacă retail_mode: D{stoc}=adaos+tva per cont de stoc + C378=Σadaos + C4428=Σtva_neex
///    - dacă ambele: combinate în același apel post_manual_journal
pub async fn finalize_nir(
    pool: &SqlitePool,
    company_id: &str,
    nir_id: &str,
) -> AppResult<NirDocument> {
    // Verifică status
    let doc = fetch_nir_doc(pool, company_id, nir_id).await?;
    if doc.status != "draft" {
        return Err(AppError::Validation("NIR-ul este deja finalizat.".into()));
    }

    // FIX 1(a): Curăță orice mișcări de stoc anterioare cu doc_ref=nir_id
    // (retry-safe: dacă o finalizare anterioară a eșuat la mijloc, nu dublăm stocul).
    // Fiecare rând din stock_ledger are o notă GL 'STOCK' separată, cheie source_id=<id rând>;
    // recompute NU o șterge pe a rândurilor eliminate, deci o ștergem explicit ÎNAINTE — altfel
    // un retry orfanizează nota D{stoc}=C{cheltuială} și dublează partea de stoc în GL.
    let affected: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT product_id, gestiune_id FROM stock_ledger \
         WHERE company_id=?1 AND doc_ref=?2",
    )
    .bind(company_id)
    .bind(nir_id)
    .fetch_all(pool)
    .await?;
    if !affected.is_empty() {
        // Notele GL 'STOCK' ale rândurilor care se șterg (cheie source_id = id rând ledger).
        let stale_ledger_ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2")
                .bind(company_id)
                .bind(nir_id)
                .fetch_all(pool)
                .await?;
        for lid in &stale_ledger_ids {
            sqlx::query(
                "DELETE FROM gl_journal \
                 WHERE company_id=?1 AND source_type='STOCK' AND source_id=?2",
            )
            .bind(company_id)
            .bind(lid)
            .execute(pool)
            .await?;
        }
        sqlx::query("DELETE FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2")
            .bind(company_id)
            .bind(nir_id)
            .execute(pool)
            .await?;
        for (pid, gid) in &affected {
            crate::db::stock_valuation::recompute_product_gestiune(pool, company_id, pid, gid)
                .await?;
        }
    }

    // Citim liniile
    let lines = fetch_nir_lines(pool, nir_id).await?;

    // FIX 1(b): Pre-validare — fail-fast înainte de orice record_movement
    for (i, ln) in lines.iter().enumerate() {
        let product_id = match &ln.product_id {
            Some(pid) if !pid.trim().is_empty() => pid.clone(),
            _ => continue, // fără product_id → omis, nu e eroare
        };
        // Verifică că produsul există și aparține companiei
        let owned: Option<String> =
            sqlx::query_scalar("SELECT id FROM products WHERE id=?1 AND company_id=?2")
                .bind(&product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await?;
        if owned.is_none() {
            return Err(AppError::Validation(format!(
                "Linia {}: produsul '{}' nu există sau nu aparține companiei.",
                i + 1,
                product_id
            )));
        }
        // Verifică că qty și unit_cost sunt Decimal valide
        parse_dec(&ln.qty, &format!("Linia {} cantitate", i + 1))?;
        parse_dec(&ln.unit_cost, &format!("Linia {} cost unitar", i + 1))?;
    }

    // Acumulatori pentru nota NIR combinată
    // standalone: D{expense} = Σcost per cont cheltuială
    let mut standalone_expense: std::collections::HashMap<String, Decimal> =
        std::collections::HashMap::new();
    let mut total_standalone_cost = Decimal::ZERO;
    // linked non-marfă: D{expense}=Σcost + C607=Σ. Factura primită debitează MEREU 607 (post_purchase_invoice
    // e fix pe 607). Pentru un produs non-marfă (301/302/345/381 → cheltuială 601/602/711/608), mișcarea de
    // stoc creditează contul specific, NU 607 → 607 din factură rămâne fantomă. Reclasul D{expense}=C607
    // netează ambele (cheltuiala specifică din mișcare ȘI 607 din factură) → net D{stoc}=C401.
    let mut linked_expense: std::collections::HashMap<String, Decimal> =
        std::collections::HashMap::new();
    let mut total_linked_607 = Decimal::ZERO;
    // retail: D{stoc} = Σ(adaos+tva) per cont stoc
    let mut retail_by_stock: std::collections::HashMap<String, (Decimal, Decimal)> =
        std::collections::HashMap::new(); // stock_account → (adaos, tva_neex)
    let mut total_adaos = Decimal::ZERO;
    let mut total_tva_neex = Decimal::ZERO;

    let is_standalone = doc.received_invoice_id.is_none();
    let mut any_stocked = false;

    for ln in &lines {
        let product_id = match &ln.product_id {
            Some(pid) if !pid.trim().is_empty() => pid.clone(),
            _ => {
                tracing::debug!(
                    "NIR {}: linia '{}' fără product_id — omisă din stoc.",
                    nir_id,
                    ln.denumire
                );
                continue;
            }
        };

        // Tipul produsului — fallback la "marfa" dacă nu găsim
        let product_type: String = sqlx::query_scalar(
            "SELECT COALESCE(product_type, 'marfa') FROM products WHERE id=?1 AND company_id=?2",
        )
        .bind(&product_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_else(|| "marfa".to_string());

        let mapping = resolve_accounts(pool, company_id, &product_type).await?;

        if !mapping.uses_stock {
            tracing::debug!(
                "NIR {}: linia '{}' este serviciu — omisă din stoc.",
                nir_id,
                ln.denumire
            );
            continue;
        }

        // Contul de stoc efectiv al produsului (identic cu ce folosește post_stock_movement)
        let product_stock_account: String = sqlx::query_scalar(
            "SELECT COALESCE(stock_account, '371') FROM products WHERE id=?1 AND company_id=?2",
        )
        .bind(&product_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_else(|| "371".to_string());

        // Contul de cheltuială corespunzător contului de stoc (același calcul ca post_stock_movement)
        let expense_account =
            crate::db::gl::stock_expense_account(&product_stock_account).to_string();

        // Valoarea costului liniei
        let value_cost = parse_dec_zero(&ln.value_cost);

        // Înregistrăm mișcarea de stoc
        let movement = StockMovementInput {
            company_id: company_id.to_string(),
            product_id: product_id.clone(),
            entry_date: doc.nir_date.clone(),
            qty: ln.qty.clone(),
            unit_cost: Some(ln.unit_cost.clone()),
            doc_type: Some("NIR".to_string()),
            doc_ref: Some(nir_id.to_string()),
            gestiune_id: Some(doc.gestiune_id.clone()),
        };
        record_movement(pool, &movement, Dir::In).await?;

        // FIX 2: Acumulăm pentru nota NIR
        if is_standalone {
            // Standalone: trebuie D{expense}=cost + C408=cost în nota NIR
            // (netează C{expense} din STOCK → D{stoc}=C408 net)
            *standalone_expense
                .entry(expense_account.clone())
                .or_insert(Decimal::ZERO) += value_cost;
            total_standalone_cost += value_cost;
        } else if expense_account != "607" {
            // Linked NIR, produs non-marfă: reclasăm cheltuiala specifică pe 607 ca să neteze
            // D607 din factura primită (marfa nu are nevoie — mișcarea creditează deja 607).
            *linked_expense
                .entry(expense_account.clone())
                .or_insert(Decimal::ZERO) += value_cost;
            total_linked_607 += value_cost;
        }

        if doc.retail_mode {
            let v_adaos = parse_dec_zero(&ln.value_adaos);
            let v_tva = parse_dec_zero(&ln.value_tva_neex);
            let entry = retail_by_stock
                .entry(product_stock_account.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += v_adaos;
            entry.1 += v_tva;
            total_adaos += v_adaos;
            total_tva_neex += v_tva;
        }

        any_stocked = true;
    }

    // Construim nota NIR combinată (un singur apel post_manual_journal — DELETE+reinsert atomic)
    if any_stocked {
        let mut owned_lines: Vec<(String, Decimal, Decimal)> = Vec::new();

        // Standalone legs: D{expense}=Σcost + C408=Σtotal_cost
        if is_standalone && total_standalone_cost > Decimal::ZERO {
            for (exp_acct, cost) in &standalone_expense {
                if *cost > Decimal::ZERO {
                    owned_lines.push((exp_acct.clone(), *cost, Decimal::ZERO));
                }
            }
            owned_lines.push(("408".to_string(), Decimal::ZERO, total_standalone_cost));
        }

        // Linked non-marfă legs: D{expense}=Σcost + C607=Σ (netează D607 din factura primită).
        if !is_standalone && total_linked_607 > Decimal::ZERO {
            for (exp_acct, cost) in &linked_expense {
                if *cost > Decimal::ZERO {
                    owned_lines.push((exp_acct.clone(), *cost, Decimal::ZERO));
                }
            }
            owned_lines.push(("607".to_string(), Decimal::ZERO, total_linked_607));
        }

        // Retail legs: D{stoc}=adaos+tva per cont stoc + C378=Σadaos + C4428=Σtva_neex
        if doc.retail_mode && (total_adaos > Decimal::ZERO || total_tva_neex > Decimal::ZERO) {
            for (stock_acct, (adaos, tva)) in &retail_by_stock {
                let total_d = adaos + tva;
                if total_d > Decimal::ZERO {
                    owned_lines.push((stock_acct.clone(), total_d, Decimal::ZERO));
                }
            }
            if total_adaos > Decimal::ZERO {
                owned_lines.push(("378".to_string(), Decimal::ZERO, total_adaos));
            }
            if total_tva_neex > Decimal::ZERO {
                owned_lines.push(("4428".to_string(), Decimal::ZERO, total_tva_neex));
            }
        }

        if !owned_lines.is_empty() {
            let description = format!("Recepție NIR {}", doc.nir_number);
            let lines_ref: Vec<(&str, Decimal, Decimal)> = owned_lines
                .iter()
                .map(|(a, d, c)| (a.as_str(), *d, *c))
                .collect();
            let mj = ManualJournal {
                company_id,
                journal_id: "NIR",
                journal_type: "NIR",
                source_type: "NIR",
                source_id: nir_id,
                date: &doc.nir_date,
                description: &description,
            };
            post_manual_journal(pool, &mj, &lines_ref).await?;
        }
    }

    // Actualizăm status → finalized
    let now = now_unix();
    sqlx::query(
        "UPDATE nir_documents SET status='finalized', finalized_at=?1 \
         WHERE id=?2 AND company_id=?3",
    )
    .bind(now)
    .bind(nir_id)
    .bind(company_id)
    .execute(pool)
    .await?;

    fetch_nir_doc(pool, company_id, nir_id).await
}

/// Returnează un NIR cu liniile sale.
pub async fn get_nir(pool: &SqlitePool, company_id: &str, nir_id: &str) -> AppResult<NirWithLines> {
    let document = fetch_nir_doc(pool, company_id, nir_id).await?;
    let lines = fetch_nir_lines(pool, nir_id).await?;
    Ok(NirWithLines { document, lines })
}

/// Listează toate NIR-urile pentru o companie (cele mai recente primele).
pub async fn list_nir(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<NirDocument>> {
    Ok(sqlx::query_as::<_, NirDocument>(
        "SELECT id, company_id, gestiune_id, received_invoice_id, supplier_name, supplier_cui, \
         nir_series, nir_number, nir_date, retail_mode, status, comisie_receptie, observatii, \
         created_at, finalized_at \
         FROM nir_documents WHERE company_id=?1 \
         ORDER BY nir_date DESC, nir_number DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

// ─── Private helpers ──────────────────────────────────────────────────────────

async fn fetch_nir_doc(
    pool: &SqlitePool,
    company_id: &str,
    nir_id: &str,
) -> AppResult<NirDocument> {
    sqlx::query_as::<_, NirDocument>(
        "SELECT id, company_id, gestiune_id, received_invoice_id, supplier_name, supplier_cui, \
         nir_series, nir_number, nir_date, retail_mode, status, comisie_receptie, observatii, \
         created_at, finalized_at \
         FROM nir_documents WHERE id=?1 AND company_id=?2",
    )
    .bind(nir_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

async fn fetch_nir_lines(pool: &SqlitePool, nir_id: &str) -> AppResult<Vec<NirLine>> {
    Ok(sqlx::query_as::<_, NirLine>(
        "SELECT id, nir_id, product_id, denumire, um, qty, unit_cost, vat_rate, adaos_pct, \
         value_cost, value_adaos, value_tva_neex, pret_amanunt, line_no \
         FROM nir_lines WHERE nir_id=?1 ORDER BY line_no",
    )
    .bind(nir_id)
    .fetch_all(pool)
    .await?)
}

fn parse_dec_zero(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_company(pool: &SqlitePool) -> String {
        let id = new_id();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, trade_name, registry_number, \
             vat_payer, address, city, county, postal_code, country, invoice_series, \
             last_invoice_number, created_at) \
             VALUES (?1,'RO123','Test SRL',NULL,NULL,0,'Str. Test 1','Cluj','CJ','400000','RO','TEST',0,?2)",
        )
        .bind(&id)
        .bind(now_unix())
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn seed_gestiune(pool: &SqlitePool, company_id: &str) -> String {
        let id = new_id();
        sqlx::query(
            "INSERT INTO gestiune (id, company_id, cod, denumire, tip, metoda_evaluare, \
             cont_stoc, is_default, activ, created_at) \
             VALUES (?1,?2,'G1','Gestiune principală','marfuri','CMP','371',1,1,?3)",
        )
        .bind(&id)
        .bind(company_id)
        .bind(now_unix())
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn seed_product(pool: &SqlitePool, company_id: &str, product_type: &str) -> String {
        let id = new_id();
        // Column names per migration 0013_products.sql + 0063_product_types_accounts.sql:
        //   unit (not unit_of_measure), unit_price (not default_price), vat_rate TEXT
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, vat_rate, \
             unit_price, product_type, created_at) \
             VALUES (?1,?2,'Produs test','buc','19','100.00',?3,?4)",
        )
        .bind(&id)
        .bind(company_id)
        .bind(product_type)
        .bind(now_unix())
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn seed_vat_rate(pool: &SqlitePool, rate: &str) {
        // Migrațiile seed vat_rates; verificăm că rata există, dacă nu o adăugăm
        let exists: Option<String> = sqlx::query_scalar("SELECT rate FROM vat_rates WHERE rate=?1")
            .bind(rate)
            .fetch_optional(pool)
            .await
            .unwrap();
        if exists.is_none() {
            sqlx::query("INSERT OR IGNORE INTO vat_rates (id, rate, label, active, created_at) VALUES (?1,?2,?2,1,?3)")
                .bind(new_id())
                .bind(rate)
                .bind(now_unix())
                .execute(pool)
                .await
                .unwrap();
        } else {
            // Ensure active
            sqlx::query("UPDATE vat_rates SET active=1 WHERE rate=?1")
                .bind(rate)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    fn make_input(gestiune_id: &str, product_id: Option<&str>, retail_mode: bool) -> NirInput {
        NirInput {
            gestiune_id: gestiune_id.to_string(),
            received_invoice_id: None,
            supplier_name: Some("Furnizor Test SRL".into()),
            supplier_cui: Some("RO999".into()),
            nir_date: "2026-06-01".into(),
            retail_mode: Some(retail_mode),
            comisie_receptie: Some("Ion Popescu".into()),
            observatii: None,
            lines: vec![NirLineInput {
                product_id: product_id.map(|s| s.to_string()),
                denumire: "Produs test".into(),
                um: Some("buc".into()),
                qty: "10.000000".into(),
                unit_cost: "50.00".into(),
                vat_rate: "19".into(),
                adaos_pct: if retail_mode { Some("20".into()) } else { None },
                line_no: 1,
            }],
        }
    }

    #[tokio::test]
    async fn nir_sequential_numbers() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;

        for expected in 1i64..=3 {
            let doc = create_nir(&pool, &co, make_input(&gest, None, false))
                .await
                .unwrap();
            assert_eq!(doc.nir_number, expected, "nir_number should be {expected}");
        }
    }

    #[tokio::test]
    async fn nir_unique_series_number() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;

        // Creăm un NIR normal
        create_nir(&pool, &co, make_input(&gest, None, false))
            .await
            .unwrap();

        // Setăm o serie non-NULL pe companie pentru a putea testa constrângerea UNIQUE
        // (SQLite: UNIQUE cu NULL nu generează conflict — NULL != NULL)
        sqlx::query("UPDATE companies SET nir_series='NIR' WHERE id=?1")
            .bind(&co)
            .execute(&pool)
            .await
            .unwrap();

        // Inserăm primul NIR cu serie + număr explicit
        let id1 = new_id();
        sqlx::query(
            "INSERT INTO nir_documents (id, company_id, gestiune_id, nir_series, nir_number, \
             nir_date, retail_mode, status, created_at) \
             VALUES (?1,?2,?3,'NIR',99,'2026-06-01',0,'draft',?4)",
        )
        .bind(&id1)
        .bind(&co)
        .bind(&gest)
        .bind(now_unix())
        .execute(&pool)
        .await
        .unwrap();

        // Al doilea INSERT cu aceeași serie+număr trebuie să eșueze (UNIQUE)
        let dup_id = new_id();
        let res = sqlx::query(
            "INSERT INTO nir_documents (id, company_id, gestiune_id, nir_series, nir_number, \
             nir_date, retail_mode, status, created_at) \
             VALUES (?1,?2,?3,'NIR',99,'2026-06-02',0,'draft',?4)",
        )
        .bind(&dup_id)
        .bind(&co)
        .bind(&gest)
        .bind(now_unix())
        .execute(&pool)
        .await;
        assert!(
            res.is_err(),
            "UNIQUE constraint should reject duplicate (series, number)"
        );
    }

    #[tokio::test]
    async fn finalize_gl_balanced() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        let doc = create_nir(&pool, &co, make_input(&gest, Some(&pid), false))
            .await
            .unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // GL trebuie să fie echilibrat: Σ debit == Σ credit
        let ok: bool = sqlx::query_scalar(
            "SELECT ABS(SUM(CAST(debit AS REAL)) - SUM(CAST(credit AS REAL))) < 0.01 \
             FROM gl_entry e JOIN gl_journal j ON j.id=e.journal_pk WHERE j.company_id=?1",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(ok, "GL trebuie să fie echilibrat după finalizare");
    }

    #[tokio::test]
    async fn finalize_371_debited() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        let doc = create_nir(&pool, &co, make_input(&gest, Some(&pid), false))
            .await
            .unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // 371 trebuie să aibă un debit = qty * unit_cost = 10 * 50 = 500
        let debit_371: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='371'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let debit = debit_371.unwrap_or(0.0);
        assert!(
            (debit - 500.0).abs() < 0.01,
            "371 debit trebuie să fie 500, got {debit}"
        );

        // 607 trebuie să aibă un credit (stocul a intrat, cheltuiala anulată)
        let credit_607: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='607'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let credit = credit_607.unwrap_or(0.0);
        assert!(
            (credit - 500.0).abs() < 0.01,
            "607 credit trebuie să fie 500, got {credit}"
        );
    }

    #[tokio::test]
    async fn retail_mode_gl() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        let doc = create_nir(&pool, &co, make_input(&gest, Some(&pid), true))
            .await
            .unwrap();

        // Verificăm că liniile au valori retail
        let lines = fetch_nir_lines(&pool, &doc.id).await.unwrap();
        let ln = &lines[0];
        // value_cost = 10 * 50 = 500; adaos = 500 * 20% = 100; baza = 600; tva = 600*19% = 114; pret = 714
        let vc = parse_dec_zero(&ln.value_cost);
        let va = parse_dec_zero(&ln.value_adaos);
        let vt = parse_dec_zero(&ln.value_tva_neex);
        let pa = parse_dec_zero(&ln.pret_amanunt);
        assert!(
            (vc - Decimal::from(500)).abs() < Decimal::new(1, 2),
            "value_cost={vc}"
        );
        assert!(
            (va - Decimal::from(100)).abs() < Decimal::new(1, 2),
            "value_adaos={va}"
        );
        assert!(
            (vt - Decimal::from(114)).abs() < Decimal::new(1, 2),
            "value_tva_neex={vt}"
        );
        assert!(
            (pa - Decimal::from(714)).abs() < Decimal::new(1, 2),
            "pret_amanunt={pa}"
        );

        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // Verific că 378 și 4428 au credite
        let credit_378: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='378'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let c378 = credit_378.unwrap_or(0.0);
        assert!((c378 - 100.0).abs() < 0.01, "378 credit={c378}");

        let credit_4428: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='4428'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let c4428 = credit_4428.unwrap_or(0.0);
        assert!((c4428 - 114.0).abs() < 0.01, "4428 credit={c4428}");
    }

    #[tokio::test]
    async fn vat_rate_validation_rejects_inactive() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;

        // Inserăm o rată inactivă
        sqlx::query(
            "INSERT OR IGNORE INTO vat_rates (id, rate, label, active, created_at) VALUES (?1,'99','99%',0,?2)",
        )
        .bind(new_id())
        .bind(now_unix())
        .execute(&pool)
        .await
        .unwrap();

        let mut input = make_input(&gest, None, false);
        input.lines[0].vat_rate = "99".into();

        let res = create_nir(&pool, &co, input).await;
        assert!(res.is_err(), "Cotă TVA inactivă trebuie respinsă");
    }

    #[tokio::test]
    async fn service_line_skipped() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "serviciu").await;

        let doc = create_nir(&pool, &co, make_input(&gest, Some(&pid), false))
            .await
            .unwrap();
        // Finalizarea nu trebuie să eșueze — serviciul e sărit
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // Nicio mișcare de stoc nu trebuie să existe
        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1")
                .bind(&co)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ledger_count, 0, "Serviciu nu trebuie să genereze stoc");
    }

    #[tokio::test]
    async fn no_product_line_skipped() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;

        // Linie fără product_id
        let doc = create_nir(&pool, &co, make_input(&gest, None, false))
            .await
            .unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        let ledger_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1")
                .bind(&co)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            ledger_count, 0,
            "Linie fără produs nu trebuie să genereze stoc"
        );
    }

    #[tokio::test]
    async fn idempotent_retail_journal() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        let doc = create_nir(&pool, &co, make_input(&gest, Some(&pid), true))
            .await
            .unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // Re-postăm manual nota NIR (simulând idempotența post_manual_journal)
        let total_adaos = Decimal::from(100);
        let total_tva_neex = Decimal::from(114);
        let total_d = total_adaos + total_tva_neex;
        let lines: Vec<(&str, Decimal, Decimal)> = vec![
            ("371", total_d, Decimal::ZERO),
            ("378", Decimal::ZERO, total_adaos),
            ("4428", Decimal::ZERO, total_tva_neex),
        ];
        let mj = ManualJournal {
            company_id: &co,
            journal_id: "NIR",
            journal_type: "NIR",
            source_type: "NIR",
            source_id: &doc.id,
            date: "2026-06-01",
            description: "Re-post test",
        };
        post_manual_journal(&pool, &mj, &lines).await.unwrap();

        // Trebuie să existe exact un jurnal cu source_type='NIR' și source_id=nir_id
        let journal_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE source_type='NIR' AND source_id=?1",
        )
        .bind(&doc.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            journal_count, 1,
            "Nota NIR trebuie să fie idempotentă (un singur jurnal)"
        );
    }

    #[tokio::test]
    async fn gestiune_required() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        seed_vat_rate(&pool, "19").await;

        let mut input = make_input("", None, false);
        input.gestiune_id = String::new();
        let res = create_nir(&pool, &co, input).await;
        assert!(res.is_err(), "gestiune_id gol trebuie respins");
    }

    // ─── New FIX tests ────────────────────────────────────────────────────────

    /// FIX 2 test: NIR standalone (fără factură primită) trebuie să posteze C408=Σcost
    /// și contul de cheltuială (607) să fie net zero (D din NIR - C din STOCK = 0).
    /// Net: D371=C408.
    #[tokio::test]
    async fn standalone_posts_408() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        // NIR standalone (received_invoice_id=None)
        let mut input = make_input(&gest, Some(&pid), false);
        input.received_invoice_id = None;
        let doc = create_nir(&pool, &co, input).await.unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // C408 trebuie să existe cu suma = value_cost = 10 * 50 = 500
        let credit_408: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='408'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let c408 = credit_408.unwrap_or(0.0);
        assert!(
            (c408 - 500.0).abs() < 0.01,
            "408 credit trebuie să fie 500 (recepție fără factură), got {c408}"
        );

        // 607 trebuie să fie net zero: D607 (din NIR journal) - C607 (din STOCK journal) = 0
        let debit_607: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='607'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let credit_607: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='607'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let net_607 = debit_607.unwrap_or(0.0) - credit_607.unwrap_or(0.0);
        assert!(
            net_607.abs() < 0.01,
            "607 trebuie să fie net zero (D={} - C={} = {})",
            debit_607.unwrap_or(0.0),
            credit_607.unwrap_or(0.0),
            net_607
        );

        // Net: D371 == C408
        let debit_371: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='371'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let d371 = debit_371.unwrap_or(0.0);
        assert!(
            (d371 - 500.0).abs() < 0.01,
            "371 debit trebuie să fie 500, got {d371}"
        );
    }

    /// FIX 2 test: NIR linked (cu factură primită) → 607 nu trebuie să fie net zero din NIR singur,
    /// dar dacă adăugăm și GL-ul facturii (D607=C401), net 607 = 0 și 371=C401.
    #[tokio::test]
    async fn golden_net_out_linked() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        // Seed a received_invoice row (required by FK on nir_documents.received_invoice_id)
        let inv_id = new_id();
        sqlx::query(
            "INSERT INTO received_invoices (id, company_id, anaf_download_id, issuer_cui, \
             issuer_name, total_amount, currency, issue_date, xml_path) \
             VALUES (?1,?2,'DL-001','RO999','Furnizor Test',500.0,'RON','2026-06-01','/tmp/inv.xml')",
        )
        .bind(&inv_id)
        .bind(&co)
        .execute(&pool)
        .await
        .unwrap();

        // Simulăm GL-ul facturii primite: D607=C401 pentru valoarea 500
        // (ce ar posta o factură primită reală)
        let inv_journal_id = new_id();
        let gl_entry_id1 = new_id();
        let gl_entry_id2 = new_id();
        sqlx::query(
            "INSERT INTO gl_journal (id, company_id, journal_id, journal_type, transaction_id, \
             transaction_date, description, source_type, source_id, customer_id, supplier_id) \
             VALUES (?1,?2,'FAC','RECEIVED_INVOICE',?3,'2026-06-01','Factură primită','RECEIVED_INVOICE',?3,NULL,NULL)",
        )
        .bind(&inv_journal_id)
        .bind(&co)
        .bind(&inv_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit, \
             partner_cui, customer_id, supplier_id, tax_type, tax_code) \
             VALUES (?1,?2,1,'607','500.00','0.00',NULL,NULL,NULL,'000','000000')",
        )
        .bind(&gl_entry_id1)
        .bind(&inv_journal_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit, \
             partner_cui, customer_id, supplier_id, tax_type, tax_code) \
             VALUES (?1,?2,2,'401','0.00','500.00',NULL,NULL,NULL,'000','000000')",
        )
        .bind(&gl_entry_id2)
        .bind(&inv_journal_id)
        .execute(&pool)
        .await
        .unwrap();

        // NIR linked la factura primită
        let mut input = make_input(&gest, Some(&pid), false);
        input.received_invoice_id = Some(inv_id.clone());
        let doc = create_nir(&pool, &co, input).await.unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // NIR linked NU trebuie să posteze o notă 'NIR' (nu există legs suplimentare)
        let nir_journal_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id=?1 AND source_type='NIR'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            nir_journal_count, 0,
            "NIR linked nu trebuie să posteze jurnal NIR"
        );

        // Net 607: D607 (din RECEIVED_INVOICE=500) - C607 (din STOCK=500) = 0
        let debit_607: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='607'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let credit_607: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='607'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        let net_607 = debit_607.unwrap_or(0.0) - credit_607.unwrap_or(0.0);
        assert!(
            net_607.abs() < 0.01,
            "607 net trebuie să fie zero în scenariul linked, got {net_607}"
        );

        // D371 == 500
        let debit_371: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='371'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            (debit_371.unwrap_or(0.0) - 500.0).abs() < 0.01,
            "371 debit trebuie să fie 500"
        );

        // C401 == 500
        let credit_401: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='401'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            (credit_401.unwrap_or(0.0) - 500.0).abs() < 0.01,
            "401 credit trebuie să fie 500"
        );
    }

    /// Regression (audit): a LINKED NIR for a NON-marfă product (stock 301 → expense 601) must reclass
    /// the cost to 607 so the received invoice's hardcoded D607 nets to ZERO (no phantom 607). marfă
    /// nets via the movement's C607; non-marfă needs the D601=C607 leg. Asserts across ALL journals:
    /// 607 net 0, 601 net 0, 301 debit = cost, 401 credit = cost → net D301=C401.
    #[tokio::test]
    async fn linked_non_marfa_reclasses_607() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        // materie_primă product with stock_account 301 (→ expense 601).
        let pid = new_id();
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, vat_rate, unit_price, product_type, \
             stock_account, created_at) \
             VALUES (?1,?2,'Materie','buc','19','50.00','materie_prima','301',?3)",
        )
        .bind(&pid)
        .bind(&co)
        .bind(now_unix())
        .execute(&pool)
        .await
        .unwrap();

        // Received invoice + its GL D607=C401 (500) — what a real received invoice posts.
        let inv_id = new_id();
        sqlx::query(
            "INSERT INTO received_invoices (id, company_id, anaf_download_id, issuer_cui, \
             issuer_name, total_amount, currency, issue_date, xml_path) \
             VALUES (?1,?2,'DL-002','RO999','Furnizor',500.0,'RON','2026-06-01','/tmp/i.xml')",
        )
        .bind(&inv_id)
        .bind(&co)
        .execute(&pool)
        .await
        .unwrap();
        let invj = new_id();
        sqlx::query(
            "INSERT INTO gl_journal (id, company_id, journal_id, journal_type, transaction_id, \
             transaction_date, description, source_type, source_id, customer_id, supplier_id) \
             VALUES (?1,?2,'FAC','RECEIVED_INVOICE',?3,'2026-06-01','Factură','RECEIVED_INVOICE',?3,NULL,NULL)",
        )
        .bind(&invj)
        .bind(&co)
        .bind(&inv_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit, \
             partner_cui, customer_id, supplier_id, tax_type, tax_code) \
             VALUES (?1,?2,1,'607','500.00','0.00',NULL,NULL,NULL,'000','000000')",
        )
        .bind(new_id())
        .bind(&invj)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gl_entry (id, journal_pk, record_id, account_code, debit, credit, \
             partner_cui, customer_id, supplier_id, tax_type, tax_code) \
             VALUES (?1,?2,2,'401','0.00','500.00',NULL,NULL,NULL,'000','000000')",
        )
        .bind(new_id())
        .bind(&invj)
        .execute(&pool)
        .await
        .unwrap();

        // Linked NIR (line cost 10×50 = 500, matching the invoice's 607).
        let mut input = make_input(&gest, Some(&pid), false);
        input.received_invoice_id = Some(inv_id.clone());
        let doc = create_nir(&pool, &co, input).await.unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
            "SELECT e.account_code, SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
             FROM gl_entry e JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code IN ('607','601','301','401') \
             GROUP BY e.account_code",
        )
        .bind(&co)
        .fetch_all(&pool)
        .await
        .unwrap();
        let mut net: std::collections::HashMap<String, (f64, f64)> =
            std::collections::HashMap::new();
        for (acct, d, c) in rows {
            net.insert(acct, (d.unwrap_or(0.0), c.unwrap_or(0.0)));
        }
        let dc = |a: &str| net.get(a).copied().unwrap_or((0.0, 0.0));
        let (d607, c607) = dc("607");
        let (d601, c601) = dc("601");
        let (d301, _) = dc("301");
        let (_, c401) = dc("401");
        assert!(
            (d607 - c607).abs() < 0.01,
            "607 net must be 0 (invoice D607 reclassed away), got {}",
            d607 - c607
        );
        assert!(
            (d601 - c601).abs() < 0.01,
            "601 net must be 0 (movement C601 reclassed to 607), got {}",
            d601 - c601
        );
        assert!(
            (d301 - 500.0).abs() < 0.01,
            "301 debit must be 500, got {d301}"
        );
        assert!(
            (c401 - 500.0).abs() < 0.01,
            "401 credit must be 500, got {c401}"
        );
    }

    /// FIX 1 test: finalizare idempotentă după un partial (simulăm o mișcare de stoc
    /// cu doc_ref=nir_id inserată manual înainte de finalizare → finalize trebuie să
    /// curețe înregistrarea anterioară și să nu dubleze stocul).
    #[tokio::test]
    async fn idempotent_refinalize_after_partial() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;
        let pid = seed_product(&pool, &co, "marfa").await;

        let input = make_input(&gest, Some(&pid), false);
        let doc = create_nir(&pool, &co, input).await.unwrap();

        // Simulăm un partial REAL: rulăm record_movement o dată — postează ȘI nota GL 'STOCK'
        // (D371=C607, cheie source_id=<id rând ledger>), exact ca un finalize întrerupt la mijloc.
        crate::db::stock_valuation::record_movement(
            &pool,
            &crate::db::stock_valuation::StockMovementInput {
                company_id: co.clone(),
                product_id: pid.clone(),
                entry_date: "2026-06-01".to_string(),
                qty: "10".to_string(),
                unit_cost: Some("50.00".to_string()),
                doc_type: Some("NIR".to_string()),
                doc_ref: Some(doc.id.clone()),
                gestiune_id: Some(gest.clone()),
            },
            crate::db::stock_valuation::Dir::In,
        )
        .await
        .unwrap();

        // NIR rămâne 'draft' — finalizăm acum
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // Stocul trebuie să fie exact 10 (nu 20) deoarece cleanup-ul a șters înregistrarea parțială
        let stock_qty: Option<String> =
            sqlx::query_scalar("SELECT stock_qty FROM products WHERE id=?1 AND company_id=?2")
                .bind(&pid)
                .bind(&co)
                .fetch_one(&pool)
                .await
                .unwrap();
        let qty = Decimal::from_str(stock_qty.as_deref().unwrap_or("0")).unwrap_or(Decimal::ZERO);
        assert!(
            (qty - Decimal::from(10)).abs() < Decimal::new(1, 3),
            "stock_qty trebuie să fie 10 (nu dublat), got {qty}"
        );

        // Numărul de înregistrări în stock_ledger trebuie să fie exact 1 (nu 2)
        let ledger_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2",
        )
        .bind(&co)
        .bind(&doc.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            ledger_count, 1,
            "stock_ledger trebuie să aibă exact 1 înregistrare (nu 2 după retry)"
        );

        // GL: nota 'STOCK' orfană a partialului trebuie ștearsă de cleanup → debit 371 total == 500
        // (nu 1000 dublat). Acesta e exact scenariul mascat anterior de partialul 'MANUAL' fără GL.
        let debit_371: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='371'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            (debit_371.unwrap_or(0.0) - 500.0).abs() < 0.01,
            "debit 371 total trebuie să fie 500 (nota STOCK orfană ștearsă, nu dublată), got {debit_371:?}"
        );
    }

    /// FIX 2 retail test: NIR retail cu un produs 371 și un produs 301 → adaos/TVA
    /// rutate per cont de stoc, nu toate la un singur cont.
    #[tokio::test]
    async fn retail_mixed_stock_accounts() {
        let pool = test_pool().await;
        let co = seed_company(&pool).await;
        let gest = seed_gestiune(&pool, &co).await;
        seed_vat_rate(&pool, "19").await;

        // Produs marfă (371) și materie primă (301)
        let pid_marfa = seed_product(&pool, &co, "marfa").await;
        let pid_materie = seed_product(&pool, &co, "materie_prima").await;
        // Setăm explicit stock_account pe materie primă
        sqlx::query("UPDATE products SET stock_account='301' WHERE id=?1 AND company_id=?2")
            .bind(&pid_materie)
            .bind(&co)
            .execute(&pool)
            .await
            .unwrap();

        let nir_input = NirInput {
            gestiune_id: gest.clone(),
            received_invoice_id: None,
            supplier_name: Some("Furnizor Mix SRL".into()),
            supplier_cui: Some("RO777".into()),
            nir_date: "2026-06-01".into(),
            retail_mode: Some(true),
            comisie_receptie: None,
            observatii: None,
            lines: vec![
                NirLineInput {
                    product_id: Some(pid_marfa.clone()),
                    denumire: "Marfă 371".into(),
                    um: Some("buc".into()),
                    qty: "10.000000".into(),
                    unit_cost: "50.00".into(),
                    vat_rate: "19".into(),
                    adaos_pct: Some("20".into()),
                    line_no: 1,
                },
                NirLineInput {
                    product_id: Some(pid_materie.clone()),
                    denumire: "Materie 301".into(),
                    um: Some("kg".into()),
                    qty: "5.000000".into(),
                    unit_cost: "100.00".into(),
                    vat_rate: "19".into(),
                    adaos_pct: Some("10".into()),
                    line_no: 2,
                },
            ],
        };

        let doc = create_nir(&pool, &co, nir_input).await.unwrap();
        finalize_nir(&pool, &co, &doc.id).await.unwrap();

        // Verificăm că nota NIR există și este echilibrată
        let ok: bool = sqlx::query_scalar(
            "SELECT ABS(SUM(CAST(debit AS REAL)) - SUM(CAST(credit AS REAL))) < 0.01 \
             FROM gl_entry e JOIN gl_journal j ON j.id=e.journal_pk WHERE j.company_id=?1",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(ok, "GL trebuie să fie echilibrat după NIR retail mixt");

        // Trebuie să existe credit pe 378 și 4428
        let c378: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND e.account_code='378'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            c378.unwrap_or(0.0) > 0.0,
            "378 trebuie să aibă credit (adaos comercial)"
        );

        // Trebuie să existe debit pe 371 (marfă) în nota NIR (retail leg)
        let d371_nir: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND j.source_type='NIR' AND e.account_code='371'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            d371_nir.unwrap_or(0.0) > 0.0,
            "371 trebuie să aibă debit în nota NIR pentru leg-ul de adaos marfă"
        );

        // Trebuie să existe debit pe 301 în nota NIR (retail leg materie primă)
        let d301_nir: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(CAST(debit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND j.source_type='NIR' AND e.account_code='301'",
        )
        .bind(&co)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            d301_nir.unwrap_or(0.0) > 0.0,
            "301 trebuie să aibă debit în nota NIR pentru leg-ul de adaos materie primă"
        );
    }
}
