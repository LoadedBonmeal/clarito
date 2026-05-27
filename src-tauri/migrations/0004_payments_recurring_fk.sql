-- Migration 0004: Add FK constraints to payments.company_id and recurring_invoices.company_id
-- SQLite does not support ALTER TABLE ADD FOREIGN KEY, so we recreate both tables.
-- PRAGMA foreign_keys must be OFF during table recreation (SQLite default).

PRAGMA foreign_keys = OFF;

-- ── payments ─────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS payments_v2 (
    id          TEXT    PRIMARY KEY NOT NULL,
    invoice_id  TEXT    NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    company_id  TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    amount      TEXT    NOT NULL,
    currency    TEXT    NOT NULL DEFAULT 'RON',
    paid_at     TEXT    NOT NULL,
    method      TEXT    NOT NULL DEFAULT 'transfer',
    reference   TEXT,
    notes       TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO payments_v2 SELECT * FROM payments;
DROP TABLE payments;
ALTER TABLE payments_v2 RENAME TO payments;

CREATE INDEX IF NOT EXISTS idx_payments_invoice ON payments(invoice_id);
CREATE INDEX IF NOT EXISTS idx_payments_company ON payments(company_id);
CREATE INDEX IF NOT EXISTS idx_payments_paid_at ON payments(paid_at);

-- ── recurring_invoices ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS recurring_invoices_v2 (
    id               TEXT    PRIMARY KEY NOT NULL,
    company_id       TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    template_name    TEXT    NOT NULL,
    client_id        TEXT    NOT NULL REFERENCES contacts(id) ON DELETE RESTRICT,
    frequency        TEXT    NOT NULL,
    next_issue_date  TEXT    NOT NULL,
    day_of_month     INTEGER NOT NULL DEFAULT 1,
    auto_submit_anaf INTEGER NOT NULL DEFAULT 0,
    active           INTEGER NOT NULL DEFAULT 1,
    series           TEXT    NOT NULL,
    lines_json       TEXT    NOT NULL,
    notes            TEXT,
    created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO recurring_invoices_v2 SELECT * FROM recurring_invoices;
DROP TABLE recurring_invoices;
ALTER TABLE recurring_invoices_v2 RENAME TO recurring_invoices;

CREATE INDEX IF NOT EXISTS idx_recurring_company   ON recurring_invoices(company_id);
CREATE INDEX IF NOT EXISTS idx_recurring_next_date ON recurring_invoices(next_issue_date);
CREATE INDEX IF NOT EXISTS idx_recurring_active    ON recurring_invoices(active);

PRAGMA foreign_keys = ON;
