-- Wave 2 — Prepaid expenses / deferred income accruals (cheltuieli/venituri în avans).
-- OMFP 1802/2014 pct. 351: amounts paid/received in the current period that relate to future
-- periods are deferred to 471 (Cheltuieli înregistrate în avans) / 472 (Venituri înregistrate în
-- avans) and recognized to 6xx/7xx over the schedule. This table is the schedule register; the GL
-- postings (deferral on create + monthly recognition) live in db/accruals.rs → gl.rs::post_accruals.

CREATE TABLE IF NOT EXISTS accruals (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- 'prepaid'  → account 471, counter is a 6xx expense recognized over time
    -- 'deferred' → account 472, counter is a 7xx income recognized over time
    kind            TEXT    NOT NULL CHECK(kind IN ('prepaid','deferred')),
    description     TEXT    NOT NULL,
    counter_acct    TEXT    NOT NULL,                 -- the 6xx (prepaid) or 7xx (deferred) account
    total_amount    TEXT    NOT NULL,                 -- Decimal text, 2dp, > 0
    start_period    TEXT    NOT NULL,                 -- 'YYYY-MM' first recognition month
    months          INTEGER NOT NULL CHECK(months > 0),
    notes           TEXT,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_accruals_company ON accruals(company_id);

-- Backfill the accrual accounts (not in the standard PCG seed).
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '471' AS code, 'Cheltuieli înregistrate în avans' AS name, 4 AS class
    UNION ALL SELECT '472', 'Venituri înregistrate în avans', 4
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
