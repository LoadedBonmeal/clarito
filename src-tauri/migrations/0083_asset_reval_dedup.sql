-- Migration 0083: UNIQUE constraint on asset_revaluations (company_id, asset_id, revaluation_date).
--
-- Prevents duplicate revaluation events on the same date for the same asset (double-submit / retry).
-- The existing idx_asset_revaluations_company_asset index becomes a UNIQUE index; the old non-unique
-- index is dropped to avoid a redundant index on the same columns.
--
-- The INSERT in revalue_asset uses INSERT OR IGNORE + re-fetch to achieve idempotent retry behaviour.

DROP INDEX IF EXISTS idx_asset_revaluations_company_asset;

CREATE UNIQUE INDEX IF NOT EXISTS idx_asset_revaluations_unique_date
    ON asset_revaluations(company_id, asset_id, revaluation_date);
