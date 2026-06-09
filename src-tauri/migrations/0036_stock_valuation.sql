-- Migration 0036: stock valuation (FIFO/CMP). Per-product valuation policy + running cache on
-- products, and the valued stock ledger (fișa de magazie) = one row per IN/OUT event.
ALTER TABLE products ADD COLUMN valuation_method TEXT NOT NULL DEFAULT 'CMP';  -- 'FIFO' | 'CMP'
ALTER TABLE products ADD COLUMN stock_account    TEXT NOT NULL DEFAULT '371';  -- 371/301/302x/345
ALTER TABLE products ADD COLUMN avg_cost         TEXT NOT NULL DEFAULT '0.00'; -- CMP running unit cost
ALTER TABLE products ADD COLUMN stock_value      TEXT NOT NULL DEFAULT '0.00'; -- running total value

CREATE TABLE IF NOT EXISTS stock_ledger (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    product_id      TEXT    NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    entry_date      TEXT    NOT NULL,                      -- YYYY-MM-DD
    seq             INTEGER NOT NULL DEFAULT 0,            -- intra-day tie-break
    direction       TEXT    NOT NULL,                      -- 'IN' | 'OUT'
    qty             TEXT    NOT NULL DEFAULT '0.000000',
    unit_cost       TEXT    NOT NULL DEFAULT '0.00',       -- IN: purchase cost; OUT: valued COGS unit
    value           TEXT    NOT NULL DEFAULT '0.00',
    run_qty         TEXT    NOT NULL DEFAULT '0.000000',   -- running on-hand AFTER this row
    run_value       TEXT    NOT NULL DEFAULT '0.00',
    fifo_remaining  TEXT    NOT NULL DEFAULT '0.000000',   -- remaining qty of this IN layer (FIFO)
    doc_type        TEXT,
    doc_ref         TEXT,
    source_type     TEXT,
    source_id       TEXT,
    notes           TEXT,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_stock_ledger_product ON stock_ledger(company_id, product_id, entry_date);
