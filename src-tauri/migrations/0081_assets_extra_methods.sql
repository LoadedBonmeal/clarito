-- Migration 0081: extend fixed_assets for degressive / accelerated / super-accelerated depreciation.
--
-- New columns (nullable / with safe defaults so existing rows are unaffected):
--   fiscal_method  — optional fiscal amortization method (may differ from book method for D101).
--   is_new         — 1 if asset is new (vs second-hand); required for super-accelerată eligibility.
--   subgroup       — HG 2139/2004 catalog subgroup code (e.g. '2.1') for super-accelerată check.

ALTER TABLE fixed_assets ADD COLUMN fiscal_method TEXT;     -- NULL = same as depreciation_method
ALTER TABLE fixed_assets ADD COLUMN is_new        INTEGER NOT NULL DEFAULT 1;  -- 1 = new, 0 = used
ALTER TABLE fixed_assets ADD COLUMN subgroup      TEXT;     -- e.g. '2.1', NULL = not specified
