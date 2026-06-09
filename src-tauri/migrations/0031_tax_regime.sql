-- Migration 0031: company tax regime (micro vs profit), for regime-aware behaviour + the
-- micro-ceiling warning.
--
-- 2026 (OUG 89/2025): micro-enterprise tax is a SINGLE 1% rate on revenue, with a turnover ceiling
-- of 100.000 EUR (tested at the year-end BNR rate); above it (or on losing the conditions) the
-- company owes PROFIT TAX (16%) from the quarter the ceiling was exceeded. We track the regime per
-- company so the app can monitor turnover vs the ceiling and advise the switch. Default 'micro'
-- (the common case for a small RO company); the user sets it in the company form.
ALTER TABLE companies ADD COLUMN tax_regime TEXT NOT NULL DEFAULT 'micro';
