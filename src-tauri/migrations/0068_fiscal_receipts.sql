-- Migration 0068: Bonuri fiscale / Raport Z (Casa de marcat)
-- Implementează înregistrarea vânzărilor zilnice prin casa de marcat cu
-- de-duplicarea față de facturile emise (metoda "Z-minus-facturat").

-- ── Tabel principal: capul raportului Z ─────────────────────────────────────
CREATE TABLE IF NOT EXISTS fiscal_receipts (
    id          TEXT    PRIMARY KEY,
    company_id  TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    serie_casa  TEXT    NOT NULL,
    nr_z        INTEGER NOT NULL,
    report_date TEXT    NOT NULL,      -- YYYY-MM-DD
    nr_bonuri   INTEGER NOT NULL DEFAULT 0,
    total       TEXT    NOT NULL DEFAULT '0.00',   -- total Z (numerar+card+tichete)
    numerar     TEXT    NOT NULL DEFAULT '0.00',
    card        TEXT    NOT NULL DEFAULT '0.00',
    tichete     TEXT    NOT NULL DEFAULT '0.00',
    status      TEXT    NOT NULL DEFAULT 'DRAFT',  -- DRAFT | POSTED | STORNAT
    retail_method INTEGER NOT NULL DEFAULT 0,      -- 0=analitic; 1=global (K amânat)
    notes       TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, serie_casa, report_date)
);

-- ── Linii TVA per cotă ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS fiscal_receipt_vat_lines (
    id           TEXT    PRIMARY KEY,
    receipt_id   TEXT    NOT NULL REFERENCES fiscal_receipts(id) ON DELETE CASCADE,
    vat_category TEXT    NOT NULL DEFAULT 'S',
    rate         TEXT    NOT NULL,          -- TEXT Decimal, ex. "21", "11", "0"
    baza         TEXT    NOT NULL DEFAULT '0.00',
    tva          TEXT    NOT NULL DEFAULT '0.00',
    UNIQUE(receipt_id, vat_category, rate)
);

-- ── Legătură bon–factură (de-dup: factura deja contabilizată → bon doar o încasează) ──
CREATE TABLE IF NOT EXISTS fiscal_receipt_invoice_links (
    id          TEXT    PRIMARY KEY,
    receipt_id  TEXT    NOT NULL REFERENCES fiscal_receipts(id) ON DELETE CASCADE,
    invoice_id  TEXT    NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    amount      TEXT    NOT NULL DEFAULT '0.00',  -- suma BRUTĂ a facturii încasate prin bon
    pay_means   TEXT    NOT NULL DEFAULT 'CASH',  -- CASH | CARD
    UNIQUE(receipt_id, invoice_id)
);

-- ── Indecși ──────────────────────────────────────────────────────────────────
CREATE INDEX IF NOT EXISTS idx_fiscal_receipts_company_date
    ON fiscal_receipts(company_id, report_date);

CREATE INDEX IF NOT EXISTS idx_fiscal_receipt_vat_lines_receipt
    ON fiscal_receipt_vat_lines(receipt_id);

CREATE INDEX IF NOT EXISTS idx_fiscal_receipt_links_receipt
    ON fiscal_receipt_invoice_links(receipt_id);

CREATE INDEX IF NOT EXISTS idx_fiscal_receipt_links_invoice
    ON fiscal_receipt_invoice_links(invoice_id);
