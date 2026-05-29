# REAL→TEXT Money Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate all monetary DB columns from REAL (f64) to TEXT (Decimal string) to eliminate float precision loss on invoice amounts, SAF-T tax filings, and payment reconciliation.

**Architecture:** SQLite does not support ALTER COLUMN TYPE, so each table is rebuilt via CREATE→INSERT→DROP→RENAME in a single migration. Rust struct fields change from `f64` to `String`; all callers parse via `Decimal::from_str()`. TypeScript types change `number` → `string` for amount fields; frontend arithmetic uses `parseFloat()`.

**Tech Stack:** Tauri 2.0, Rust, sqlx (dynamic queries, no macros), rust_decimal, React 19 + TypeScript, SQLite

**Critical Constraints:**
- AppState field is `.db` (NOT `.pool`)
- Money ONLY via `rust_decimal::Decimal` — NEVER f64/f32
- SQL placeholders `?1 ?2` — never interpolate user values
- IDs via `crate::db::models::new_id()` (UUIDv7)
- NO `query!` macro — `query().bind().fetch_*()` + `try_get()` only

---

## IMPORTANT — Pre-flight verification (read before Task 1)

During plan authoring, a tooling outage prevented re-reading the raw SQL of `0001_initial.sql` and the bodies of the `commands/*`, `ubl/*`, `background/mod.rs`, and frontend `.tsx`/`.ts` files in this session. The **Rust DB layer was read verbatim** and is authoritative for column names, ordering, and current types. Everything below derived from the DB layer is exact. A small number of items (exact CREATE TABLE constraint clauses, exact index names, and the precise call sites inside commands/ubl/frontend) are marked **[VERIFY]** with a grep command. The implementing worker MUST run each **[VERIFY]** grep and reconcile before editing that file. Do not skip the greps — they take seconds and prevent guessing.

Authoritative facts established by verbatim reads:
- `db/invoices.rs` is the **dynamic-WHERE-builder** version. It declares column lists in three module-level consts: `SELECT_INVOICE`, `SELECT_LINE`, `SELECT_EVENT`. All SELECTs reference these consts. **No SELECT enumerates columns inline anymore** — so changing read types is localized to struct fields + the `create()` body.
- `db/payments.rs` is **already fully migrated to TEXT** (`amount: String`, parsed via `Decimal::from_str`). It is the **canonical reference pattern** for this entire migration — copy its idioms (`use rust_decimal::Decimal; use std::str::FromStr; use sqlx::Row;` + `row.try_get::<String,_>` + accumulate in `Decimal`). `payments.amount` does NOT need migration; only the three header/line tables do.
- `db/recurring.rs` has **no monetary columns**. Amounts are embedded in `lines_json: String` (already text). Recurring needs changes only where it *generates* an invoice via the invoices create path (see Task 5).
- Migrations `0001`–`0004` exist. Next free filename per the task spec is `006_amounts_to_text.sql`. **[VERIFY] `ls src-tauri/migrations/`** — if a `0005_*` already exists, keep `006`; if nothing between, `006` still sorts last and is fine. (sqlx sorts lexicographically; `006_` > `0004_`.)

---

## File Map

| File | Change |
|------|--------|
| `src-tauri/migrations/006_amounts_to_text.sql` | **NEW.** Rebuild `invoices`, `invoice_line_items`, `received_invoices` with TEXT money columns; backfill via `printf('%.2f', col)`; recreate indexes. |
| `src-tauri/src/db/invoices.rs` | `Invoice`: `subtotal_amount/vat_amount/total_amount` `f64`→`String` (keep `exchange_rate: Option<f64>` — FX rate, not money). `LineItem`: `quantity/unit_price/vat_rate/subtotal_amount/vat_amount/total_amount` `f64`→`String`. Add `parse_dec` helper. `create()`: stop converting Decimals to f64; bind `.round_dp(2).to_string()`. `line_rows` tuple type `(String,f64,f64,f64)`→`(String,String,String,String)`. Line-item binds for `quantity/unit_price/vat_rate` bind `Decimal::…to_string()`. `CreateInvoiceInput`/`CreateLineInput` inputs stay `f64` (wire format from frontend numbers) — see note. |
| `src-tauri/src/db/received.rs` | `ReceivedInvoice.total_amount` `f64`→`String`. `CreateReceivedInput.total_amount` `f64`→`String`. `create()` bind: validate via `Decimal::from_str` then `.bind(&input.total_amount)` (already a String). |
| `src-tauri/src/db/payments.rs` | **No change** (already TEXT). Reference only. |
| `src-tauri/src/db/recurring.rs` | **No DB-field change** (no money columns). |
| `src-tauri/src/commands/invoices.rs` | **[VERIFY]** create/update/storno amount binds: any `.to_f64()` / `as f64` on Decimal → `.round_dp(2).to_string()`. Any `try_get::<f64,_>` on amount cols → `try_get::<String,_>`. |
| `src-tauri/src/commands/reports.rs` | **[VERIFY]** Replace any SQL `SUM(total_amount)`/`SUM(vat_amount)` etc. on now-TEXT columns: fetch rows as `String`, fold into `Decimal` in Rust. |
| `src-tauri/src/commands/saft.rs` | **[VERIFY]** All `try_get::<f64,_>` on amount columns → `try_get::<String,_>` then `Decimal::from_str`. |
| `src-tauri/src/commands/integrations.rs` | **[VERIFY]** XLSX export: read amount as `String`→`Decimal`→`f64` only at the `write_number` call (rust_xlsxwriter needs f64 for numeric cells). |
| `src-tauri/src/commands/import.rs` | **[VERIFY]** CSV/XML import: ensure parsed amounts are stored as `String` (Decimal→string) into the new TEXT columns. |
| `src-tauri/src/ubl/generator.rs` | **[VERIFY]** `fmt_amount(v: f64)`→`fmt_amount(s: &str) -> String` (parse Decimal, `format!("{:.2}", d)`); update callers to pass the String fields. |
| `src-tauri/src/ubl/pdf.rs` | **[VERIFY]** Replace `inv.total_amount as f64` / f64 field reads with `Decimal::from_str(&field)` then format. |
| `src-tauri/src/ubl/rocius_rules.rs` | **[VERIFY]** `f64_to_dec` helper callers: parse from `String` directly via `Decimal::from_str`. |
| `src-tauri/src/ubl/validator.rs` | **[VERIFY]** Update amount field access to parse String→Decimal. |
| `src-tauri/src/background/mod.rs` | **[VERIFY]** Recurring generation: amount binds f64→String (or route through `db::invoices::create`, which already handles it). |
| `src/types/index.ts` | `Invoice.{subtotalAmount,vatAmount,totalAmount}` `number`→`string`. `InvoiceLine.{quantity,unitPrice,vatRate,subtotalAmount,vatAmount,totalAmount}` `number`→`string`. `ReceivedInvoice.{subtotalAmount?,vatAmount?,totalAmount}` `number`→`string`. **[VERIFY]** exact interface/field names. |
| `src/lib/utils.ts` | Add `export const parseDec = (s: string | number): number => parseFloat(String(s)) || 0;` **[VERIFY]** file exists; if not, create it (and ensure imports resolve). |
| `src/lib/tauri.ts` | **[VERIFY]** any client-side typing/casting of amount fields. |
| `src/pages/Dashboard.tsx` | **[VERIFY]** wrap amount arithmetic/sums with `parseDec(...)`. |
| `src/pages/Reports.tsx` | **[VERIFY]** wrap amount arithmetic/sums with `parseDec(...)`. |
| `src/pages/InvoiceDetail.tsx` | **[VERIFY]** wrap amount arithmetic/formatting with `parseDec(...)`. |
| `src/pages/Payments.tsx` | **[VERIFY]** already string-based for payment amounts; reconcile invoice total math with `parseDec(...)`. |

**Files needing changes: 21** (1 new migration + 4 DB-layer/utility-confirmed + 16 [VERIFY] call-site files). `db/payments.rs` and `db/recurring.rs` are read/reference only and excluded from the count.

---

## Task 1: SQL Migration 006

SQLite cannot `ALTER COLUMN TYPE`. For each table: CREATE `_new` with TEXT money columns → INSERT…SELECT with `printf('%.2f', col)` backfill → DROP old → RENAME → recreate indexes.

> **[VERIFY] before writing SQL — capture the exact current schema so the `_new` table is byte-identical except for the money column types, and so every index/FK/constraint is reproduced:**
> ```bash
> grep -nE "CREATE (TABLE|INDEX)|REAL|NOT NULL|DEFAULT|FOREIGN KEY|REFERENCES|PRIMARY KEY" \
>   src-tauri/migrations/0001_initial.sql
> # also check later migrations that may have altered these tables:
> grep -nE "invoices|invoice_line_items|received_invoices|ALTER|CREATE INDEX" \
>   src-tauri/migrations/0002_payment_means.sql \
>   src-tauri/migrations/0003_payments_recurring.sql \
>   src-tauri/migrations/0004_payments_recurring_fk.sql
> ```
> The **column set and order** below is authoritative (taken verbatim from `SELECT_INVOICE`, `SELECT_LINE`, and the `received_invoices` SELECT in the Rust DB layer). Only the **constraint clauses** (NOT NULL / DEFAULT / FK / PK) must be copied from the grep output — fill them into the templates below. Keep `exchange_rate` as REAL (it is an FX rate, not a monetary amount).

Money columns to convert to TEXT:
- `invoices`: `subtotal_amount`, `vat_amount`, `total_amount`  *(keep `exchange_rate` REAL)*
- `invoice_line_items`: `quantity`, `unit_price`, `vat_rate`, `subtotal_amount`, `vat_amount`, `total_amount`
- `received_invoices`: `total_amount`

`invoices` column order (from `SELECT_INVOICE`):
`id, company_id, contact_id, series, number, full_number, issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at, xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes, payment_means_code, created_at, updated_at`

`invoice_line_items` column order (from `SELECT_LINE`):
`id, invoice_id, position, name, description, quantity, unit, unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, cpv_code`

`received_invoices` column order (from `received.rs` SELECT):
`id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, series, number, total_amount, currency, issue_date, xml_path, pdf_path, status, downloaded_at, created_at`

**Migration template** (fill `<…constraints…>` from the [VERIFY] grep; `PRAGMA foreign_keys=OFF` brackets the rebuild so child-table FKs don't cascade on DROP):

```sql
-- 006_amounts_to_text.sql
-- Migrate monetary columns REAL(f64) -> TEXT(Decimal string).
-- printf('%.2f', col) normalizes existing floats to 2dp strings.

PRAGMA foreign_keys=OFF;

-- ── invoices ────────────────────────────────────────────────────────────────
CREATE TABLE invoices_new (
    id                 TEXT PRIMARY KEY,
    company_id         TEXT NOT NULL,            -- <verify FK REFERENCES companies(id)>
    contact_id         TEXT NOT NULL,            -- <verify FK REFERENCES contacts(id)>
    series             TEXT NOT NULL,
    number             INTEGER NOT NULL,
    full_number        TEXT NOT NULL,
    issue_date         TEXT NOT NULL,
    due_date           TEXT NOT NULL,
    currency           TEXT NOT NULL,            -- <verify DEFAULT 'RON'>
    exchange_rate      REAL,                     -- KEEP REAL (FX rate, not money)
    subtotal_amount    TEXT NOT NULL,            -- was REAL
    vat_amount         TEXT NOT NULL,            -- was REAL
    total_amount       TEXT NOT NULL,            -- was REAL
    status             TEXT NOT NULL,            -- <verify DEFAULT 'DRAFT'>
    anaf_upload_id     TEXT,
    anaf_index         TEXT,
    anaf_submitted_at  INTEGER,
    anaf_validated_at  INTEGER,
    anaf_rejected_at   INTEGER,
    xml_path           TEXT,
    pdf_path           TEXT,
    signature_xml_path TEXT,
    rejection_reason   TEXT,
    rejection_code     TEXT,
    notes              TEXT,
    payment_means_code TEXT NOT NULL,            -- <verify DEFAULT '30'>
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
    -- <verify: copy any FOREIGN KEY clauses here>
);

INSERT INTO invoices_new
SELECT
    id, company_id, contact_id, series, number, full_number,
    issue_date, due_date, currency, exchange_rate,
    printf('%.2f', subtotal_amount) AS subtotal_amount,
    printf('%.2f', vat_amount)      AS vat_amount,
    printf('%.2f', total_amount)    AS total_amount,
    status, anaf_upload_id, anaf_index, anaf_submitted_at, anaf_validated_at, anaf_rejected_at,
    xml_path, pdf_path, signature_xml_path, rejection_reason, rejection_code, notes,
    payment_means_code, created_at, updated_at
FROM invoices;

DROP TABLE invoices;
ALTER TABLE invoices_new RENAME TO invoices;

-- <verify: recreate every index that existed on invoices, e.g.>
-- CREATE INDEX idx_invoices_company ON invoices(company_id);
-- CREATE INDEX idx_invoices_status  ON invoices(status);
-- CREATE INDEX idx_invoices_issue   ON invoices(issue_date);

-- ── invoice_line_items ───────────────────────────────────────────────────────
CREATE TABLE invoice_line_items_new (
    id              TEXT PRIMARY KEY,
    invoice_id      TEXT NOT NULL,               -- <verify FK REFERENCES invoices(id) ON DELETE CASCADE>
    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    quantity        TEXT NOT NULL,               -- was REAL
    unit            TEXT NOT NULL,
    unit_price      TEXT NOT NULL,               -- was REAL
    vat_rate        TEXT NOT NULL,               -- was REAL
    vat_category    TEXT NOT NULL,
    subtotal_amount TEXT NOT NULL,               -- was REAL
    vat_amount      TEXT NOT NULL,               -- was REAL
    total_amount    TEXT NOT NULL,               -- was REAL
    cpv_code        TEXT
    -- <verify: copy FOREIGN KEY clause(s)>
);

INSERT INTO invoice_line_items_new
SELECT
    id, invoice_id, position, name, description,
    printf('%.2f', quantity)   AS quantity,
    unit,
    printf('%.2f', unit_price) AS unit_price,
    printf('%.2f', vat_rate)   AS vat_rate,
    vat_category,
    printf('%.2f', subtotal_amount) AS subtotal_amount,
    printf('%.2f', vat_amount)      AS vat_amount,
    printf('%.2f', total_amount)    AS total_amount,
    cpv_code
FROM invoice_line_items;

DROP TABLE invoice_line_items;
ALTER TABLE invoice_line_items_new RENAME TO invoice_line_items;

-- <verify: recreate indexes, e.g.>
-- CREATE INDEX idx_line_items_invoice ON invoice_line_items(invoice_id);

-- ── received_invoices ────────────────────────────────────────────────────────
CREATE TABLE received_invoices_new (
    id               TEXT PRIMARY KEY,
    company_id       TEXT NOT NULL,              -- <verify FK>
    anaf_download_id TEXT NOT NULL,
    anaf_index       TEXT,
    issuer_cui       TEXT NOT NULL,
    issuer_name      TEXT NOT NULL,
    series           TEXT,
    number           TEXT,
    total_amount     TEXT NOT NULL,              -- was REAL
    currency         TEXT NOT NULL,
    issue_date       TEXT NOT NULL,
    xml_path         TEXT NOT NULL,
    pdf_path         TEXT,
    status           TEXT NOT NULL,
    downloaded_at    INTEGER NOT NULL,
    created_at       INTEGER NOT NULL
    -- <verify: copy FOREIGN KEY clause(s)>
);

INSERT INTO received_invoices_new
SELECT
    id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name,
    series, number,
    printf('%.2f', total_amount) AS total_amount,
    currency, issue_date, xml_path, pdf_path, status, downloaded_at, created_at
FROM received_invoices;

DROP TABLE received_invoices;
ALTER TABLE received_invoices_new RENAME TO received_invoices;

-- <verify: recreate indexes, e.g.>
-- CREATE INDEX idx_received_company ON received_invoices(company_id);
-- CREATE INDEX idx_received_status  ON received_invoices(status);

PRAGMA foreign_keys=ON;
```

> **NOTE on `quantity`/`vat_rate`:** `printf('%.2f', …)` forces 2dp. Quantities like `1` become `"1.00"` and VAT rate `19` becomes `"19.00"`. This is acceptable (Decimal parses both; UBL/PDF reformatting controls display). If business rules require >2dp quantities, change those three `printf('%.2f', quantity)` to `CAST(quantity AS TEXT)` instead — **[VERIFY] desired precision with quantity usage in `ubl/generator.rs`** before finalizing.

Steps:
- [ ] Run the [VERIFY] grep commands; record exact constraint clauses + index names
- [ ] Create `src-tauri/migrations/006_amounts_to_text.sql` with the filled-in SQL
- [ ] `ls src-tauri/migrations/` — confirm `006_amounts_to_text.sql` sorts after `0004…`
- [ ] `cargo build` — confirm sqlx discovers the migration (build embeds the migrations dir)

---

## Task 2: Rust DB Layer — Struct Fields

Add this helper at the top of `db/invoices.rs` (after the `use` block), and add `use std::str::FromStr;`:

```rust
fn parse_dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO).round_dp(2)
}
```
*(Bring `rust_decimal::Decimal` into module scope — it is currently imported locally inside `create()`. Promote `use rust_decimal::Decimal;` to the top of the file so the helper and struct logic share it.)*

### 2a. `Invoice` struct (`db/invoices.rs`) — fields read via `query_as` (FromRow)

Before:
```rust
    pub subtotal_amount: f64,
    pub vat_amount: f64,
    pub total_amount: f64,
```
After:
```rust
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
```
Leave `pub exchange_rate: Option<f64>` unchanged (FX rate, not money).

### 2b. `LineItem` struct (`db/invoices.rs`)

Before:
```rust
    pub quantity: f64,
    ...
    pub unit_price: f64,

    pub vat_rate: f64,
    ...
    pub subtotal_amount: f64,
    pub vat_amount: f64,
    pub total_amount: f64,
```
After:
```rust
    pub quantity: String,
    ...
    pub unit_price: String,

    pub vat_rate: String,
    ...
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
```
Because every read SELECT goes through `query_as::<_, LineItem>` with the `SELECT_LINE` const, changing the struct fields to `String` is sufficient for all reads — `FromRow` will pull TEXT. No per-call-site `try_get` edits needed in this file for reads.

### 2c. `create()` body (`db/invoices.rs`) — writes

The totals are already computed as `Decimal`. Stop down-casting to f64.

Before (line_rows builder, ~the `.map(|l| …)` closure):
```rust
    let line_rows: Vec<(String, f64, f64, f64)> = input
        .lines
        .iter()
        .map(|l| {
            ...
            (
                new_id(),
                ls.to_f64().unwrap_or(0.0),
                lv.to_f64().unwrap_or(0.0),
                lt.to_f64().unwrap_or(0.0),
            )
        })
        .collect();
    let subtotal  = subtotal_dec.to_f64().unwrap_or(0.0);
    let vat_total = vat_total_dec.to_f64().unwrap_or(0.0);
    let total     = (subtotal_dec + vat_total_dec).to_f64().unwrap_or(0.0);
```
After:
```rust
    let line_rows: Vec<(String, String, String, String)> = input
        .lines
        .iter()
        .map(|l| {
            ...
            (
                new_id(),
                ls.round_dp(2).to_string(),
                lv.round_dp(2).to_string(),
                lt.round_dp(2).to_string(),
            )
        })
        .collect();
    let subtotal  = subtotal_dec.round_dp(2).to_string();
    let vat_total = vat_total_dec.round_dp(2).to_string();
    let total     = (subtotal_dec + vat_total_dec).round_dp(2).to_string();
```
The `use rust_decimal::prelude::ToPrimitive;` import inside `create()` becomes unused — remove it (or `cargo build` will warn).

Invoice INSERT binds — `.bind(subtotal) .bind(vat_total) .bind(total)` now bind `String` (no code change to the `.bind(...)` lines themselves; they bind the renamed-type locals). Keep `.bind(input.exchange_rate)` as-is.

Line INSERT binds — currently:
```rust
        .bind(line.quantity)
        ...
        .bind(line.unit_price)
        .bind(line.vat_rate)
        ...
        .bind(line_subtotal)
        .bind(line_vat)
        .bind(line_total)
```
`line.quantity` / `line.unit_price` / `line.vat_rate` come from `CreateLineInput` which stays `f64` (wire format). Convert at bind time to normalized Decimal strings:
```rust
        .bind(Decimal::try_from(line.quantity).unwrap_or(Decimal::ZERO).round_dp(2).to_string())
        ...
        .bind(Decimal::try_from(line.unit_price).unwrap_or(Decimal::ZERO).round_dp(2).to_string())
        .bind(Decimal::try_from(line.vat_rate).unwrap_or(Decimal::ZERO).round_dp(2).to_string())
        ...
        .bind(line_subtotal)   // now String
        .bind(line_vat)        // now String
        .bind(line_total)      // now String
```

> **Decision — keep inputs as f64:** `CreateInvoiceInput`/`CreateLineInput` are deserialized from frontend JSON numbers. Keeping them `f64` avoids a frontend form-submission refactor; the f64→Decimal→String conversion happens once, immediately, at the DB boundary. (Alternative: change inputs to `String` and parse — more invasive on the form side. Default to keeping f64 unless the [VERIFY] of `src/lib/tauri.ts`/forms shows amounts already submitted as strings.)

### 2d. `ReceivedInvoice` struct + `create()` (`db/received.rs`)

Struct, before:
```rust
    pub total_amount: f64,
```
After:
```rust
    pub total_amount: String,
```

`CreateReceivedInput`, before:
```rust
    pub total_amount: f64,
```
After:
```rust
    pub total_amount: String,
```

`create()` bind, before:
```rust
    .bind(input.total_amount)
```
After (validate, then bind the string):
```rust
    .bind(&input.total_amount)
```
Add validation at the top of `create()` (mirrors `payments.rs`):
```rust
    use rust_decimal::Decimal;
    use std::str::FromStr;
    Decimal::from_str(input.total_amount.trim())
        .map_err(|_| AppError::Validation("Sumă invalidă — folosiți formatul 1234.56".into()))?;
```
**[VERIFY]** the callers of `received::create` (likely in `commands/integrations.rs` import or an ANAF download path) and update them to pass a `String` total (Decimal→string). Grep:
```bash
grep -rn "CreateReceivedInput\|total_amount" src-tauri/src/commands src-tauri/src/background
```

### 2e. `recurring.rs`
No struct change (no money columns; amounts live in `lines_json`). Skip.

Steps:
- [ ] Promote `use rust_decimal::Decimal;` + add `use std::str::FromStr;` to top of `db/invoices.rs`; add `parse_dec` helper
- [ ] `Invoice`: 3 money fields f64→String
- [ ] `LineItem`: 6 money fields f64→String
- [ ] `create()`: `line_rows` tuple type + Decimal→String; subtotal/vat_total/total →String; remove unused `ToPrimitive`; line binds convert qty/price/rate via Decimal::try_from→string
- [ ] `ReceivedInvoice.total_amount` f64→String; `CreateReceivedInput.total_amount` f64→String; add validation; bind `&input.total_amount`
- [ ] **[VERIFY]** + update all `received::create` callers to pass String
- [ ] `cargo build` (expect errors only in commands/ubl/background until later tasks)

---

## Task 3: Business Logic — Commands Layer

> Run each grep first; it reveals the exact lines. The DB layer above is verbatim; these files were not re-readable during authoring, so the worker confirms call sites with the grep, then applies the documented pattern.

### commands/invoices.rs
**[VERIFY]**
```bash
grep -nE "to_f64|as f64|try_get::<f64|subtotal_amount|vat_amount|total_amount|\.bind\(" src-tauri/src/commands/invoices.rs
```
Pattern:
- Any place that computes a Decimal and binds it: `.bind(amount.to_f64().unwrap_or(0.0))` → `.bind(amount.round_dp(2).to_string())`.
- Any place reading an amount column: `try_get::<f64, _>("total_amount")` → `try_get::<String, _>("total_amount")` then `Decimal::from_str(&s)` for math.
- Storno (credit note) creation: amounts are negated Decimals — bind `(-amount).round_dp(2).to_string()`.
- If create/update simply delegate to `db::invoices::create`, no amount handling here — confirm via grep.

Steps:
- [ ] Grep; for each amount bind, Decimal→`.round_dp(2).to_string()`
- [ ] For each amount read, `try_get::<String,_>` + `Decimal::from_str`
- [ ] Storno path binds negated Decimal strings

### commands/reports.rs
**[VERIFY]**
```bash
grep -nE "SUM\(|try_get::<f64|total_amount|vat_amount|subtotal_amount|query_scalar" src-tauri/src/commands/reports.rs
```
SQLite `SUM()` on TEXT yields 0/garbage — **do not** SUM in SQL anymore. Replace each aggregate with: select the raw amount column(s) as `String`, fold into `Decimal` in Rust (exactly like `payments::summary_for_invoice`):
```rust
use rust_decimal::Decimal;
use std::str::FromStr;
let rows: Vec<String> = sqlx::query_scalar(
    "SELECT total_amount FROM invoices WHERE company_id = ?1 AND issue_date BETWEEN ?2 AND ?3"
).bind(company_id).bind(from).bind(to).fetch_all(&state.db).await?;
let total = rows.iter()
    .map(|s| Decimal::from_str(s).unwrap_or(Decimal::ZERO))
    .fold(Decimal::ZERO, |a, b| a + b)
    .round_dp(2);
// return total.to_string() (or per the existing report return type)
```
If reports group by status/month, fetch `(group_key, amount_string)` rows via `query` + `try_get`, then aggregate into a `HashMap<String, Decimal>` in Rust. Report struct money fields f64→String accordingly.

Steps:
- [ ] Grep; replace every SQL `SUM()` of a money column with Rust Decimal fold
- [ ] Update report struct money fields f64→String; serialize as strings
- [ ] Confirm grouped/period totals aggregate in Rust, not SQL

### commands/saft.rs
**[VERIFY]**
```bash
grep -nE "try_get::<f64|as f64|total_amount|vat_amount|subtotal_amount|SUM\(" src-tauri/src/commands/saft.rs
```
SAF-T is the highest-stakes consumer (tax filing). Every amount read: `try_get::<f64,_>` → `try_get::<String,_>` then `Decimal::from_str`. Format into the SAF-T XML with explicit 2dp (`format!("{:.2}", d)` or `d.round_dp(2)`), never raw f64 `to_string()`. Replace any in-SQL `SUM` with Rust Decimal fold.

Steps:
- [ ] Grep; convert all amount reads to String→Decimal
- [ ] Ensure SAF-T XML numeric output uses Decimal 2dp formatting
- [ ] Replace any SQL SUM of money with Rust fold

### commands/integrations.rs
**[VERIFY]**
```bash
grep -nE "write_number|try_get::<f64|total_amount|vat_amount|as f64|xlsx" src-tauri/src/commands/integrations.rs
```
XLSX export (rust_xlsxwriter `write_number` requires f64). Read amount as `String`, convert to f64 **only** at the cell write:
```rust
let amt_s: String = row.try_get("total_amount")?;
let amt = Decimal::from_str(&amt_s).unwrap_or(Decimal::ZERO);
ws.write_number(r, c, amt.to_f64().unwrap_or(0.0))?;   // f64 ONLY for the cell value
```
(Keep `use rust_decimal::prelude::ToPrimitive;` local to this fn.) If the export also writes received-invoice or report totals, apply the same at each `write_number`.

Steps:
- [ ] Grep; for each money `write_number`, read String→Decimal→f64 at the call only
- [ ] Confirm no other f64 amount reads remain

### commands/import.rs
**[VERIFY]**
```bash
grep -nE "total_amount|parse::<f64|CreateReceivedInput|CreateInvoiceInput|Decimal|as f64" src-tauri/src/commands/import.rs
```
CSV/XML import parses external amount strings. Parse into `Decimal` (not f64), then pass `.round_dp(2).to_string()` into `CreateReceivedInput.total_amount` (now String) / invoice create path. If currently `parse::<f64>()`, switch to `Decimal::from_str` with a validation error on failure.

Steps:
- [ ] Grep; parse imported amounts as Decimal
- [ ] Pass Decimal strings into the (now String) input structs
- [ ] Validation error on unparseable amounts

---

## Task 4: UBL Layer

### ubl/generator.rs
**[VERIFY]**
```bash
grep -nE "fn fmt_amount|fmt_amount\(|: f64|total_amount|vat_amount|subtotal_amount|quantity|unit_price|vat_rate" src-tauri/src/ubl/generator.rs
```
Change the formatter signature:
Before:
```rust
fn fmt_amount(v: f64) -> String { format!("{:.2}", v) }
```
After:
```rust
fn fmt_amount(s: &str) -> String {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let d = Decimal::from_str(s.trim()).unwrap_or_default().round_dp(2);
    format!("{:.2}", d)
}
```
Update every caller from `fmt_amount(inv.total_amount)` (f64) to `fmt_amount(&inv.total_amount)` (String field, now `&str`). Same for line `quantity`/`unit_price`/`vat_rate`/`subtotal_amount`/`vat_amount`/`total_amount`. Quantities in UBL `InvoicedQuantity` may need different precision than 2dp — **[VERIFY]** and, if so, add a `fmt_qty(&str)` variant.

Steps:
- [ ] Change `fmt_amount` to take `&str`, parse via Decimal
- [ ] Update all callers to pass `&field`
- [ ] Confirm quantity precision; add `fmt_qty` if needed

### ubl/pdf.rs
**[VERIFY]**
```bash
grep -nE "as f64|: f64|total_amount|vat_amount|subtotal_amount|quantity|unit_price|format!\(\"\{:\." src-tauri/src/ubl/pdf.rs
```
Replace `inv.total_amount as f64` / numeric field reads with `Decimal::from_str(&inv.total_amount).unwrap_or_default()` then format for display. Reuse `parse_dec`/`fmt_amount` style.

Steps:
- [ ] Grep; convert all amount field reads to String→Decimal→formatted
- [ ] Verify PDF totals/line columns render correctly

### ubl/rocius_rules.rs
**[VERIFY]**
```bash
grep -nE "f64_to_dec|: f64|total_amount|vat_amount|subtotal_amount|Decimal::try_from" src-tauri/src/ubl/rocius_rules.rs
```
The `f64_to_dec` helper exists to recover Decimal from the old f64 fields. Now the fields are already strings — replace `f64_to_dec(inv.total_amount)` with `Decimal::from_str(&inv.total_amount).unwrap_or_default()`. If `f64_to_dec` becomes unused, delete it.

Steps:
- [ ] Grep; replace `f64_to_dec(field)` with `Decimal::from_str(&field)`
- [ ] Remove `f64_to_dec` if now unused

### ubl/validator.rs
**[VERIFY]**
```bash
grep -nE ": f64|as f64|total_amount|vat_amount|subtotal_amount|quantity|unit_price|abs\(\)" src-tauri/src/ubl/validator.rs
```
Validation arithmetic (e.g., line sums == header total) must run on `Decimal` parsed from the String fields, with exact `==`/tolerance on Decimal. Replace any f64 comparison with Decimal.

Steps:
- [ ] Grep; parse String→Decimal for all validation math
- [ ] Use Decimal equality/tolerance, not f64

---

## Task 5: Background Scheduler

### background/mod.rs
**[VERIFY]**
```bash
grep -nE "total_amount|vat_amount|subtotal_amount|as f64|to_f64|CreateInvoiceInput|db::invoices::create|\.bind\(" src-tauri/src/background/mod.rs
```
Recurring invoice generation: if it builds a `CreateInvoiceInput` and calls `db::invoices::create`, the amount handling is already covered by Task 2c — **no bind changes needed**, just confirm `lines_json` deserializes into `CreateLineInput` (f64 inputs) correctly. If it INSERTs into `invoices`/`invoice_line_items` directly, convert every amount bind f64→`Decimal…round_dp(2).to_string()` per Task 2c.

Steps:
- [ ] Grep; determine if it delegates to `db::invoices::create` (preferred) or inserts directly
- [ ] If direct insert, convert amount binds to Decimal strings
- [ ] Confirm `lines_json` → input deserialization still compiles

---

## Task 6: TypeScript Types + Frontend

### src/types/index.ts
**[VERIFY]**
```bash
grep -nE "subtotalAmount|vatAmount|totalAmount|unitPrice|vatRate|quantity" src/types/index.ts
```
Change to `string`:
- `Invoice.subtotalAmount`, `Invoice.vatAmount`, `Invoice.totalAmount`
- `InvoiceLine.quantity`, `InvoiceLine.unitPrice`, `InvoiceLine.vatRate`, `InvoiceLine.subtotalAmount`, `InvoiceLine.vatAmount`, `InvoiceLine.totalAmount`
- `ReceivedInvoice.totalAmount` (and `subtotalAmount`/`vatAmount` if present)
Keep `exchangeRate?: number` (FX rate). Payment `amount` is already `string`.

> **Input/form note:** Form components that build a `CreateInvoiceInput` likely keep numeric `<input type="number">` values. Since the Rust input structs stay `f64` (Task 2 decision), the create payload can still send numbers — **[VERIFY]** `src/lib/tauri.ts` create-invoice typing. Only the **read** types (`Invoice`, `InvoiceLine`, `ReceivedInvoice`) flip to `string`.

### src/lib/utils.ts
**[VERIFY]** file exists (`ls src/lib/`). Add:
```ts
export const parseDec = (s: string | number): number => parseFloat(String(s)) || 0;
```
If `utils.ts` doesn't exist, create it; ensure it doesn't collide with an existing helper export.

### src/lib/tauri.ts
**[VERIFY]**
```bash
grep -nE "totalAmount|vatAmount|subtotalAmount|unitPrice|vatRate|: number" src/lib/tauri.ts
```
Reconcile any inline typing/casting of returned amounts with the new `string` types. Update command return-type generics if they reference the changed interfaces.

### Frontend arithmetic — Dashboard / Reports / InvoiceDetail / Payments
For each page, **[VERIFY]** then wrap math:
```bash
grep -nE "totalAmount|vatAmount|subtotalAmount|unitPrice|vatRate|quantity|\.reduce\(|\+ *[a-zA-Z].*Amount|toFixed" \
  src/pages/Dashboard.tsx src/pages/Reports.tsx src/pages/InvoiceDetail.tsx src/pages/Payments.tsx
```
Pattern: `import { parseDec } from "@/lib/utils";` then
- `inv.totalAmount + x` → `parseDec(inv.totalAmount) + x`
- `arr.reduce((a, i) => a + i.totalAmount, 0)` → `arr.reduce((a, i) => a + parseDec(i.totalAmount), 0)`
- `line.quantity * line.unitPrice` → `parseDec(line.quantity) * parseDec(line.unitPrice)`
- `value.toFixed(2)` where `value` is now a string → `parseDec(value).toFixed(2)`
- Pure display (no math) of an already-2dp string can render directly; but prefer `parseDec(x).toFixed(2)` for consistent formatting.

Steps:
- [ ] `index.ts`: flip read-type money fields to `string`
- [ ] `utils.ts`: add `parseDec`
- [ ] `tauri.ts`: reconcile return typings
- [ ] Dashboard.tsx: wrap amount math with `parseDec`
- [ ] Reports.tsx: wrap amount math with `parseDec`
- [ ] InvoiceDetail.tsx: wrap line/total math with `parseDec`
- [ ] Payments.tsx: reconcile invoice-total math with `parseDec`

---

## Task 7: Build + Verification

- [ ] `cargo build 2>&1 | grep "^error"` — expect 0 errors
- [ ] `cd .. && npx tsc --noEmit` (from repo root) — expect 0 errors
- [ ] `cargo test 2>&1` — all tests pass (update any test fixtures asserting f64 amounts to expect strings)
- [ ] Manual: create invoice with one line qty 1 × 100.00 @ 19% → DB `total_amount` is exactly `"119.00"` (not `119.0`/`118.999…`). Verify: `sqlite3 <appdb> "SELECT total_amount FROM invoices ORDER BY created_at DESC LIMIT 1;"`
- [ ] Manual: pay 100.00 on a 200.00 invoice → `payment_status` = PARTIAL, paid `"100.00"`, remaining 100.00 (Payments page)
- [ ] Manual: generate SAF-T and UBL XML for the test invoice → amounts render `119.00`, line/header totals reconcile
- [ ] Manual: XLSX export opens with numeric amount cells equal to the invoice values
- [ ] Manual: existing pre-migration rows display correctly (backfill `printf('%.2f')` applied)
- [ ] Commit: `git add -A && git commit -m "feat: migrate money columns REAL→TEXT for Decimal precision"`

---

## Risk register

1. **Migration data loss / FK cascade (Task 1)** — highest risk. DROP TABLE on a parent with `ON DELETE CASCADE` children can wipe child rows if `foreign_keys` is ON. The `PRAGMA foreign_keys=OFF` bracket mitigates this; the rebuild order (parent then child, or child first) and exact FK clauses MUST come from the [VERIFY] grep. **Back up the SQLite file before first run.**
2. **Reports/SAF-T silent zeroing** — `SUM()` over TEXT returns 0 in SQLite without error. If any aggregate is missed, totals silently read 0. The Task 3 greps for `SUM(` are mandatory.
3. **Quantity precision** — `printf('%.2f')` truncates quantities to 2dp; if the domain needs more, use `CAST(... AS TEXT)` for the three quantity-ish columns.
4. **Frontend `+` string concatenation** — JS `"100.00" + "19.00"` = `"100.0019.00"`. Any missed `parseDec` becomes a visible wrong total, caught by `tsc` only if types are correct (they will be after Task 6) — the type flip to `string` is what makes `tsc` flag unwrapped arithmetic. Run `tsc` after every page edit.

---

## Gaps identificate la verificarea de fezabilitate (adăugate post-Opus)

### GAP 1 — saft.rs e parțial migrat deja
Header amounts (`vat_amount`, `total_amount` la liniile ~203-206) sunt DEJA citite ca `String`.
Line item amounts (`quantity`, `unit_price` etc. la liniile ~150-157) sunt încă `f64` via `try_get(...).unwrap_or(0.0)`.
Agentul trebuie să verifice ce e deja String și să schimbe NUMAI ce e încă f64.

### GAP 2 — background/mod.rs face INSERT direct, NU delegă la db::invoices::create
Are `let mut total_amount = 0.0f64;` (linia ~710) și `.bind(total_amount)` direct.
Schimbarea: `total_amount` din f64 → String, parse via Decimal, bind String.
Struct intern cu `subtotal: f64, vat_amount: f64, total_amount: f64` (~liniile 876-878) → String.

### GAP 3 — rocius_rules.rs: comparații directe pe câmpuri care devin String
```rust
.filter(|(_, l)| l.quantity == 0.0)      // linia ~343
.filter(|(_, l)| l.unit_price < 0.0)     // linia ~379
if !VALID_S.contains(&line.vat_rate)     // linia ~427 — Set<f64> vs String
```
Toate trebuie schimbate să parse din String via Decimal:
```rust
.filter(|(_, l)| parse_dec(&l.quantity) == Decimal::ZERO)
.filter(|(_, l)| parse_dec(&l.unit_price) < Decimal::ZERO)
// VALID_S devine Set<&str> sau comparăm Decimal
```

### GAP 4 — generator.rs: fixture-uri de test cu f64
La liniile ~628-661, struct-urile de test folosesc `subtotal_amount: 100.0` etc.
După migrare: `subtotal_amount: "100.00".to_string()`.

### GAP 5 — import.rs: 3 structuri locale + 2 parse paths
`pub total_amount: Option<f64>` struct local, `0.0f64` init, `text.parse().unwrap_or(0.0)` parse.
Struct CSV intern cu `subtotal: f64, vat_amount: f64, total_amount: f64`.
Toate trebuie schimbate la String + Decimal::from_str validation.
