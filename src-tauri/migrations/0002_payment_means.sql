-- Migration 0002: Add payment_means_code to invoices
-- UNCL4461 codes: 30=transfer bancar, 10=numerar, 48=card, 42=cont bancar, 58=SEPA
ALTER TABLE invoices ADD COLUMN payment_means_code TEXT NOT NULL DEFAULT '30';
