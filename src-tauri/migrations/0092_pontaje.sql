-- condică de prezență (CM art. 119) — worked days per employee per period.
-- UNIQUE(company_id, employee_id, period) — one record per employee/month.
CREATE TABLE IF NOT EXISTS pontaje (
    id           TEXT NOT NULL PRIMARY KEY,
    company_id   TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    employee_id  TEXT NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    period       TEXT NOT NULL,              -- YYYY-MM
    worked_days  INTEGER NOT NULL DEFAULT 0,
    overtime_hours TEXT NOT NULL DEFAULT '0',  -- Decimal stored as TEXT
    night_hours    TEXT NOT NULL DEFAULT '0',  -- Decimal stored as TEXT
    absence_days   INTEGER NOT NULL DEFAULT 0,
    leave_days     INTEGER NOT NULL DEFAULT 0,
    notes          TEXT NOT NULL DEFAULT '',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE(company_id, employee_id, period)
);
CREATE INDEX IF NOT EXISTS idx_pontaje_company_period ON pontaje(company_id, period);
