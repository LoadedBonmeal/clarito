-- Migration 0017: store exchange rate on received invoices.
-- Nullable REAL, consistent with invoices.exchange_rate REAL.
-- sqlx runs each migration exactly once (tracked in _sqlx_migrations),
-- so ALTER TABLE without IF NOT EXISTS is safe here.

ALTER TABLE received_invoices ADD COLUMN exchange_rate REAL;
