-- Migration 0022: intra_eu_kind — split intra-EU acquisitions (K) into goods vs services.
--
-- D300 goods acquisitions → R5/R18; services acquisitions → R7/R20.
-- Only the D300 row mapping reads this column; the GL engine (gl.rs),
-- reconcile core (declarations.rs d300_vat_totals), D394, and SAF-T all
-- continue to treat category "K" as reverse charge (unchanged).
--
-- TEXT NOT NULL DEFAULT 'goods': existing rows default to goods (backward-compatible).

ALTER TABLE received_invoices ADD COLUMN intra_eu_kind TEXT NOT NULL DEFAULT 'goods';
