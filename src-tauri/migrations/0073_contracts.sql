-- P3 Wave B — Contracts
-- A contract is a commercial/legal driver record that groups recurring invoices.
-- NOT a document justificativ (OMFP 3512/2008) — no GL postings on any contract operation.

CREATE TABLE IF NOT EXISTS contracts (
    id                   TEXT    PRIMARY KEY NOT NULL,
    company_id           TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    contact_id           TEXT    REFERENCES contacts(id),
    number               TEXT,
    title                TEXT    NOT NULL,
    object               TEXT,
    value                TEXT,
    currency             TEXT    NOT NULL DEFAULT 'RON',
    start_date           TEXT    NOT NULL,
    end_date             TEXT,
    status               TEXT    NOT NULL DEFAULT 'active'
                                 CHECK(status IN ('draft','active','expired','terminated')),
    payment_terms_days   INTEGER,
    auto_renew           INTEGER NOT NULL DEFAULT 0,
    renewal_notice_days  INTEGER NOT NULL DEFAULT 30,
    notes                TEXT,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_contracts_company   ON contracts(company_id);
CREATE INDEX IF NOT EXISTS idx_contracts_status    ON contracts(status);
CREATE INDEX IF NOT EXISTS idx_contracts_end_date  ON contracts(end_date);
CREATE INDEX IF NOT EXISTS idx_contracts_contact   ON contracts(contact_id);

-- Link recurring invoices to a contract (nullable; existing rows stay valid).
ALTER TABLE recurring_invoices ADD COLUMN contract_id TEXT REFERENCES contracts(id);

CREATE INDEX IF NOT EXISTS idx_recurring_contract  ON recurring_invoices(contract_id);
