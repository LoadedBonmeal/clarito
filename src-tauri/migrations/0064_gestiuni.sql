-- 0064: gestiuni (warehouses) — PURELY ADDITIVE, no recompute
-- Step 1: gestiune table
CREATE TABLE IF NOT EXISTS gestiune (
    id                   TEXT    NOT NULL PRIMARY KEY,
    company_id           TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    cod                  TEXT    NOT NULL,
    denumire             TEXT    NOT NULL,
    tip                  TEXT    NOT NULL DEFAULT 'cantitativ_valorica',  -- cantitativ_valorica|global_valorica
    metoda_evaluare      TEXT    NOT NULL DEFAULT 'CMP',                  -- CMP|FIFO|LIFO
    cont_stoc            TEXT    NOT NULL DEFAULT '371',
    adresa               TEXT,
    dispersata_teritorial INTEGER NOT NULL DEFAULT 0,
    is_default           INTEGER NOT NULL DEFAULT 0,
    activ                INTEGER NOT NULL DEFAULT 1,
    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, cod)
);

-- Step 2: seed one default gestiune per existing company
INSERT OR IGNORE INTO gestiune (id, company_id, cod, denumire, tip, metoda_evaluare, cont_stoc, is_default, activ, created_at)
SELECT
    'gest-default-' || id,
    id,
    'PRINCIPALA',
    'Gestiune principală',
    'cantitativ_valorica',
    COALESCE(
        (SELECT valuation_method FROM products WHERE company_id = companies.id LIMIT 1),
        'CMP'
    ),
    '371',
    1,
    1,
    unixepoch()
FROM companies;

-- Step 3: add gestiune_id to stock_ledger (nullable, backfilled immediately)
ALTER TABLE stock_ledger ADD COLUMN gestiune_id TEXT REFERENCES gestiune(id);
UPDATE stock_ledger SET gestiune_id = 'gest-default-' || company_id WHERE gestiune_id IS NULL;

-- Step 4: add gestiune_id to stock_movement_lines (optional, for SAF-T tracing)
ALTER TABLE stock_movement_lines ADD COLUMN gestiune_id TEXT REFERENCES gestiune(id);
UPDATE stock_movement_lines
SET gestiune_id = (
    SELECT 'gest-default-' || sm.company_id
    FROM stock_movements sm
    WHERE sm.id = stock_movement_lines.movement_id
)
WHERE gestiune_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_stock_ledger_gestiune
    ON stock_ledger(company_id, product_id, gestiune_id, entry_date);
