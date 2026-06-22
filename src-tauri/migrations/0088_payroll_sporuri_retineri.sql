-- Wave F: Sporuri (taxable salary additions) + Rețineri/Popriri (net deductions).
--
-- SPORURI (Codul muncii): spor vechime, spor de noapte, ore suplimentare, condiții
-- deosebite. All ENTER the CAS/CASS/impozit/CAM base — same as gross_salary.  Engine
-- adds per-employee sporuri to the gross BEFORE computing contributions so GL≡D112 holds
-- automatically (single rounding on combined base).
--
-- REȚINERI/POPRIRI (Codul muncii art.169 + Cod proc. civ.): deducted from the NET.
-- Withheld after CAS+CASS+impozit.  Legal caps: max 1/3 net per debt, Σ ≤ 1/2 net
-- (pensie alimentară has priority).  GL: D 421 = C 427/4282/462 + remaining D 421 = C 5311.

-- ── payroll_sporuri ─────────────────────────────────────────────────────────────
-- Per-employee per-month taxable spor amount.  Idempotency key: (company_id, employee_id, period).
-- `kind` is informational (display / filtering) — engine only uses `amount`.
CREATE TABLE IF NOT EXISTS payroll_sporuri (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id)  ON DELETE CASCADE,
    employee_id     TEXT    NOT NULL REFERENCES employees(id)  ON DELETE CASCADE,
    -- Period the spor belongs to (YYYY-MM).
    period          TEXT    NOT NULL,
    -- Amount (lei, Decimal text, ≥ 0).  Folded into gross for CAS/CASS/impozit/CAM.
    amount          TEXT    NOT NULL,
    -- Informational type: 'vechime' | 'noapte' | 'suplimentare' | 'conditii_deosebite' | 'alte'
    kind            TEXT    NOT NULL DEFAULT 'alte',
    -- Optional description / HR note.
    description     TEXT    NOT NULL DEFAULT '',
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    -- One row per (company, employee, period, kind) — upsert-safe.
    UNIQUE(company_id, employee_id, period, kind)
);

CREATE INDEX IF NOT EXISTS idx_payroll_sporuri_company_period
    ON payroll_sporuri(company_id, period);

CREATE INDEX IF NOT EXISTS idx_payroll_sporuri_employee
    ON payroll_sporuri(employee_id);

-- ── payroll_retineri ────────────────────────────────────────────────────────────
-- Per-employee per-month net deductions withheld and paid to a third party.
-- `account` must be one of: '427' (rețineri față de terți), '4282' (alte creanțe personal),
-- '462' (creditori diverși).  `priority` is used to apply pensie alimentară first when
-- multiple rețineri exist (lower number = higher priority, pensie alimentară = 1).
CREATE TABLE IF NOT EXISTS payroll_retineri (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id)  ON DELETE CASCADE,
    employee_id     TEXT    NOT NULL REFERENCES employees(id)  ON DELETE CASCADE,
    -- Period (YYYY-MM).
    period          TEXT    NOT NULL,
    -- Amount to retain from net (lei, Decimal text, > 0).
    amount          TEXT    NOT NULL,
    -- Type: 'poprire' | 'pensie_alimentara' | 'avans' | 'sindicat' | 'alte'
    kind            TEXT    NOT NULL DEFAULT 'alte',
    -- Creditor name / description.
    creditor        TEXT    NOT NULL DEFAULT '',
    -- GL credit account: '427' | '4282' | '462'
    account         TEXT    NOT NULL DEFAULT '427',
    -- Priority: 1 = pensie alimentară (highest), 2+ = others (lower).
    priority        INTEGER NOT NULL DEFAULT 2,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_payroll_retineri_company_period
    ON payroll_retineri(company_id, period);

CREATE INDEX IF NOT EXISTS idx_payroll_retineri_employee
    ON payroll_retineri(employee_id);

-- Backfill 427 and 462 into chart_of_accounts for all companies (defensive — many companies
-- already have them; INSERT OR IGNORE is a no-op in that case).
INSERT OR IGNORE INTO chart_of_accounts (
    id, company_id, account_code, account_name, parent_code, created_at, updated_at
)
SELECT
    lower(hex(randomblob(16))),
    id,
    '427',
    'Rețineri din remunerații datorate terților',
    '42',
    unixepoch(),
    unixepoch()
FROM companies
WHERE id NOT IN (
    SELECT company_id FROM chart_of_accounts WHERE account_code = '427'
);

INSERT OR IGNORE INTO chart_of_accounts (
    id, company_id, account_code, account_name, parent_code, created_at, updated_at
)
SELECT
    lower(hex(randomblob(16))),
    id,
    '462',
    'Creditori diverși',
    '46',
    unixepoch(),
    unixepoch()
FROM companies
WHERE id NOT IN (
    SELECT company_id FROM chart_of_accounts WHERE account_code = '462'
);
