-- Dezmembrare stocuri — decomposing one product/asset into its constituent parts.
--
-- This is the inverse of a BOM (bill of materials) assembly: a single dismantled
-- item is taken OUT of stock, and multiple recovered components are put BACK IN at
-- their fair/market value.  The total fair value of all components equals the
-- carrying (book) cost of the dismantled item; any difference is posted to a
-- difference account (655 Cheltuieli din cedarea activelor / 758 Alte venituri din
-- exploatare).
--
-- Typical use-cases:
--   • Scrap / salvage of a damaged finished good
--   • Dis-assembling a fixed-asset bundle into individually-tracked spare parts
--   • Recovering raw materials from unsold finished products
--
-- Lifecycle:  DRAFT → POSTED
--   DRAFT  : valori estimate; no stock movement yet
--   POSTED : stock OUT for dismantled_product posted; stock IN for each line posted
--
-- Money columns are TEXT (Decimal strings) — same convention as the rest of the app.

-- ── dezmembrari ─────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS dezmembrari (
    id                      TEXT    PRIMARY KEY NOT NULL,
    company_id              TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- Gestiunea where the dismantling operation takes place
    gestiune_id             TEXT    NOT NULL REFERENCES gestiune(id),
    -- The item being dismantled
    dismantled_product_id   TEXT    NOT NULL REFERENCES products(id),
    -- Quantity being dismantled: 6dp Decimal stored as TEXT
    dismantled_qty          TEXT    NOT NULL,
    -- Total carrying cost (valoare contabilă) of the dismantled quantity: 2dp Decimal
    -- This is the credit side of 301/345/371 in the GL posting.
    dismantled_carrying_cost TEXT   NOT NULL,
    -- Date of the physical dismantling operation (YYYY-MM-DD)
    dezmembrare_date        TEXT    NOT NULL,
    -- Workflow status
    -- 'DRAFT'  – estimări, editabil
    -- 'POSTED' – mișcări de stoc postate în stock_ledger
    status                  TEXT    NOT NULL DEFAULT 'DRAFT',
    notes                   TEXT,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dezmembrari_company_id
    ON dezmembrari(company_id);

CREATE INDEX IF NOT EXISTS idx_dezmembrari_company_date
    ON dezmembrari(company_id, dezmembrare_date);

CREATE INDEX IF NOT EXISTS idx_dezmembrari_gestiune
    ON dezmembrari(gestiune_id);

CREATE INDEX IF NOT EXISTS idx_dezmembrari_product
    ON dezmembrari(dismantled_product_id);

-- ── dezmembrare_lines ────────────────────────────────────────────────────────────
-- Each row is a recovered component that enters stock.
-- The engine distributes dismantled_carrying_cost across lines proportionally to
-- their total_fair_value so that Σ(total_fair_value) ≈ dismantled_carrying_cost;
-- any rounding residual goes on the last line or is posted to a difference account.
CREATE TABLE IF NOT EXISTS dezmembrare_lines (
    id                  TEXT    PRIMARY KEY NOT NULL,
    dezmembrare_id      TEXT    NOT NULL REFERENCES dezmembrari(id) ON DELETE CASCADE,
    -- Display order on the printed process-verbal (1-based)
    position            INTEGER NOT NULL,
    -- The recovered component
    product_id          TEXT    NOT NULL REFERENCES products(id),
    -- Quantity recovered: 6dp Decimal stored as TEXT
    qty                 TEXT    NOT NULL,
    -- Fair / market value per unit of this component: 2dp Decimal stored as TEXT
    -- Used to determine the debit-side GL value when stock IN is posted.
    unit_fair_value     TEXT    NOT NULL,
    -- Total fair value = qty × unit_fair_value (pre-computed, stored for GL posting)
    -- 2dp Decimal stored as TEXT
    total_fair_value    TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dezmembrare_lines_parent
    ON dezmembrare_lines(dezmembrare_id);

CREATE INDEX IF NOT EXISTS idx_dezmembrare_lines_product
    ON dezmembrare_lines(product_id);
