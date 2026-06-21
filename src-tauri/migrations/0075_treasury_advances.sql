-- P3 Wave D: Treasury advances (avansuri de trezorerie 542) + expense reports (deconturi)
-- with per-diem (diurnă) engine. CF art.76(2)(k)/(4)(h), HG 714/2018, OMFP 1802/2014.

CREATE TABLE IF NOT EXISTS treasury_advances (
    id             TEXT    NOT NULL PRIMARY KEY,
    company_id     TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    employee_id    TEXT,
    amount         TEXT    NOT NULL,           -- Decimal as text
    currency       TEXT    NOT NULL DEFAULT 'RON',
    granted_date   TEXT    NOT NULL,           -- ISO YYYY-MM-DD
    method         TEXT    NOT NULL DEFAULT 'cash' CHECK(method IN ('cash','bank')),
    status         TEXT    NOT NULL DEFAULT 'granted' CHECK(status IN ('granted','settled','returned')),
    notes          TEXT,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS expense_reports (
    id                    TEXT    NOT NULL PRIMARY KEY,
    company_id            TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    advance_id            TEXT    REFERENCES treasury_advances(id),
    employee_id           TEXT,
    delegation_from       TEXT,   -- ISO YYYY-MM-DD
    delegation_to         TEXT,   -- ISO YYYY-MM-DD
    destination           TEXT,
    days                  INTEGER,
    diurna_acordata       TEXT,   -- total diurnă acordată (Decimal text)
    diurna_neimpozabila   TEXT,   -- computed non-taxable portion
    diurna_impozabila     TEXT,   -- computed taxable excess (flagged, NOT posted to GL)
    salariu_baza          TEXT,   -- gross salary used for Limit B
    report_date           TEXT    NOT NULL,
    status                TEXT    NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','approved')),
    notes                 TEXT,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS expense_lines (
    id            TEXT    NOT NULL PRIMARY KEY,
    report_id     TEXT    NOT NULL REFERENCES expense_reports(id) ON DELETE CASCADE,
    category      TEXT    NOT NULL CHECK(category IN ('diurna','transport','cazare','combustibil','alte')),
    description   TEXT,
    amount        TEXT    NOT NULL,           -- net amount (Decimal text)
    vat_amount    TEXT,                       -- deductible VAT (Decimal text, nullable)
    account_code  TEXT    NOT NULL            -- 625/624/6022/628 + 4426 for VAT
);

-- Backfill missing accounts into existing companies' charts (pattern from 0034).
-- 542 Avansuri de trezorerie, 6022 Cheltuieli combustibili, 425 Avansuri personal.
-- (624, 625, 4426, 5311, 5121 already in standard_accounts().)
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '542'  AS code, 'Avansuri de trezorerie'                      AS name, 5 AS class
    UNION ALL SELECT '6022', 'Cheltuieli privind combustibilii',           6
    UNION ALL SELECT '425',  'Avansuri acordate personalului',             4
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
