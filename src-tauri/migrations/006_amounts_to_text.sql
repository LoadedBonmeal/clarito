-- Migration 006: Convert monetary columns from REAL to TEXT (Decimal string storage).
-- SQLite does not support ALTER COLUMN TYPE; each table is rebuilt.
-- printf('%.2f', col) normalises existing float values to 2-decimal strings.

PRAGMA foreign_keys = OFF;

-- ── invoices ──────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS invoices_new (
    id                   TEXT PRIMARY KEY,
    company_id           TEXT NOT NULL REFERENCES companies(id),
    contact_id           TEXT NOT NULL REFERENCES contacts(id),

    series               TEXT NOT NULL,
    number               INTEGER NOT NULL,
    full_number          TEXT NOT NULL,

    issue_date           TEXT NOT NULL,
    due_date             TEXT NOT NULL,

    currency             TEXT NOT NULL DEFAULT 'RON',
    exchange_rate        REAL,

    subtotal_amount      TEXT NOT NULL,
    vat_amount           TEXT NOT NULL,
    total_amount         TEXT NOT NULL,

    status               TEXT NOT NULL DEFAULT 'DRAFT',

    anaf_upload_id       TEXT,
    anaf_index           TEXT,
    anaf_submitted_at    INTEGER,
    anaf_validated_at    INTEGER,
    anaf_rejected_at     INTEGER,

    xml_path             TEXT,
    pdf_path             TEXT,
    signature_xml_path   TEXT,

    rejection_reason     TEXT,
    rejection_code       TEXT,

    notes                TEXT,

    payment_means_code   TEXT NOT NULL DEFAULT '30',

    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at           INTEGER NOT NULL DEFAULT (unixepoch()),

    UNIQUE(company_id, series, number)
);

INSERT INTO invoices_new SELECT
    id,
    company_id,
    contact_id,
    series,
    number,
    full_number,
    issue_date,
    due_date,
    currency,
    exchange_rate,
    printf('%.2f', subtotal_amount) AS subtotal_amount,
    printf('%.2f', vat_amount)      AS vat_amount,
    printf('%.2f', total_amount)    AS total_amount,
    status,
    anaf_upload_id,
    anaf_index,
    anaf_submitted_at,
    anaf_validated_at,
    anaf_rejected_at,
    xml_path,
    pdf_path,
    signature_xml_path,
    rejection_reason,
    rejection_code,
    notes,
    payment_means_code,
    created_at,
    updated_at
FROM invoices;

DROP TABLE invoices;
ALTER TABLE invoices_new RENAME TO invoices;

CREATE INDEX IF NOT EXISTS idx_invoices_company_status ON invoices(company_id, status);
CREATE INDEX IF NOT EXISTS idx_invoices_anaf_upload    ON invoices(anaf_upload_id);
CREATE INDEX IF NOT EXISTS idx_invoices_issue_date     ON invoices(issue_date);

-- ── invoice_line_items ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS invoice_line_items_new (
    id              TEXT PRIMARY KEY,
    invoice_id      TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,

    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    quantity        TEXT NOT NULL,
    unit            TEXT NOT NULL,
    unit_price      TEXT NOT NULL,

    vat_rate        TEXT NOT NULL,
    vat_category    TEXT NOT NULL,

    subtotal_amount TEXT NOT NULL,
    vat_amount      TEXT NOT NULL,
    total_amount    TEXT NOT NULL,

    cpv_code        TEXT
);

INSERT INTO invoice_line_items_new SELECT
    id,
    invoice_id,
    position,
    name,
    description,
    printf('%.2f', quantity)        AS quantity,
    unit,
    printf('%.2f', unit_price)      AS unit_price,
    printf('%.2f', vat_rate)        AS vat_rate,
    vat_category,
    printf('%.2f', subtotal_amount) AS subtotal_amount,
    printf('%.2f', vat_amount)      AS vat_amount,
    printf('%.2f', total_amount)    AS total_amount,
    cpv_code
FROM invoice_line_items;

DROP TABLE invoice_line_items;
ALTER TABLE invoice_line_items_new RENAME TO invoice_line_items;

CREATE INDEX IF NOT EXISTS idx_lines_invoice ON invoice_line_items(invoice_id);

-- ── received_invoices ─────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS received_invoices_new (
    id                TEXT PRIMARY KEY,
    company_id        TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    anaf_download_id  TEXT NOT NULL UNIQUE,
    anaf_index        TEXT,

    issuer_cui        TEXT NOT NULL,
    issuer_name       TEXT NOT NULL,
    series            TEXT,
    number            TEXT,

    total_amount      TEXT NOT NULL,
    currency          TEXT NOT NULL,
    issue_date        TEXT NOT NULL,

    xml_path          TEXT NOT NULL,
    pdf_path          TEXT,

    status            TEXT NOT NULL DEFAULT 'NEW',

    downloaded_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    created_at        INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO received_invoices_new SELECT
    id,
    company_id,
    anaf_download_id,
    anaf_index,
    issuer_cui,
    issuer_name,
    series,
    number,
    printf('%.2f', total_amount) AS total_amount,
    currency,
    issue_date,
    xml_path,
    pdf_path,
    status,
    downloaded_at,
    created_at
FROM received_invoices;

DROP TABLE received_invoices;
ALTER TABLE received_invoices_new RENAME TO received_invoices;

CREATE INDEX IF NOT EXISTS idx_received_company_status ON received_invoices(company_id, status);

PRAGMA foreign_keys = ON;
