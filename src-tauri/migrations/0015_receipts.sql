-- Migration 0015: Chitanțe (cash receipts) — company-scoped.
--
-- Each receipt belongs to exactly one company. Cross-company access is
-- prevented at the command layer (company_id param + verify-after-fetch).
-- Amount stored as TEXT (Decimal-as-TEXT convention).

ALTER TABLE companies ADD COLUMN last_receipt_number INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS receipts (
    id          TEXT    PRIMARY KEY,
    company_id  TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    series      TEXT    NOT NULL DEFAULT 'CH',
    number      INTEGER NOT NULL,
    contact_id  TEXT    REFERENCES contacts(id),
    invoice_id  TEXT    REFERENCES invoices(id),
    amount      TEXT    NOT NULL,
    currency    TEXT    NOT NULL DEFAULT 'RON',
    issue_date  TEXT    NOT NULL,
    payer_name  TEXT,
    notes       TEXT,
    pdf_path    TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_receipts_company ON receipts(company_id);
