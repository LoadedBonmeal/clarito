-- Plan de conturi (chart of accounts) — company-scoped catalog.
-- Account codes are unique per company (unique index on company_id + account_code).

CREATE TABLE IF NOT EXISTS chart_of_accounts (
    id           TEXT    PRIMARY KEY NOT NULL,
    company_id   TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    account_code TEXT    NOT NULL,
    account_name TEXT    NOT NULL,
    account_class INTEGER,
    parent_code  TEXT,
    active       INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_chart_of_accounts_company_id
    ON chart_of_accounts (company_id);

-- Each company has at most one account with a given code.
CREATE UNIQUE INDEX IF NOT EXISTS idx_chart_of_accounts_company_code
    ON chart_of_accounts (company_id, account_code);
