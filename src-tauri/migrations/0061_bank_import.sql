-- Wave 6: Bank statement import (jurnal de bancă)
--
-- bank_accounts  — company's own bank accounts (5121 RON / 5124 valută)
-- bank_statements — one imported statement file (MT940 / CAMT053 / CSV)
-- bank_transactions — individual transaction lines within a statement

CREATE TABLE IF NOT EXISTS bank_accounts (
    id          TEXT    PRIMARY KEY NOT NULL,
    company_id  TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    iban        TEXT    NOT NULL,
    bank_name   TEXT    NOT NULL DEFAULT '',
    currency    TEXT    NOT NULL DEFAULT 'RON',
    -- 5121 = conturi la bănci în lei / 5124 = conturi la bănci în valută
    gl_account  TEXT    NOT NULL DEFAULT '5121',
    created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS bank_statements (
    id               TEXT    PRIMARY KEY NOT NULL,
    company_id       TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    bank_account_id  TEXT    REFERENCES bank_accounts(id) ON DELETE SET NULL,
    source_format    TEXT    NOT NULL,   -- MT940 | CAMT053 | CSV
    statement_ref    TEXT    NOT NULL DEFAULT '',
    opening_balance  TEXT    NOT NULL DEFAULT '0',
    closing_balance  TEXT    NOT NULL DEFAULT '0',
    statement_date   TEXT    NOT NULL DEFAULT '',
    -- SHA-2 / hash of the raw file bytes — re-importing is idempotent
    content_hash     TEXT    NOT NULL,
    created_at       INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE(company_id, content_hash)
);

CREATE TABLE IF NOT EXISTS bank_transactions (
    id                  TEXT    PRIMARY KEY NOT NULL,
    statement_id        TEXT    NOT NULL REFERENCES bank_statements(id) ON DELETE CASCADE,
    company_id          TEXT    NOT NULL,
    booking_date        TEXT    NOT NULL,
    value_date          TEXT,
    -- signed: positive = credit (money in), negative = debit (money out)
    amount              TEXT    NOT NULL,
    currency            TEXT    NOT NULL DEFAULT 'RON',
    counterparty_name   TEXT,
    counterparty_iban   TEXT,
    counterparty_cui    TEXT,
    reference           TEXT,   -- :86: / RmtInf / CSV description
    txn_hash            TEXT    NOT NULL,
    status              TEXT    NOT NULL DEFAULT 'UNMATCHED', -- UNMATCHED | MATCHED | IGNORED
    matched_invoice_id  TEXT,
    matched_payment_id  TEXT,
    UNIQUE(statement_id, txn_hash)
);

CREATE INDEX IF NOT EXISTS idx_bank_txn_company  ON bank_transactions(company_id);
CREATE INDEX IF NOT EXISTS idx_bank_txn_status   ON bank_transactions(company_id, status);
CREATE INDEX IF NOT EXISTS idx_bank_stmt_company ON bank_statements(company_id);
