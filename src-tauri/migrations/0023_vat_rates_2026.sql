-- Migration 0023: 2026 VAT-rate catalog — ordering + labels (Legea 141/2025).
--
-- In force from 1-Aug-2025, the 2026 regime is:
--   • standard rate         = 21%
--   • ONE general reduced   = 11%   (the former 5% and 9% general rates were repealed)
--   • 9% survives ONLY as a transitional rate for residential housing delivered
--     to individuals, until 31-Jul-2026.
--
-- This migration re-orders the catalog seeded by 0014 so the invoice rate picker
-- (ordered by `sort_order`, see db/vat_rates.rs::list) leads with the rates an
-- accountant actually uses in 2026 — 21% then 11% — and demotes the historical
-- 19%/5% rates to the end.
--
-- The historical rates are kept ACTIVE (not deleted): 19% is still needed to
-- correct / regularize invoices issued before 1-Aug-2025 (D300 old-rate
-- regularizări rows R16/R30), and removing a rate an accountant might need for a
-- correction would be unsafe. Labels are annotated to signal their status.
--
-- Idempotent: keyed on the fixed ids from 0014; re-runs are no-ops.

UPDATE vat_rates SET sort_order = 0, label = 'Standard 21%'                        WHERE id = 'vat-21';
UPDATE vat_rates SET sort_order = 1, label = 'Redus 11%'                           WHERE id = 'vat-11';
UPDATE vat_rates SET sort_order = 2, label = 'Redus 9% (locuințe, până la 31.07.2026)' WHERE id = 'vat-9';
UPDATE vat_rates SET sort_order = 3, label = 'Cotă zero 0%'                        WHERE id = 'vat-0';
UPDATE vat_rates SET sort_order = 8, label = 'Standard 19% (istoric)'             WHERE id = 'vat-19';
UPDATE vat_rates SET sort_order = 9, label = 'Redus 5% (istoric)'                 WHERE id = 'vat-5';
