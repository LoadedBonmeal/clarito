-- Migration 0080: Persist the multi-month diurnă split at create time (single source of truth).
--
-- P1 fix: the per-month breakdown is computed ONCE at create_report and stored here.
-- approve_report deserializes it — no recomputation, no drift.
--
-- diurna_interna: the configured internal daily rate (lei) used at create time for limit_A = 2.5×interna.
--   Stored so approve never falls back to a hardcoded "23.00".
--
-- diurna_breakdown_json: JSON array of [{period, nontax, excess}] per calendar month.
--   approve_report feeds one payroll_extra_income row per segment with excess > 0.
--   NULL for reports created before this migration (pre-0080 backward-compat path).
--
-- Σ-reconciliation invariant (enforced at create time, not by the DB):
--   Σ(nontax) + Σ(excess) == diurna_acordata EXACTLY (rounding remainder placed on last segment).
--   diurna_neimpozabila = Σ(nontax), diurna_impozabila = Σ(excess) (multimonth totals).

ALTER TABLE expense_reports ADD COLUMN diurna_interna         TEXT;   -- configured rate used at create (lei/zi)
ALTER TABLE expense_reports ADD COLUMN diurna_breakdown_json  TEXT;   -- JSON [{period,nontax,excess}]
