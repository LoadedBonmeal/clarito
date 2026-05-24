# RoFactura v2 — Plan de implementare module noi

**Data:** 2026-05-24  
**Scope:** 5 module noi + integrare completă în UI (ribbon, routes, pagini)

---

## Rezumat module noi

| Modul | DB | Backend (Rust) | Frontend (React) | Ribbon |
|---|---|---|---|---|
| Cote TVA | `vat_rates` | `commands/vat.rs` | `pages/VatRates.tsx` | Setări grup |
| Articole / Stocuri | `products` + `stock_movements` | `commands/products.rs` | `pages/Products.tsx` | Date grup |
| Plan de conturi | `chart_accounts` | `commands/accounts.rs` | `pages/Accounts.tsx` | Date grup |
| Chitanță | `receipts` | `commands/receipts.rs` | `pages/Receipts.tsx` | Operațiuni grup |
| Plăți | `payments` | `commands/payments.rs` | `pages/Payments.tsx` | Operațiuni grup |

---

## Arhitectură generală

**Principii de securitate obligatorii (NESCHIMBABILE):**
- `AppState` folosește `.db` — NU `.pool`
- Token-urile ANAF nu se loghează NICIODATĂ
- Matematică monetară EXCLUSIV cu `rust_decimal::Decimal` — NICIODATĂ `f64`
- XML-ul trebuie să aibă BOM UTF-8 — ANAF respinge fără el
- `uuid::Uuid::now_v7()` — NU `new_v4()`

---

## Task 1: Cote TVA

Cel mai simplu modul — fundație pentru celelalte. Gestionează cotele de TVA aplicabile în România.

### 1.1 Migrație DB

**Fișier:** `src-tauri/migrations/0002_vat_rates.sql`

```sql
CREATE TABLE IF NOT EXISTS vat_rates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,          -- "Standard 19%", "Redusă 9%", etc.
    rate        TEXT NOT NULL,          -- "0.19" — stocat ca TEXT pentru Decimal
    category    TEXT NOT NULL DEFAULT 'STANDARD', -- STANDARD | REDUCED | ZERO | EXEMPT
    is_active   INTEGER NOT NULL DEFAULT 1,
    is_default  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Seed cu cotele standard românești
INSERT OR IGNORE INTO vat_rates (id, name, rate, category, is_default) VALUES
    (lower(hex(randomblob(16))), 'Standard 19%',  '0.19', 'STANDARD', 1),
    (lower(hex(randomblob(16))), 'Redusă 9%',     '0.09', 'REDUCED',  0),
    (lower(hex(randomblob(16))), 'Redusă 5%',     '0.05', 'REDUCED',  0),
    (lower(hex(randomblob(16))), 'Scutit TVA',    '0.00', 'EXEMPT',   0);
```

### 1.2 Backend Rust

**Fișier nou:** `src-tauri/src/commands/vat.rs`

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use std::str::FromStr;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VatRate {
    pub id: String,
    pub name: String,
    pub rate: String,        // Decimal serializat ca String
    pub category: String,
    pub is_active: bool,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateVatRateInput {
    pub name: String,
    pub rate: String,        // "0.19"
    pub category: String,
    pub is_default: bool,
}

#[tauri::command]
pub async fn list_vat_rates(state: tauri::State<'_, AppState>) -> Result<Vec<VatRate>, String> {
    let rows = sqlx::query_as!(
        VatRate,
        r#"SELECT id, name, rate, category,
                  is_active as "is_active: bool",
                  is_default as "is_default: bool",
                  created_at, updated_at
           FROM vat_rates WHERE is_active = 1 ORDER BY rate DESC"#
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub async fn create_vat_rate(
    input: CreateVatRateInput,
    state: tauri::State<'_, AppState>,
) -> Result<VatRate, String> {
    // Validăm că rate-ul este un Decimal valid
    Decimal::from_str(&input.rate).map_err(|_| "Cotă TVA invalidă".to_string())?;
    
    let id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    
    if input.is_default {
        sqlx::query!("UPDATE vat_rates SET is_default = 0")
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    }
    
    sqlx::query!(
        "INSERT INTO vat_rates (id, name, rate, category, is_default, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        id, input.name, input.rate, input.category, input.is_default, now, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    list_vat_rates(state).await.map(|v| v.into_iter().find(|r| r.id == id).unwrap())
}

#[tauri::command]
pub async fn delete_vat_rate(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    sqlx::query!("UPDATE vat_rates SET is_active = 0 WHERE id = ?", id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
```

**Modificare:** `src-tauri/src/commands/mod.rs` — adaugă `pub mod vat;`

**Modificare:** `src-tauri/src/lib.rs` — înregistrează comenzile:
```rust
vat::list_vat_rates,
vat::create_vat_rate,
vat::delete_vat_rate,
```

### 1.3 Frontend

**Tip:** `src/types/index.ts`
```ts
export interface VatRate {
  id: string;
  name: string;
  rate: string;
  category: "STANDARD" | "REDUCED" | "ZERO" | "EXEMPT";
  isActive: boolean;
  isDefault: boolean;
  createdAt: number;
  updatedAt: number;
}
```

**API:** `src/lib/tauri.ts` — adaugă în `api`:
```ts
vat: {
  list: () => invoke<VatRate[]>("list_vat_rates"),
  create: (input: CreateVatRateInput) => invoke<VatRate>("create_vat_rate", { input }),
  delete: (id: string) => invoke<void>("delete_vat_rate", { id }),
},
```

**Pagină nouă:** `src/pages/VatRates.tsx` — tabel cu cotele active, formular inline pentru adăugare.

**Route:** adaugă în routerul TanStack `/vat-rates`

### 1.4 Integrare Ribbon

Grupul **Setări** din ribbon (sau sub Instrumente) — buton "Cote TVA" care navighează la `/vat-rates`.  
Alternativ: opțiune în meniul Settings existent.

---

## Task 2: Articole / Stocuri

Catalog de produse/servicii reutilizabil la crearea facturilor. Stocul e opțional (poate fi dezactivat per companie).

### 2.1 Migrație DB

**Fișier:** `src-tauri/migrations/0003_products.sql`

```sql
CREATE TABLE IF NOT EXISTS products (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    
    code            TEXT,                   -- cod intern / cod bare
    name            TEXT NOT NULL,
    description     TEXT,
    unit            TEXT NOT NULL DEFAULT 'buc',  -- buc, kg, m, ore, etc.
    
    price           TEXT NOT NULL,          -- Decimal ca TEXT
    vat_rate_id     TEXT REFERENCES vat_rates(id),
    
    track_stock     INTEGER NOT NULL DEFAULT 0,
    stock_quantity  TEXT NOT NULL DEFAULT '0',  -- Decimal ca TEXT
    stock_min       TEXT,                   -- alertă la minim
    
    is_active       INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_products_company ON products(company_id);
CREATE INDEX IF NOT EXISTS idx_products_code    ON products(company_id, code);

CREATE TABLE IF NOT EXISTS stock_movements (
    id          TEXT PRIMARY KEY,
    product_id  TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    type        TEXT NOT NULL,      -- IN | OUT | ADJUST
    quantity    TEXT NOT NULL,      -- Decimal ca TEXT
    reference   TEXT,               -- nr. factură sau document sursă
    note        TEXT,
    moved_at    INTEGER NOT NULL DEFAULT (unixepoch())
);
```

### 2.2 Backend Rust

**Fișier nou:** `src-tauri/src/commands/products.rs`

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use std::str::FromStr;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Product {
    pub id: String,
    pub company_id: String,
    pub code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub unit: String,
    pub price: String,              // Decimal ca String
    pub vat_rate_id: Option<String>,
    pub track_stock: bool,
    pub stock_quantity: String,     // Decimal ca String
    pub stock_min: Option<String>,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductInput {
    pub company_id: String,
    pub code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub unit: String,
    pub price: String,
    pub vat_rate_id: Option<String>,
    pub track_stock: bool,
    pub stock_quantity: Option<String>,
    pub stock_min: Option<String>,
}

#[tauri::command]
pub async fn list_products(
    company_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Product>, String> {
    sqlx::query_as!(
        Product,
        r#"SELECT id, company_id, code, name, description, unit, price,
                  vat_rate_id,
                  track_stock as "track_stock: bool",
                  stock_quantity,
                  stock_min,
                  is_active as "is_active: bool",
                  created_at, updated_at
           FROM products
           WHERE company_id = ? AND is_active = 1
           ORDER BY name ASC"#,
        company_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_product(
    input: CreateProductInput,
    state: tauri::State<'_, AppState>,
) -> Result<Product, String> {
    // Validare price
    Decimal::from_str(&input.price).map_err(|_| "Preț invalid".to_string())?;
    if let Some(ref qty) = input.stock_quantity {
        Decimal::from_str(qty).map_err(|_| "Cantitate stoc invalidă".to_string())?;
    }
    
    let id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    let qty = input.stock_quantity.unwrap_or_else(|| "0".to_string());
    
    sqlx::query!(
        "INSERT INTO products (id, company_id, code, name, description, unit, price,
                               vat_rate_id, track_stock, stock_quantity, stock_min, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        id, input.company_id, input.code, input.name, input.description,
        input.unit, input.price, input.vat_rate_id, input.track_stock,
        qty, input.stock_min, now, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    list_products(input.company_id, state).await
        .map(|v| v.into_iter().find(|p| p.id == id).unwrap())
}

#[tauri::command]
pub async fn update_product_stock(
    product_id: String,
    movement_type: String,   // "IN" | "OUT" | "ADJUST"
    quantity: String,
    reference: Option<String>,
    note: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let qty = Decimal::from_str(&quantity).map_err(|_| "Cantitate invalidă".to_string())?;
    
    let row = sqlx::query!("SELECT stock_quantity FROM products WHERE id = ?", product_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    
    let current = Decimal::from_str(&row.stock_quantity).unwrap_or(Decimal::ZERO);
    let new_qty = match movement_type.as_str() {
        "IN"     => current + qty,
        "OUT"    => current - qty,
        "ADJUST" => qty,
        _        => return Err("Tip mișcare invalid".to_string()),
    };
    
    let new_qty_str = new_qty.to_string();
    let mov_id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    
    sqlx::query!(
        "UPDATE products SET stock_quantity = ?, updated_at = ? WHERE id = ?",
        new_qty_str, now, product_id
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    sqlx::query!(
        "INSERT INTO stock_movements (id, product_id, type, quantity, reference, note, moved_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        mov_id, product_id, movement_type, quantity, reference, note, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    Ok(())
}
```

### 2.3 Frontend

**Pagini noi:**
- `src/pages/Products.tsx` — listă produse cu tabel dens (cod, denumire, unitate, preț, TVA, stoc)
- `src/pages/ProductNew.tsx` — formular creare produs
- `src/pages/ProductEdit.tsx` — editare produs

**Routes:** `/products`, `/products/new`, `/products/$id/edit`

**Integrare în `InvoiceNew.tsx`:** selector produs din catalog (autocomplete pe `name` / `code`) care pre-populează linia de factură cu preț + TVA.

### 2.4 Integrare Ribbon

Grupul **Date** — buton "Articole" (icon `package` sau `layers`) → `/products`

---

## Task 3: Plan de conturi

Planul de conturi românesc (PCR). Folosit pentru maparea contabilă a tranzacțiilor. La v2 poate fi read-only (seed cu PCR standard) cu posibilitate de adăugare conturi auxiliare.

### 3.1 Migrație DB

**Fișier:** `src-tauri/migrations/0004_chart_accounts.sql`

```sql
CREATE TABLE IF NOT EXISTS chart_accounts (
    id          TEXT PRIMARY KEY,
    company_id  TEXT REFERENCES companies(id) ON DELETE CASCADE,  -- NULL = global/standard
    
    symbol      TEXT NOT NULL,      -- "411", "4111", "701", etc.
    name        TEXT NOT NULL,      -- "Clienți", "Venituri din vânzări", etc.
    type        TEXT NOT NULL,      -- ASSET | LIABILITY | EQUITY | REVENUE | EXPENSE
    parent      TEXT,               -- simbol cont părinte
    is_detail   INTEGER NOT NULL DEFAULT 1,  -- 0 = sinteic, 1 = analitic
    is_active   INTEGER NOT NULL DEFAULT 1,
    
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_accounts_symbol ON chart_accounts(company_id, symbol);
CREATE INDEX        IF NOT EXISTS idx_accounts_parent ON chart_accounts(parent);
```

### 3.2 Backend Rust

**Fișier nou:** `src-tauri/src/commands/accounts.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChartAccount {
    pub id: String,
    pub company_id: Option<String>,
    pub symbol: String,
    pub name: String,
    pub account_type: String,   // "type" e cuvânt rezervat Rust
    pub parent: Option<String>,
    pub is_detail: bool,
    pub is_active: bool,
    pub created_at: i64,
}

#[tauri::command]
pub async fn list_accounts(
    company_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ChartAccount>, String> {
    sqlx::query_as!(
        ChartAccount,
        r#"SELECT id, company_id, symbol, name,
                  type as account_type,
                  parent,
                  is_detail as "is_detail: bool",
                  is_active as "is_active: bool",
                  created_at
           FROM chart_accounts
           WHERE is_active = 1
             AND (company_id IS NULL OR company_id = ?)
           ORDER BY symbol ASC"#,
        company_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_account(
    company_id: String,
    symbol: String,
    name: String,
    account_type: String,
    parent: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<ChartAccount, String> {
    let id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    
    sqlx::query!(
        "INSERT INTO chart_accounts (id, company_id, symbol, name, type, parent, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        id, company_id, symbol, name, account_type, parent, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    list_accounts(Some(company_id), state).await
        .map(|v| v.into_iter().find(|a| a.id == id).unwrap())
}
```

### 3.3 Seed PCR standard

**Fișier:** `src-tauri/src/commands/accounts_seed.rs` — funcție apelată la prima rulare care populează conturile standard (411, 4111, 701-706, 401, 531, 5121, etc.)

### 3.4 Frontend

**Pagină nouă:** `src/pages/Accounts.tsx` — arbore ierarhic al conturilor cu expandare pe simbol.

**Route:** `/accounts`

---

## Task 4: Chitanță

Chitanța este documentul de confirmare a primirii unei plăți în numerar sau card. Se emite opțional alăturat unei facturi.

### 4.1 Migrație DB

**Fișier:** `src-tauri/migrations/0005_receipts.sql`

```sql
CREATE TABLE IF NOT EXISTS receipts (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE RESTRICT,
    invoice_id      TEXT REFERENCES invoices(id) ON DELETE SET NULL,
    
    series          TEXT NOT NULL DEFAULT 'CH',
    number          INTEGER NOT NULL,
    issue_date      TEXT NOT NULL,   -- ISO 8601 "YYYY-MM-DD"
    
    payer_name      TEXT NOT NULL,
    payer_cui       TEXT,
    
    amount          TEXT NOT NULL,   -- Decimal ca TEXT
    currency        TEXT NOT NULL DEFAULT 'RON',
    payment_method  TEXT NOT NULL DEFAULT 'CASH',  -- CASH | CARD | TRANSFER
    
    notes           TEXT,
    
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_receipts_number ON receipts(company_id, series, number);
CREATE INDEX        IF NOT EXISTS idx_receipts_invoice ON receipts(invoice_id);
```

### 4.2 Backend Rust

**Fișier nou:** `src-tauri/src/commands/receipts.rs`

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use std::str::FromStr;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Receipt {
    pub id: String,
    pub company_id: String,
    pub invoice_id: Option<String>,
    pub series: String,
    pub number: i64,
    pub issue_date: String,
    pub payer_name: String,
    pub payer_cui: Option<String>,
    pub amount: String,
    pub currency: String,
    pub payment_method: String,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateReceiptInput {
    pub company_id: String,
    pub invoice_id: Option<String>,
    pub series: Option<String>,
    pub issue_date: String,
    pub payer_name: String,
    pub payer_cui: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub payment_method: String,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn list_receipts(
    company_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Receipt>, String> {
    sqlx::query_as!(
        Receipt,
        "SELECT * FROM receipts WHERE company_id = ? ORDER BY issue_date DESC, number DESC",
        company_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_receipt(
    input: CreateReceiptInput,
    state: tauri::State<'_, AppState>,
) -> Result<Receipt, String> {
    Decimal::from_str(&input.amount).map_err(|_| "Sumă invalidă".to_string())?;
    
    let id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    let series = input.series.unwrap_or_else(|| "CH".to_string());
    let currency = input.currency.unwrap_or_else(|| "RON".to_string());
    
    // Număr următor în serie
    let next_number = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM receipts WHERE company_id = ? AND series = ?",
        input.company_id, series
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?
    .unwrap_or(1);
    
    sqlx::query!(
        "INSERT INTO receipts (id, company_id, invoice_id, series, number, issue_date,
                               payer_name, payer_cui, amount, currency, payment_method, notes,
                               created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        id, input.company_id, input.invoice_id, series, next_number,
        input.issue_date, input.payer_name, input.payer_cui, input.amount,
        currency, input.payment_method, input.notes, now, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    list_receipts(input.company_id, state).await
        .map(|v| v.into_iter().find(|r| r.id == id).unwrap())
}
```

### 4.3 Frontend

**Pagini noi:**
- `src/pages/Receipts.tsx` — listă chitanțe (tabel: serie-nr, dată, plătitor, sumă, metodă)
- `src/pages/ReceiptNew.tsx` — formular chitanță, cu selector factură opțional
- `src/pages/ReceiptDetail.tsx` — detaliu + buton printare

**Route:** `/receipts`, `/receipts/new`, `/receipts/$id`

**Integrare în `InvoiceDetail.tsx`:** buton "Emite chitanță" care pre-populează `ReceiptNew` cu datele facturii.

### 4.4 Integrare Ribbon

Grupul **Operațiuni** — buton "Chitanță" (icon `receipt`) → `/receipts/new`

---

## Task 5: Plăți

Modulul de plăți urmărește plățile primite sau efectuate față de facturi emise/primite.

### 5.1 Migrație DB

**Fișier:** `src-tauri/migrations/0006_payments.sql`

```sql
CREATE TABLE IF NOT EXISTS payments (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE RESTRICT,
    
    direction       TEXT NOT NULL,   -- INCOMING | OUTGOING
    invoice_id      TEXT REFERENCES invoices(id)  ON DELETE SET NULL,
    received_id     TEXT REFERENCES received_invoices(id) ON DELETE SET NULL,
    
    amount          TEXT NOT NULL,   -- Decimal ca TEXT
    currency        TEXT NOT NULL DEFAULT 'RON',
    payment_date    TEXT NOT NULL,   -- "YYYY-MM-DD"
    payment_method  TEXT NOT NULL,   -- CASH | CARD | BANK_TRANSFER | CEC | OP
    
    reference       TEXT,            -- nr. OP, referință bancară
    notes           TEXT,
    
    receipt_id      TEXT REFERENCES receipts(id) ON DELETE SET NULL,
    
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_payments_company  ON payments(company_id);
CREATE INDEX IF NOT EXISTS idx_payments_invoice  ON payments(invoice_id);
CREATE INDEX IF NOT EXISTS idx_payments_received ON payments(received_id);
CREATE INDEX IF NOT EXISTS idx_payments_date     ON payments(payment_date DESC);
```

### 5.2 Backend Rust

**Fișier nou:** `src-tauri/src/commands/payments.rs`

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use std::str::FromStr;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Payment {
    pub id: String,
    pub company_id: String,
    pub direction: String,
    pub invoice_id: Option<String>,
    pub received_id: Option<String>,
    pub amount: String,
    pub currency: String,
    pub payment_date: String,
    pub payment_method: String,
    pub reference: Option<String>,
    pub notes: Option<String>,
    pub receipt_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct InvoicePaymentStatus {
    pub invoice_id: String,
    pub total_amount: String,
    pub paid_amount: String,
    pub remaining: String,
    pub is_fully_paid: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreatePaymentInput {
    pub company_id: String,
    pub direction: String,
    pub invoice_id: Option<String>,
    pub received_id: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub payment_date: String,
    pub payment_method: String,
    pub reference: Option<String>,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn list_payments(
    company_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Payment>, String> {
    sqlx::query_as!(
        Payment,
        "SELECT * FROM payments WHERE company_id = ? ORDER BY payment_date DESC",
        company_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_payment(
    input: CreatePaymentInput,
    state: tauri::State<'_, AppState>,
) -> Result<Payment, String> {
    Decimal::from_str(&input.amount).map_err(|_| "Sumă invalidă".to_string())?;
    
    if input.invoice_id.is_none() && input.received_id.is_none() {
        return Err("O plată trebuie asociată unei facturi emise sau primite.".to_string());
    }
    
    let id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp();
    let currency = input.currency.unwrap_or_else(|| "RON".to_string());
    
    sqlx::query!(
        "INSERT INTO payments (id, company_id, direction, invoice_id, received_id, amount,
                               currency, payment_date, payment_method, reference, notes, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        id, input.company_id, input.direction, input.invoice_id, input.received_id,
        input.amount, currency, input.payment_date, input.payment_method,
        input.reference, input.notes, now, now
    )
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    list_payments(input.company_id, state).await
        .map(|v| v.into_iter().find(|p| p.id == id).unwrap())
}

#[tauri::command]
pub async fn get_invoice_payment_status(
    invoice_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<InvoicePaymentStatus, String> {
    let total_row = sqlx::query!(
        "SELECT total_amount FROM invoices WHERE id = ?",
        invoice_id
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Factura nu există".to_string())?;
    
    let paid_rows = sqlx::query!(
        "SELECT amount FROM payments WHERE invoice_id = ?",
        invoice_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    
    let total = Decimal::from_str(&total_row.total_amount).unwrap_or(Decimal::ZERO);
    let paid: Decimal = paid_rows
        .iter()
        .filter_map(|r| Decimal::from_str(&r.amount).ok())
        .sum();
    let remaining = (total - paid).max(Decimal::ZERO);
    
    Ok(InvoicePaymentStatus {
        invoice_id,
        total_amount: total.to_string(),
        paid_amount: paid.to_string(),
        remaining: remaining.to_string(),
        is_fully_paid: remaining == Decimal::ZERO,
    })
}
```

### 5.3 Frontend

**Pagini noi:**
- `src/pages/Payments.tsx` — listă plăți cu filtre (direcție, metodă, perioadă)
- `src/pages/PaymentNew.tsx` — formular plată, cu selector factură + sumă pre-populată

**Route:** `/payments`, `/payments/new`

**Integrare în `InvoiceDetail.tsx`:** badge status plată ("Plătit / Parțial / Neîncasat") + buton "Înregistrează plată".

**Integrare în `ReceivedDetail.tsx`:** la fel — badge + buton "Marchează plătit".

### 5.4 Integrare Ribbon

Grupul **Operațiuni** — buton "Plată" (icon `creditCard` sau `banknote`) → `/payments/new`

---

## Task 6: Integrare finală Ribbon v2

Ribbon-ul final va arăta astfel (5 grupuri):

```
[ Operațiuni          ] [ Sincronizare ANAF    ] [ Date              ] [ Documente       ] [ Instrumente ]
  Factură nouă (Ctrl+N)  Trimite ANAF (F9)        Companii             Chitanță
  Primită nouă           Descarcă SPV (Ctrl+D)    Contacte             Plată
  Storno                 Verifică status (F10)    Articole
  Contact nou            Mesaje SPV               Conturi                              ←auto→  Comenzi (Ctrl+K)
                                                  Cote TVA                                     Setări
```

**Modificare:** `src/components/layout/Ribbon.tsx` — adaugă grupul DOCUMENTE cu Chitanță + Plată și extinde DATE cu Articole + Conturi + Cote TVA.

---

## Task 7: Icons noi necesare

Verifică în `src/components/shared/Icon.tsx` și adaugă ce lipsește:

| Modul | Icon sugerat | SVG |
|---|---|---|
| Articole | `package` | cutie cu linii |
| Chitanță | `receipt` | document cu linii orizontale |
| Plată | `banknote` | bancnotă |
| Plan conturi | `book` | carte |
| Cote TVA | `percent` | simbol % |

---

## Task 8: Teste de integrare

Pentru fiecare modul nou, scrie cel puțin un test Rust de integrare în `src-tauri/tests/`:

```rust
// tests/vat_rates_test.rs
#[tokio::test]
async fn test_create_and_list_vat_rate() {
    let db = setup_test_db().await;
    // insert rate
    // assert list returns it
    // assert rate is valid Decimal
}
```

---

## Estimare efort

| Task | Complexitate | Estimat |
|---|---|---|
| Cote TVA | Simplă | 0.5 zile |
| Articole/Stocuri | Medie | 1.5 zile |
| Plan de conturi | Medie | 1 zi |
| Chitanță | Medie | 1 zi |
| Plăți | Complexă | 2 zile |
| Ribbon + routes + icons | Simplă | 0.5 zile |
| Teste integrare | Medie | 1 zi |
| **TOTAL** | | **~7.5 zile** |

---

## Ordine de implementare recomandată

1. **Cote TVA** — fundație pentru Articole (FK `vat_rate_id`)
2. **Articole/Stocuri** — necesar pentru pre-popularea liniilor de factură
3. **Plan de conturi** — independent, poate merge în paralel cu 1-2
4. **Chitanță** — după ce facturile sunt stabile
5. **Plăți** — ultimul, depinde de chitanțe + facturi emise + primite
