-- Companies: own counters for quotes and orders (NEVER touch last_invoice_number)
ALTER TABLE companies ADD COLUMN last_quote_number INTEGER NOT NULL DEFAULT 0;
ALTER TABLE companies ADD COLUMN last_order_number INTEGER NOT NULL DEFAULT 0;

-- Quotes (oferte + devize)
CREATE TABLE IF NOT EXISTS quotes (
    id                   TEXT PRIMARY KEY NOT NULL,
    company_id           TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    contact_id           TEXT REFERENCES contacts(id),
    kind                 TEXT NOT NULL DEFAULT 'quote' CHECK(kind IN ('quote','deviz')),
    series               TEXT,
    number               INTEGER NOT NULL,
    full_number          TEXT,
    issue_date           TEXT NOT NULL,
    valid_until          TEXT,
    currency             TEXT NOT NULL DEFAULT 'RON',
    exchange_rate        TEXT,
    subtotal_amount      TEXT NOT NULL,
    vat_amount           TEXT NOT NULL,
    total_amount         TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','sent','accepted','invoiced','cancelled','expired')),
    notes                TEXT,
    accepted_at          INTEGER,
    converted_invoice_id TEXT REFERENCES invoices(id),
    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, series, number)
);

CREATE TABLE IF NOT EXISTS quote_lines (
    id               TEXT PRIMARY KEY NOT NULL,
    quote_id         TEXT NOT NULL REFERENCES quotes(id) ON DELETE CASCADE,
    position         INTEGER NOT NULL,
    name             TEXT NOT NULL,
    description      TEXT,
    quantity         TEXT NOT NULL,
    unit             TEXT,
    unit_price       TEXT NOT NULL,
    vat_rate         TEXT NOT NULL,
    vat_category     TEXT,
    subtotal_amount  TEXT NOT NULL,
    vat_amount       TEXT NOT NULL,
    total_amount     TEXT NOT NULL,
    revenue_kind     TEXT,
    cost_section     TEXT CHECK(cost_section IS NULL OR cost_section IN ('material','manopera','utilaj','transport'))
);

-- Orders (comenzi)
CREATE TABLE IF NOT EXISTS orders (
    id                   TEXT PRIMARY KEY NOT NULL,
    company_id           TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    contact_id           TEXT REFERENCES contacts(id),
    series               TEXT,
    number               INTEGER NOT NULL,
    full_number          TEXT,
    order_date           TEXT NOT NULL,
    expected_delivery    TEXT,
    currency             TEXT NOT NULL DEFAULT 'RON',
    exchange_rate        TEXT,
    subtotal_amount      TEXT NOT NULL,
    vat_amount           TEXT NOT NULL,
    total_amount         TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','sent','accepted','invoiced','cancelled')),
    notes                TEXT,
    converted_invoice_id TEXT REFERENCES invoices(id),
    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, series, number)
);

CREATE TABLE IF NOT EXISTS order_lines (
    id               TEXT PRIMARY KEY NOT NULL,
    order_id         TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    position         INTEGER NOT NULL,
    name             TEXT NOT NULL,
    description      TEXT,
    quantity         TEXT NOT NULL,
    unit             TEXT,
    unit_price       TEXT NOT NULL,
    vat_rate         TEXT NOT NULL,
    vat_category     TEXT,
    subtotal_amount  TEXT NOT NULL,
    vat_amount       TEXT NOT NULL,
    total_amount     TEXT NOT NULL,
    revenue_kind     TEXT,
    qty_reserved     TEXT NOT NULL DEFAULT '0'
);
