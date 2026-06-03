-- Phase 6a: stock movements (MovementOfGoods SAF-T section)
-- Manual-entry capture path; UI planned for P7.

CREATE TABLE IF NOT EXISTS stock_movements (
    id              TEXT    NOT NULL PRIMARY KEY,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    movement_ref    TEXT    NOT NULL,              -- unique per company, e.g. "NIR-001"
    movement_date   TEXT    NOT NULL,              -- YYYY-MM-DD
    posting_date    TEXT    NOT NULL,              -- YYYY-MM-DD
    movement_type   TEXT    NOT NULL DEFAULT '10', -- DUK MovementType numeric code
    direction       TEXT    NOT NULL DEFAULT 'IN', -- IN | OUT
    document_type   TEXT,                          -- optional doc type (e.g. "NIR","BC","FF")
    document_number TEXT,                          -- optional doc number
    source_type     TEXT,                          -- optional: "invoice"/"received_invoice"/etc.
    source_id       TEXT,                          -- optional FK to source record
    notes           TEXT,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, movement_ref)
);

CREATE TABLE IF NOT EXISTS stock_movement_lines (
    id               TEXT    NOT NULL PRIMARY KEY,
    movement_id      TEXT    NOT NULL REFERENCES stock_movements(id) ON DELETE CASCADE,
    line_number      INTEGER NOT NULL DEFAULT 1,
    product_id       TEXT    REFERENCES products(id) ON DELETE SET NULL,
    product_code     TEXT    NOT NULL,             -- denormalized for SAF-T
    account_id       TEXT    NOT NULL DEFAULT '371', -- stock GL account
    customer_id      TEXT    NOT NULL DEFAULT '0',   -- canonical partner ("00"+CUI or "0")
    supplier_id      TEXT    NOT NULL DEFAULT '0',   -- canonical partner
    quantity         TEXT    NOT NULL DEFAULT '1',
    unit_of_measure  TEXT    NOT NULL DEFAULT 'H87', -- UN/ECE Rec-20 code
    uom_conv_factor  TEXT    NOT NULL DEFAULT '1',   -- UOMToUOMPhysicalStockConversionFactor
    book_value       TEXT    NOT NULL DEFAULT '0.00', -- SAFmonetaryType
    movement_subtype TEXT    NOT NULL DEFAULT '10',   -- DUK MovementSubType numeric code
    comments         TEXT
);

CREATE INDEX IF NOT EXISTS idx_stock_movements_company_date
    ON stock_movements(company_id, movement_date);

CREATE INDEX IF NOT EXISTS idx_stock_movement_lines_movement
    ON stock_movement_lines(movement_id);
