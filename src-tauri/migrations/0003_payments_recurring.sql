-- Migration 0003: Payment tracking + Recurring invoices

PRAGMA foreign_keys = ON;

-- ─── PAYMENTS ──────────────────────────────────────────────────────────────
-- Tracks money received against issued invoices.

CREATE TABLE IF NOT EXISTS payments (
    id          TEXT PRIMARY KEY NOT NULL,
    invoice_id  TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    company_id  TEXT NOT NULL,
    amount      TEXT NOT NULL,       -- Decimal stored as string (exact, never float)
    currency    TEXT NOT NULL DEFAULT 'RON',
    paid_at     TEXT NOT NULL,       -- ISO 8601 date YYYY-MM-DD
    method      TEXT NOT NULL DEFAULT 'transfer',  -- transfer|cash|card|other
    reference   TEXT,                -- bank reference / receipt number
    notes       TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_payments_invoice   ON payments(invoice_id);
CREATE INDEX IF NOT EXISTS idx_payments_company   ON payments(company_id);
CREATE INDEX IF NOT EXISTS idx_payments_paid_at   ON payments(paid_at);

-- ─── RECURRING INVOICES ────────────────────────────────────────────────────
-- Templates for auto-generating invoices on a schedule.

CREATE TABLE IF NOT EXISTS recurring_invoices (
    id               TEXT PRIMARY KEY NOT NULL,
    company_id       TEXT NOT NULL,
    template_name    TEXT NOT NULL,
    client_id        TEXT NOT NULL REFERENCES contacts(id),
    frequency        TEXT NOT NULL,           -- monthly|quarterly|annual
    next_issue_date  TEXT NOT NULL,           -- ISO 8601 date YYYY-MM-DD
    day_of_month     INTEGER NOT NULL DEFAULT 1,  -- 1-28
    auto_submit_anaf INTEGER NOT NULL DEFAULT 0,
    active           INTEGER NOT NULL DEFAULT 1,
    series           TEXT NOT NULL,
    lines_json       TEXT NOT NULL,           -- JSON array of invoice line items
    notes            TEXT,
    created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_recurring_company   ON recurring_invoices(company_id);
CREATE INDEX IF NOT EXISTS idx_recurring_next_date ON recurring_invoices(next_issue_date);
CREATE INDEX IF NOT EXISTS idx_recurring_active    ON recurring_invoices(active);
