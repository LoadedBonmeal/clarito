-- RoFactura — schemă inițială
-- Toate tabelele sunt create în prima migrație; modificări ulterioare prin migrații noi.

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

-- ─── SETTINGS ──────────────────────────────────────────────────────────────
-- KV store pentru setări runtime (env ANAF, polling, etc.)

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- ─── LICENSE ───────────────────────────────────────────────────────────────
-- O singură linie (CHECK id = 1). Stocează tier-ul + machine fingerprint.

CREATE TABLE IF NOT EXISTS license (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    license_key        TEXT,
    tier               TEXT NOT NULL DEFAULT 'TRIAL',
    activated_at       INTEGER,
    expires_at         INTEGER NOT NULL,
    machine_id         TEXT NOT NULL,
    email              TEXT,
    last_validated_at  INTEGER
);

-- ─── COMPANIES ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS companies (
    id                  TEXT PRIMARY KEY,
    cui                 TEXT NOT NULL UNIQUE,
    legal_name          TEXT NOT NULL,
    trade_name          TEXT,
    registry_number     TEXT,
    vat_payer           INTEGER NOT NULL DEFAULT 1,

    address             TEXT NOT NULL,
    city                TEXT NOT NULL,
    county              TEXT NOT NULL,
    postal_code         TEXT,
    country             TEXT NOT NULL DEFAULT 'RO',

    email               TEXT,
    phone               TEXT,
    iban                TEXT,
    bank_name           TEXT,

    is_active           INTEGER NOT NULL DEFAULT 1,
    spv_enabled         INTEGER NOT NULL DEFAULT 0,

    invoice_series      TEXT NOT NULL DEFAULT 'FACT',
    last_invoice_number INTEGER NOT NULL DEFAULT 0,

    logo_path           TEXT,

    created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_companies_active ON companies(is_active);
CREATE INDEX IF NOT EXISTS idx_companies_cui    ON companies(cui);

-- ─── CERTIFICATES ──────────────────────────────────────────────────────────
-- Doar metadata; token-urile efective sunt în OS Keychain.

CREATE TABLE IF NOT EXISTS certificates (
    id                  TEXT PRIMARY KEY,
    company_id          TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    keychain_ref        TEXT NOT NULL UNIQUE,

    issued_at           INTEGER NOT NULL,
    expires_at          INTEGER NOT NULL,
    refreshable_until   INTEGER NOT NULL,

    is_active           INTEGER NOT NULL DEFAULT 1,
    last_refreshed_at   INTEGER,
    last_used_at        INTEGER,

    created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at          INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_certs_company  ON certificates(company_id);
CREATE INDEX IF NOT EXISTS idx_certs_expires  ON certificates(expires_at);

-- ─── CONTACTS ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS contacts (
    id            TEXT PRIMARY KEY,
    company_id    TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    contact_type  TEXT NOT NULL,     -- CUSTOMER | SUPPLIER | BOTH
    cui           TEXT,
    legal_name    TEXT NOT NULL,
    vat_payer     INTEGER NOT NULL DEFAULT 0,

    address       TEXT,
    city          TEXT,
    county        TEXT,
    country       TEXT NOT NULL DEFAULT 'RO',

    email         TEXT,
    phone         TEXT,

    created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_contacts_company ON contacts(company_id);
CREATE INDEX IF NOT EXISTS idx_contacts_cui     ON contacts(cui);

-- ─── INVOICES ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS invoices (
    id                   TEXT PRIMARY KEY,
    company_id           TEXT NOT NULL REFERENCES companies(id),
    contact_id           TEXT NOT NULL REFERENCES contacts(id),

    series               TEXT NOT NULL,
    number               INTEGER NOT NULL,
    full_number          TEXT NOT NULL,

    issue_date           TEXT NOT NULL,   -- ISO "2026-05-18"
    due_date             TEXT NOT NULL,

    currency             TEXT NOT NULL DEFAULT 'RON',
    exchange_rate        REAL,

    subtotal_amount      REAL NOT NULL,
    vat_amount           REAL NOT NULL,
    total_amount         REAL NOT NULL,

    status               TEXT NOT NULL DEFAULT 'DRAFT',
        -- DRAFT | QUEUED | SUBMITTED | VALIDATED | REJECTED | STORNED

    anaf_upload_id       TEXT,
    anaf_index           TEXT,
    anaf_submitted_at    INTEGER,
    anaf_validated_at    INTEGER,
    anaf_rejected_at     INTEGER,

    xml_path             TEXT,
    pdf_path             TEXT,
    signature_xml_path   TEXT,

    rejection_reason     TEXT,
    rejection_code       TEXT,

    notes                TEXT,

    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at           INTEGER NOT NULL DEFAULT (unixepoch()),

    UNIQUE(company_id, series, number)
);

CREATE INDEX IF NOT EXISTS idx_invoices_company_status ON invoices(company_id, status);
CREATE INDEX IF NOT EXISTS idx_invoices_anaf_upload    ON invoices(anaf_upload_id);
CREATE INDEX IF NOT EXISTS idx_invoices_issue_date     ON invoices(issue_date);

-- ─── INVOICE LINE ITEMS ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS invoice_line_items (
    id              TEXT PRIMARY KEY,
    invoice_id      TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,

    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    quantity        REAL NOT NULL,
    unit            TEXT NOT NULL,
    unit_price      REAL NOT NULL,

    vat_rate        REAL NOT NULL,
    vat_category    TEXT NOT NULL,        -- S | Z | E | AE | K | G | O

    subtotal_amount REAL NOT NULL,
    vat_amount      REAL NOT NULL,
    total_amount    REAL NOT NULL,

    cpv_code        TEXT
);

CREATE INDEX IF NOT EXISTS idx_lines_invoice ON invoice_line_items(invoice_id);

-- ─── INVOICE EVENTS ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS invoice_events (
    id          TEXT PRIMARY KEY,
    invoice_id  TEXT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,

    event_type  TEXT NOT NULL,
    message     TEXT NOT NULL,
    metadata    TEXT,

    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_events_invoice ON invoice_events(invoice_id, created_at);

-- ─── RECEIVED INVOICES ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS received_invoices (
    id                TEXT PRIMARY KEY,
    company_id        TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,

    anaf_download_id  TEXT NOT NULL UNIQUE,
    anaf_index        TEXT,

    issuer_cui        TEXT NOT NULL,
    issuer_name       TEXT NOT NULL,
    series            TEXT,
    number            TEXT,

    total_amount      REAL NOT NULL,
    currency          TEXT NOT NULL,
    issue_date        TEXT NOT NULL,

    xml_path          TEXT NOT NULL,
    pdf_path          TEXT,

    status            TEXT NOT NULL DEFAULT 'NEW',
        -- NEW | REVIEWED | APPROVED | REJECTED | ARCHIVED

    downloaded_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    created_at        INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_received_company_status ON received_invoices(company_id, status);

-- ─── NOTIFICATIONS ─────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS notifications (
    id                     TEXT PRIMARY KEY,

    notification_type      TEXT NOT NULL,
    title                  TEXT NOT NULL,
    body                   TEXT NOT NULL,
    data                   TEXT,

    is_read                INTEGER NOT NULL DEFAULT 0,
    read_at                INTEGER,
    os_notification_shown  INTEGER NOT NULL DEFAULT 0,

    created_at             INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_notif_unread ON notifications(is_read, created_at);

-- ─── AUDIT LOG ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS audit_log (
    id           TEXT PRIMARY KEY,

    action       TEXT NOT NULL,
    entity_type  TEXT NOT NULL,
    entity_id    TEXT NOT NULL,
    metadata     TEXT,

    created_at   INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_entity  ON audit_log(entity_type, entity_id);
