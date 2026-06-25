-- Wave 5 — Registrul bunurilor de capital + ajustarea TVA (Cod fiscal art. 305).
-- Capital goods = fixed assets / immovables whose deducted VAT is subject to a multi-year adjustment
-- period: 5 years for movables (and services on immovables), 20 years for the acquisition/construction
-- of immovables. If the deduction right changes during that period (use-change toward/away from taxable
-- operations — art. 305(4)), 1/5 (resp. 1/20) of the initially-deducted VAT is adjusted each affected
-- year, with the plus/minus sign reported on the D300 deductible-adjustment row. A *registru al
-- bunurilor de capital* is legally required (art. 305(6)).

CREATE TABLE IF NOT EXISTS capital_goods (
    id                    TEXT    PRIMARY KEY NOT NULL,
    company_id            TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    asset_id              TEXT,                          -- optional link to fixed_assets.id
    description           TEXT    NOT NULL,
    kind                  TEXT    NOT NULL CHECK(kind IN ('movable','immovable')),  -- 5yr / 20yr
    acquisition_date      TEXT    NOT NULL,              -- 'YYYY-MM-DD' (year 1 of the adjustment period)
    base_value            TEXT    NOT NULL,              -- cost base, Decimal text (informational)
    vat_deducted          TEXT    NOT NULL,              -- TVA dedusă inițial — the adjustment base
    adjustment_years      INTEGER NOT NULL CHECK(adjustment_years IN (5, 20)),
    initial_deduction_pct REAL    NOT NULL DEFAULT 100,  -- deduction right at acquisition (0..100)
    status                TEXT    NOT NULL DEFAULT 'active' CHECK(status IN ('active','disposed')),
    disposed_date         TEXT,                          -- 'YYYY-MM-DD' (early disposal: single one-off adj.)
    notes                 TEXT,
    created_at            INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at            INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_capital_goods_company ON capital_goods(company_id);

-- Per-year adjustment ledger. One row per (good, year) the deduction right differs from the initial one.
CREATE TABLE IF NOT EXISTS capital_good_adjustments (
    id                 TEXT    PRIMARY KEY NOT NULL,
    company_id         TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    capital_good_id    TEXT    NOT NULL REFERENCES capital_goods(id) ON DELETE CASCADE,
    year               INTEGER NOT NULL,                 -- adjustment year (1..adjustment_years)
    new_deduction_pct  REAL    NOT NULL,                 -- deduction right for this year (use-change)
    adjustment_amount  TEXT    NOT NULL,                 -- signed Decimal text: + extra deduction / - clawback
    period             TEXT    NOT NULL,                 -- 'YYYY-MM' the adjustment is recorded/declared
    posted             INTEGER NOT NULL DEFAULT 0,       -- 1 once a GL journal exists for it
    notes              TEXT,
    created_at         INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, capital_good_id, year)
);

CREATE INDEX IF NOT EXISTS idx_cg_adj_company ON capital_good_adjustments(company_id, capital_good_id);

-- Backfill the adjustment counterparties (idempotent; harmless if already seeded by the standard PCG).
-- 635 = clawback (deducted VAT becomes a cost); 758 = positive adjustment (additional deductible VAT
-- recognized as income). 4426 is the deductible-VAT account that is adjusted.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '635' AS code, 'Cheltuieli cu alte impozite, taxe și vărsăminte asimilate' AS name, 6 AS class
    UNION ALL SELECT '758', 'Alte venituri din exploatare', 7
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts a
    WHERE a.company_id = c.id AND a.account_code = v.code
);
