-- Aviz de însoțire a mărfii (formular OMFP 2634/2015 14-3-6A).
--
-- An aviz accompanies goods physically delivered to a customer WITHOUT an
-- immediate invoice — the invoice follows later ("pe baza avizului").  Common
-- use-cases: consignment deliveries, goods-in-transit, partial deliveries where
-- the full order isn't yet confirmed.
--
-- Lifecycle:  DRAFT → ISSUED → INVOICED
--   DRAFT    : ciornă, no stock movement yet
--   ISSUED   : aviz emis; stock OUT posted to stock_ledger (aviz_id tracks the link)
--   INVOICED : factura a fost emisă pe baza avizului; invoice_id populated
--
-- Money columns are stored as TEXT (Decimal strings) — same convention as invoices.

-- ── avize ───────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS avize (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    contact_id      TEXT    NOT NULL REFERENCES contacts(id),
    -- Document numbering
    series          TEXT    NOT NULL,
    number          INTEGER NOT NULL,
    -- Rendered series+number, e.g. "AV-00042"
    full_number     TEXT    NOT NULL,
    aviz_date       TEXT    NOT NULL,   -- YYYY-MM-DD
    -- Transport details (optional — filled when the company handles own transport)
    transport_means TEXT,               -- ex: "auto", "tren", "naval"
    driver_name     TEXT,
    vehicle_plate   TEXT,
    destination     TEXT,
    -- Workflow status
    -- 'DRAFT'    – ciornă, editabilă
    -- 'ISSUED'   – aviz emis, stoc OUT postat
    -- 'INVOICED' – factura emisă pe baza avizului
    status          TEXT    NOT NULL DEFAULT 'DRAFT',
    -- Populated once the covering invoice is created
    invoice_id      TEXT    REFERENCES invoices(id),
    -- Gestiunea from which the goods leave
    gestiune_id     TEXT    REFERENCES gestiune(id),
    -- Currency / FX (default RON; exchange_rate only relevant for non-RON avize)
    currency        TEXT    NOT NULL DEFAULT 'RON',
    exchange_rate   REAL,
    -- Totals (TEXT = Decimal, 2dp)
    subtotal_amount TEXT    NOT NULL DEFAULT '0.00',
    vat_amount      TEXT    NOT NULL DEFAULT '0.00',
    total_amount    TEXT    NOT NULL DEFAULT '0.00',
    notes           TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

-- One aviz number per series per company — same uniqueness rule as invoices.
CREATE UNIQUE INDEX IF NOT EXISTS avize_company_series_number
    ON avize(company_id, series, number);

CREATE INDEX IF NOT EXISTS idx_avize_company_id
    ON avize(company_id);

CREATE INDEX IF NOT EXISTS idx_avize_contact_id
    ON avize(contact_id);

CREATE INDEX IF NOT EXISTS idx_avize_status
    ON avize(company_id, status);

CREATE INDEX IF NOT EXISTS idx_avize_invoice_id
    ON avize(invoice_id);

-- ── aviz_lines ──────────────────────────────────────────────────────────────────
-- One row per goods line on the aviz.  Mirrors invoice_lines in structure so that
-- converting an aviz to an invoice is a straight copy.
CREATE TABLE IF NOT EXISTS aviz_lines (
    id              TEXT    PRIMARY KEY NOT NULL,
    aviz_id         TEXT    NOT NULL REFERENCES avize(id) ON DELETE CASCADE,
    -- Display order on the printed form (1-based)
    position        INTEGER NOT NULL,
    -- Optional link to the products catalogue
    product_id      TEXT    REFERENCES products(id),
    -- Free-text name (copied from product at creation time; may diverge later)
    name            TEXT    NOT NULL,
    description     TEXT,
    -- Quantity: 6dp Decimal stored as TEXT
    quantity        TEXT    NOT NULL,
    -- Unit of measure (buc, kg, m, l, …)
    unit            TEXT    NOT NULL DEFAULT 'buc',
    -- Unit price excl. VAT: 2dp Decimal stored as TEXT
    unit_price      TEXT    NOT NULL,
    -- VAT rate as a percentage Decimal string: "21", "9", "5", "0"
    vat_rate        TEXT    NOT NULL,
    -- VAT category per ANAF nomenclature: 'S' (standard) | 'R' (redusă) | 'Z' (zero) | 'E' (scutit)
    vat_category    TEXT    NOT NULL DEFAULT 'S',
    -- Line totals (TEXT = Decimal, 2dp)
    subtotal_amount TEXT    NOT NULL,
    vat_amount      TEXT    NOT NULL,
    total_amount    TEXT    NOT NULL,
    -- Revenue classification for GL posting: 'goods' | 'services'
    revenue_kind    TEXT    NOT NULL DEFAULT 'goods'
);

CREATE INDEX IF NOT EXISTS idx_aviz_lines_aviz_id
    ON aviz_lines(aviz_id);

-- ── stock_ledger backfill ────────────────────────────────────────────────────────
-- Track which aviz triggered a stock OUT movement.
-- Nullable — existing rows and non-aviz movements leave it NULL.
ALTER TABLE stock_ledger ADD COLUMN aviz_id TEXT REFERENCES avize(id);

CREATE INDEX IF NOT EXISTS idx_stock_ledger_aviz_id
    ON stock_ledger(aviz_id);
