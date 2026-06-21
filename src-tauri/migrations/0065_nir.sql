-- 0065: NIR (Notă de Intrare Recepție) — formular 14-3-1A (OMFP 2634/2015)
--
-- Documentul justificativ pentru intrarea mărfurilor în gestiune (recepție de la furnizor).
-- Suportă atât gestiunea cantitativ-valorică (cost de achiziție) cât și gestiunea global-valorică
-- (prețuri de amănunt: cost + adaos comercial + TVA neexigibilă = prețul de vânzare).
--
-- GL flow (fără dublă-contare):
--   1. Factura primită postează D607=C401 (generate_gl_entries, source_type='RECEIVED_INVOICE')
--   2. La finalizarea NIR, record_movement(Dir::In) postează D371=C607 (source_type='STOCK')
--   => NET: D371=C401 (stocul capitalizat, 607 se anulează)
-- La modul amănunt, se mai postează: D371=C378 (adaos) + D371=C4428 (TVA neexigibilă)
-- => Soldul 371 = cost + adaos + TVA neexig. = prețul de amănunt

CREATE TABLE IF NOT EXISTS nir_documents (
    id                   TEXT    NOT NULL PRIMARY KEY,
    company_id           TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    gestiune_id          TEXT    NOT NULL REFERENCES gestiune(id),
    received_invoice_id  TEXT    REFERENCES received_invoices(id),
    supplier_name        TEXT,
    supplier_cui         TEXT,
    nir_series           TEXT,
    nir_number           INTEGER NOT NULL,
    nir_date             TEXT    NOT NULL,       -- ISO YYYY-MM-DD
    retail_mode          INTEGER NOT NULL DEFAULT 0,  -- 0=cantitativ-valoric, 1=global-valoric amănunt
    status               TEXT    NOT NULL DEFAULT 'draft',  -- 'draft' | 'finalized'
    comisie_receptie     TEXT,   -- membrii comisiei (text liber, tipărit pe formular)
    observatii           TEXT,
    created_at           INTEGER NOT NULL,
    finalized_at         INTEGER,
    UNIQUE(company_id, nir_series, nir_number)
);

CREATE TABLE IF NOT EXISTS nir_lines (
    id               TEXT    NOT NULL PRIMARY KEY,
    nir_id           TEXT    NOT NULL REFERENCES nir_documents(id) ON DELETE CASCADE,
    product_id       TEXT    REFERENCES products(id),
    denumire         TEXT    NOT NULL,
    um               TEXT,
    qty              TEXT    NOT NULL,           -- cantitate recepționată (6 zec.)
    unit_cost        TEXT    NOT NULL,           -- prețul unitar fără TVA
    vat_rate         TEXT    NOT NULL,           -- cota TVA (e.g. "19", "9", "0")
    adaos_pct        TEXT,                       -- procentul de adaos (modul amănunt)
    value_cost       TEXT    NOT NULL,           -- qty × unit_cost
    value_adaos      TEXT    NOT NULL DEFAULT '0.00',   -- value_cost × adaos_pct/100
    value_tva_neex   TEXT    NOT NULL DEFAULT '0.00',   -- (value_cost+value_adaos) × vat_rate/100
    pret_amanunt     TEXT    NOT NULL DEFAULT '0.00',   -- value_cost + value_adaos + value_tva_neex
    line_no          INTEGER NOT NULL
);

-- Adăugăm coloane pentru șeria/numărul NIR la nivelul companiei
ALTER TABLE companies ADD COLUMN nir_series TEXT;
ALTER TABLE companies ADD COLUMN last_nir_number INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_nir_documents_company ON nir_documents(company_id, nir_date);
CREATE INDEX IF NOT EXISTS idx_nir_documents_invoice ON nir_documents(received_invoice_id);
CREATE INDEX IF NOT EXISTS idx_nir_lines_nir ON nir_lines(nir_id, line_no);
