-- Wave 3 — Provizioane (class 15x). OMFP 1802/2014 pct. 374(1): a provision is recognized only when
-- three cumulative conditions hold — (a) a present obligation (legal or constructive) from a past
-- event, (b) a probable outflow of resources, (c) a reliable estimate. Created D 6812 / C 15x;
-- reversed/used D 15x / C 7812. NB: per Cod fiscal art. 26 most provisions are NOT profit-tax
-- deductible (exceptions: warranty / good-execution, etc.) — tracked via the `deductible` flag.

CREATE TABLE IF NOT EXISTS provisions (
    id                   TEXT    PRIMARY KEY NOT NULL,
    company_id           TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    account_15x          TEXT    NOT NULL,             -- 1511..1518
    description          TEXT    NOT NULL,
    amount               TEXT    NOT NULL,             -- Decimal text, 2dp, > 0
    probability          TEXT,                          -- free text (e.g. 'probabil', 'posibil')
    expected_settlement  TEXT,                          -- 'YYYY-MM-DD' (optional)
    deductible           INTEGER NOT NULL DEFAULT 0,    -- profit-tax deductible (Cod fiscal art. 26)
    status               TEXT    NOT NULL DEFAULT 'active' CHECK(status IN ('active','reversed')),
    created_period       TEXT    NOT NULL,             -- 'YYYY-MM' constituire
    reversed_period      TEXT,                          -- 'YYYY-MM' reluare/utilizare
    notes                TEXT,
    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at           INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_provisions_company ON provisions(company_id);

-- Backfill the provision + expense/income accounts (not in the standard PCG seed).
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '1511' AS code, 'Provizioane pentru litigii' AS name, 1 AS class
    UNION ALL SELECT '1512', 'Provizioane pentru garanții acordate clienților', 1
    UNION ALL SELECT '1513', 'Provizioane pentru dezafectare imobilizări', 1
    UNION ALL SELECT '1514', 'Provizioane pentru restructurare', 1
    UNION ALL SELECT '1515', 'Provizioane pentru pensii și obligații similare', 1
    UNION ALL SELECT '1518', 'Alte provizioane', 1
    UNION ALL SELECT '6812', 'Cheltuieli de exploatare privind provizioanele', 6
    UNION ALL SELECT '7812', 'Venituri din provizioane', 7
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
