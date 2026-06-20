-- Registru-inventar + inventariere (Legea 82/1991 art. 7/20/25; OMFP 2861/2009; OMFP 2634/2015)
-- Forme: Registru-inventar cod 14-1-2, Listă de inventariere cod 14-3-12.

CREATE TABLE IF NOT EXISTS inventory_sessions (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    reference_date  TEXT    NOT NULL,  -- YYYY-MM-DD: data de referință a inventarierii
    fiscal_year     INTEGER NOT NULL,
    -- ANUAL | INCEPERE | INCETARE | PREDARE_GESTIUNE | CALAMITATE (OMFP 2861/2009 art. 1)
    type            TEXT    NOT NULL DEFAULT 'ANUAL',
    gestiune        TEXT,              -- numele/codul gestiunii (NULL = toate gestiunile)
    status          TEXT    NOT NULL DEFAULT 'DRAFT',  -- DRAFT | FINALIZED
    comisie_members TEXT    NOT NULL DEFAULT '[]',     -- JSON array de membri comisie
    notes           TEXT,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS inventory_lines (
    id              TEXT    PRIMARY KEY NOT NULL,
    session_id      TEXT    NOT NULL REFERENCES inventory_sessions(id) ON DELETE CASCADE,
    account_code    TEXT    NOT NULL,  -- contul de stoc (371, 301, 345 etc.)
    item_name       TEXT    NOT NULL,
    um              TEXT    NOT NULL DEFAULT 'buc',
    qty_scriptic    TEXT    NOT NULL DEFAULT '0.000000',  -- stoc scriptic (din sistem)
    qty_faptic      TEXT    NOT NULL DEFAULT '0.000000',  -- stoc faptic (măsurat/numărat)
    unit_price      TEXT    NOT NULL DEFAULT '0.00',      -- cost unitar (la data inventarierii)
    value_contabila TEXT    NOT NULL DEFAULT '0.00',      -- = qty_scriptic × unit_price
    value_inventar  TEXT    NOT NULL DEFAULT '0.00',      -- = qty_faptic × unit_price
    diff_value      TEXT    NOT NULL DEFAULT '0.00',      -- = value_inventar − value_contabila (semnat)
    -- perisabilitati | imputabil | neimputabil | depreciere | altele
    diff_cause      TEXT,
    -- 1 = lipsă imputată unui gestionar (art. 275 C. fiscal; 461/4282 = 758+TVA)
    imputable       INTEGER NOT NULL DEFAULT 0,
    product_id      TEXT,              -- referință la products.id (NULL pentru conturi fără produs)
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Registru-inventar cod 14-1-2 (OMFP 2634/2015):
-- col 1: Nr. crt. | col 2: Recapitulație | col 3: Valoare contabilă |
-- col 4: Valoare inventar | col 5: Diferențe | col 6: Cauze diferențe
CREATE TABLE IF NOT EXISTS registru_inventar_entries (
    id                  TEXT    PRIMARY KEY NOT NULL,
    company_id          TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    fiscal_year         INTEGER NOT NULL,
    seq_no              INTEGER NOT NULL,      -- nr. crt. din registru (secvențial per companie/an)
    recap_text          TEXT    NOT NULL,      -- col 2: recapitulația (ex. "Mărfuri ct. 371")
    value_contabila     TEXT    NOT NULL DEFAULT '0.00',
    value_inventar      TEXT    NOT NULL DEFAULT '0.00',
    diff_value          TEXT    NOT NULL DEFAULT '0.00',  -- semnat
    diff_cause          TEXT    NOT NULL DEFAULT '',
    source_session_id   TEXT,              -- sesiunea de inventariere sursă (NULL = intrare manuală)
    created_at          INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, fiscal_year, seq_no)
);

CREATE INDEX IF NOT EXISTS idx_inventory_sessions_company ON inventory_sessions(company_id, fiscal_year);
CREATE INDEX IF NOT EXISTS idx_inventory_lines_session ON inventory_lines(session_id);
CREATE INDEX IF NOT EXISTS idx_registru_inventar_company_year ON registru_inventar_entries(company_id, fiscal_year, seq_no);
