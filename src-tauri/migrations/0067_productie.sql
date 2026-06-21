-- Producție / BOM (P2 Wave 5, OMFP 1802/2014 pct. 8)
--
-- Monografie producție (materiale):
--   Consum materii prime:   D 601 = C 301  (materie_prima OUT)
--   Consum mat. consumabile: D 602 = C 302  (material_consumabil OUT)
--   Obținere produse finite: D 345 = C 711  (produs_finit IN)
--
-- Costul 345 capitalizat = DOAR costul materialelor (MVP). Manoperă directă
-- (641/421) și regie (681, cheltuieli fixe/variabile) rămân cheltuieli ale
-- perioadei și nu sunt adăugate la costul unitar în această versiune.
-- Per OMFP 1802/2014 pct. 8 costul complet ar include manoperă + regie alocată
-- pe capacitatea normală, cu regie fixă neabsorbită → cheltuiala perioadei.
-- Alocarea regiei este un follow-up planificat.

-- Rețeta (capul de BOM): un produs finit + cantitatea produsă per rulare standard.
CREATE TABLE IF NOT EXISTS bom (
    id          TEXT NOT NULL PRIMARY KEY,
    company_id  TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    product_id  TEXT NOT NULL REFERENCES products(id),  -- produsul finit obținut
    name        TEXT NOT NULL,
    output_qty  TEXT NOT NULL DEFAULT '1',              -- câte unități produce o rulare standard
    active      INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bom_company ON bom(company_id);

-- Liniile BOM: componentele consumate per output_qty unități produse.
CREATE TABLE IF NOT EXISTS bom_lines (
    id                   TEXT NOT NULL PRIMARY KEY,
    bom_id               TEXT NOT NULL REFERENCES bom(id) ON DELETE CASCADE,
    component_product_id TEXT NOT NULL REFERENCES products(id),
    qty                  TEXT NOT NULL,   -- cantitate consumată per output_qty unități
    um                   TEXT,
    line_no              INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bom_lines_bom ON bom_lines(bom_id);

-- Ordinul de producție (bon de lucru / lansare producție).
CREATE TABLE IF NOT EXISTS productie_orders (
    id                  TEXT NOT NULL PRIMARY KEY,
    company_id          TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    bom_id              TEXT NOT NULL REFERENCES bom(id),
    product_id          TEXT NOT NULL,               -- produs finit (denormalizat pt. queries)
    gestiune_id         TEXT NOT NULL REFERENCES gestiune(id),
    qty_produced        TEXT NOT NULL,               -- cantitatea efectiv produsă
    production_date     TEXT NOT NULL,               -- YYYY-MM-DD
    total_material_cost TEXT NOT NULL,               -- Σ(qty_comp × unit_cost_FIFO/CMP/LIFO)
    unit_cost           TEXT NOT NULL,               -- total_material_cost / qty_produced
    status              TEXT NOT NULL DEFAULT 'finalized',
    notes               TEXT,
    created_at          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_productie_orders_company_date
    ON productie_orders(company_id, production_date);
