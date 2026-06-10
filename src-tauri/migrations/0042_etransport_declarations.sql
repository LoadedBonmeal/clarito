-- Migration 0042: evidența declarațiilor RO e-Transport transmise (audit r3 W6). Codul UIT primit
-- de la ANAF e valabil 5 zile (transport național, cod operațiune 30) respectiv 15 zile (operațiuni
-- intracomunitare / import-export) de la transmitere — fără evidență, un transport poate pleca cu
-- un UIT expirat (sancționabil). Sumele/atributele mărfii rămân în XML-ul generat; aici păstrăm
-- doar ce e necesar urmăririi (UIT, termen, stare).
CREATE TABLE IF NOT EXISTS etransport_declarations (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    uit             TEXT,                                -- codul UIT (NULL dacă ANAF nu l-a întors încă)
    index_incarcare TEXT    NOT NULL DEFAULT '',
    cod_tip_operatiune TEXT NOT NULL DEFAULT '',
    partner_name    TEXT    NOT NULL DEFAULT '',
    vehicle         TEXT    NOT NULL DEFAULT '',
    test_mode       INTEGER NOT NULL DEFAULT 0,
    submitted_at    INTEGER NOT NULL,                    -- unix epoch
    expires_at      INTEGER NOT NULL,                    -- unix epoch (5/15 zile după transmitere)
    created_at      INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_etransport_decl_company
    ON etransport_declarations(company_id, submitted_at DESC);
