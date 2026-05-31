-- Migration 0012: Add net/VAT breakdown to received_invoices.
-- Adds nullable net_amount / vat_amount to the existing table (no rebuild needed)
-- and a new per-rate breakdown table for Wave-B aggregation.

ALTER TABLE received_invoices ADD COLUMN net_amount TEXT;
ALTER TABLE received_invoices ADD COLUMN vat_amount TEXT;

CREATE TABLE IF NOT EXISTS received_invoice_vat_lines (
    id                   TEXT PRIMARY KEY,
    received_invoice_id  TEXT NOT NULL REFERENCES received_invoices(id) ON DELETE CASCADE,
    vat_rate             TEXT NOT NULL,
    vat_category         TEXT NOT NULL,
    base_amount          TEXT NOT NULL,
    vat_amount           TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_received_vat_lines_invoice
    ON received_invoice_vat_lines(received_invoice_id);
