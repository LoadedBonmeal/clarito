-- Migration 0014: Editable VAT-rate catalog (global table, not company-scoped).
--
-- Romanian VAT rates are defined by national legislation and apply uniformly
-- to all companies registered in Romania. This table is intentionally GLOBAL
-- (no company_id) — the same rates appear in every company's invoice editor.
-- This is the deliberate exception to the company-scoping rule used elsewhere.
--
-- The `rate` column is TEXT ("0", "5", "9", "11", "19", "21") to match the
-- Decimal-as-TEXT convention used throughout the application.

CREATE TABLE IF NOT EXISTS vat_rates (
    id         TEXT    PRIMARY KEY,
    rate       TEXT    NOT NULL,
    label      TEXT    NOT NULL,
    active     INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Seed the standard Romanian VAT rates idempotently.
-- Fixed ids (e.g. 'vat-19') guarantee re-runs are no-ops.
INSERT OR IGNORE INTO vat_rates (id, rate, label, active, sort_order) VALUES
    ('vat-19', '19', 'Standard 19%',  1, 0),
    ('vat-21', '21', 'Standard 21%',  1, 1),
    ('vat-9',  '9',  'Redus 9%',      1, 2),
    ('vat-11', '11', 'Redus 11%',     1, 3),
    ('vat-5',  '5',  'Redus 5%',      1, 4),
    ('vat-0',  '0',  'Cotă zero 0%',  1, 5);
