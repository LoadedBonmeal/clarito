-- Wave C W1: staging tables for the multi-source data-migration importer.
-- These tables hold PARSED (not yet committed) rows from external sources.
-- All monetary/quantity columns are TEXT (Decimal-as-TEXT convention).

CREATE TABLE import_batch (
  id TEXT PRIMARY KEY,
  company_id TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
  source TEXT NOT NULL,            -- 'SMARTBILL_XML' | 'SMARTBILL_REST' | 'SAGA_XML' | 'SAGA_DBF' | 'WINMENTOR_TXT'
  source_label TEXT,
  column_map TEXT,                 -- JSON: user-confirmed header->field map (DEFENSIVE adapters)
  status TEXT NOT NULL DEFAULT 'PARSED', -- PARSED | PREVIEWED | COMMITTED | FAILED | CANCELLED
  counts_json TEXT,
  created_at INTEGER NOT NULL,
  committed_at INTEGER
);
CREATE INDEX idx_import_batch_company ON import_batch(company_id, created_at);

CREATE TABLE import_staging_contact (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES import_batch(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  source_code TEXT,
  contact_type TEXT,
  cui_raw TEXT,
  cui_canonical TEXT,
  legal_name TEXT,
  vat_payer INTEGER,
  is_individual INTEGER,
  address TEXT, city TEXT, county TEXT, country TEXT, email TEXT, phone TEXT,
  dedup_key TEXT,
  matched_id TEXT,
  resolution TEXT NOT NULL DEFAULT 'NEW', -- NEW | MATCH | DUP_IN_BATCH | REVIEW | ERROR
  error TEXT
);
CREATE INDEX idx_stg_contact_batch ON import_staging_contact(batch_id, resolution);

CREATE TABLE import_staging_product (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES import_batch(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  source_code TEXT,
  name TEXT,
  unit TEXT,
  unit_price TEXT,
  vat_rate TEXT,
  vat_category TEXT,
  code TEXT,
  barcode TEXT,
  stock_qty TEXT,
  is_service INTEGER,
  dedup_key TEXT,
  matched_id TEXT,
  resolution TEXT NOT NULL DEFAULT 'NEW',
  error TEXT
);
CREATE INDEX idx_stg_product_batch ON import_staging_product(batch_id, resolution);

CREATE TABLE import_staging_account (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES import_batch(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  account_code TEXT,
  synthetic_code TEXT,
  analytic_suffix TEXT,
  account_name TEXT,
  account_class INTEGER,
  dedup_key TEXT,
  matched_id TEXT,
  resolution TEXT NOT NULL DEFAULT 'NEW',
  error TEXT
);
CREATE INDEX idx_stg_account_batch ON import_staging_account(batch_id, resolution);

CREATE TABLE import_staging_invoice (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES import_batch(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  direction TEXT NOT NULL,         -- 'ISSUED' | 'RECEIVED'
  external_id TEXT,
  partner_cui_canonical TEXT,
  partner_name TEXT,
  partner_staging_id TEXT,
  partner_matched_id TEXT,
  series TEXT, number TEXT, full_number TEXT,
  issue_date TEXT, due_date TEXT,
  currency TEXT, exchange_rate REAL,
  reverse_charge INTEGER, cash_vat INTEGER,
  subtotal_amount TEXT, vat_amount TEXT, total_amount TEXT,
  dedup_key TEXT,
  matched_id TEXT,
  resolution TEXT NOT NULL DEFAULT 'NEW',
  error TEXT
);
CREATE INDEX idx_stg_invoice_batch ON import_staging_invoice(batch_id, direction, resolution);

CREATE TABLE import_staging_invoice_line (
  id TEXT PRIMARY KEY,
  invoice_staging_id TEXT NOT NULL REFERENCES import_staging_invoice(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  name TEXT, description TEXT,
  product_code TEXT,
  quantity TEXT, unit TEXT,
  unit_price TEXT, vat_rate TEXT, vat_category TEXT,
  subtotal_amount TEXT, vat_amount TEXT, total_amount TEXT,
  account_code TEXT, warehouse TEXT
);
CREATE INDEX idx_stg_invoice_line_parent ON import_staging_invoice_line(invoice_staging_id, position);
