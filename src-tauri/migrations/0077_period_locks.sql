CREATE TABLE IF NOT EXISTS period_locks (
    id TEXT PRIMARY KEY NOT NULL,
    company_id TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    period TEXT NOT NULL,
    locked_at INTEGER NOT NULL,
    source TEXT NOT NULL,
    locked_by TEXT,
    note TEXT,
    UNIQUE(company_id, period)
);
CREATE INDEX IF NOT EXISTS idx_period_locks_company ON period_locks(company_id);
