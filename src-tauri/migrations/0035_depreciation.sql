-- Migration 0035: depreciation register (one row per asset per posted month) + backfill the
-- depreciation accounts into existing companies' charts (mirror 0034). The monthly run UPSERTs the
-- register and posts D 6811 / C 281x to the GL.
CREATE TABLE IF NOT EXISTS asset_depreciation (
    id            TEXT    PRIMARY KEY NOT NULL,
    company_id    TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    asset_id      TEXT    NOT NULL REFERENCES fixed_assets(id) ON DELETE CASCADE,
    period        TEXT    NOT NULL,                 -- 'YYYY-MM' (depreciated month)
    amount        TEXT    NOT NULL DEFAULT '0.00',  -- monthly charge (Decimal string)
    accumulated   TEXT    NOT NULL DEFAULT '0.00',  -- accumulated AFTER this month (capped at cost)
    book_value    TEXT    NOT NULL DEFAULT '0.00',  -- cost − accumulated
    expense_acct  TEXT    NOT NULL DEFAULT '6811',
    amort_acct    TEXT    NOT NULL DEFAULT '2813',
    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, asset_id, period)
);
CREATE INDEX IF NOT EXISTS idx_asset_deprec_company_period ON asset_depreciation(company_id, period);

INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '6811' AS code, 'Cheltuieli de exploatare privind amortizarea imobilizărilor' AS name, 6 AS class
    UNION ALL SELECT '2812', 'Amortizarea construcțiilor', 2
    UNION ALL SELECT '2814', 'Amortizarea altor imobilizări corporale', 2
    UNION ALL SELECT '6583', 'Cheltuieli privind activele cedate și alte operații de capital', 6
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
