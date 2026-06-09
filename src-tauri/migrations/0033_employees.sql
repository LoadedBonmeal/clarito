-- Migration 0033: payroll subsystem — employees (the per-employee data D112 + the monthly salary
-- states are built from). Salary stored as TEXT (Decimal string), matching the money convention.
CREATE TABLE IF NOT EXISTS employees (
    id                 TEXT    PRIMARY KEY NOT NULL,
    company_id         TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    cnp                TEXT    NOT NULL,
    full_name          TEXT    NOT NULL,
    -- Salariul brut lunar.
    gross_salary       TEXT    NOT NULL DEFAULT '0',
    -- Deducerea personală lunară (din tabelul ANAF).
    personal_deduction TEXT    NOT NULL DEFAULT '0',
    employment_date    TEXT,
    active             INTEGER NOT NULL DEFAULT 1,
    created_at         INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at         INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_employees_company ON employees(company_id);
