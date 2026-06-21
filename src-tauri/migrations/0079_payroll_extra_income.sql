-- Wave E: Diurnă taxable-excess → payroll extra income feed.
--
-- Stores the per-employee per-month taxable diurnă surplus computed at decont settlement,
-- so the payroll run and D112 emitter can fold it into the CAS/CASS/impozit/CAM bases.
--
-- Idempotency key: (company_id, source_ref, employee_id, period) — re-settling the same
-- decont does NOT double-add. kind='venit_asimilat', source='diurna_decont'.
--
-- period_lock_status:
--   'open'              — payroll month is still open; the amount will be picked up by the
--                         next run_payroll call for that (company, period).
--   'needs_rectificativa' — the payroll month was CLOSED (period-locked) when the decont
--                           was approved; the excess is stored for reference but MUST be
--                           included via a D112 rectificativă (rectification filing).
CREATE TABLE IF NOT EXISTS payroll_extra_income (
    id                  TEXT    PRIMARY KEY NOT NULL,
    company_id          TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    employee_id         TEXT    NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    -- Period the excess belongs to (YYYY-MM) — the delegation calendar month.
    period              TEXT    NOT NULL,
    kind                TEXT    NOT NULL DEFAULT 'venit_asimilat',
    source              TEXT    NOT NULL DEFAULT 'diurna_decont',
    -- FK back to the approved expense_report.
    source_ref          TEXT    NOT NULL,
    -- Taxable excess amount (lei, Decimal text, ≥ 0).
    amount              TEXT    NOT NULL,
    -- Contribution flags — all TRUE for diurnă excess (CF art.76(2)(k)).
    flag_cas            INTEGER NOT NULL DEFAULT 1,  -- CAS 25% employee
    flag_cass           INTEGER NOT NULL DEFAULT 1,  -- CASS 10% employee
    flag_impozit        INTEGER NOT NULL DEFAULT 1,  -- impozit 10%
    flag_cam            INTEGER NOT NULL DEFAULT 1,  -- CAM 2.25% employer
    -- Period-lock status at the time of settlement.
    period_lock_status  TEXT    NOT NULL DEFAULT 'open',
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    -- Idempotency: one row per (company, source_ref, employee, period).
    UNIQUE(company_id, source_ref, employee_id, period)
);

CREATE INDEX IF NOT EXISTS idx_payroll_extra_income_company_period
    ON payroll_extra_income(company_id, period);

CREATE INDEX IF NOT EXISTS idx_payroll_extra_income_employee
    ON payroll_extra_income(employee_id);

-- Backfill 4282 (creanțe față de salariați — sumele reținute din diurna impozabilă
-- deja plătite cash vor fi recuperate de la angajat) into the standard chart of accounts.
-- This is a standard account that most companies already have, but we add it defensively.
INSERT OR IGNORE INTO chart_of_accounts (
    id, company_id, account_code, account_name, parent_code, created_at, updated_at
)
SELECT
    lower(hex(randomblob(16))),
    id,
    '4282',
    'Alte creanțe în legătură cu personalul',
    '428',
    unixepoch(),
    unixepoch()
FROM companies
WHERE id NOT IN (
    SELECT company_id FROM chart_of_accounts WHERE account_code = '4282'
);
