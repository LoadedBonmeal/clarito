-- P2 Wave 7: payroll GL account map + diurnă (per company override; NULL = use code default).
CREATE TABLE IF NOT EXISTS payroll_config (
    id                         TEXT    NOT NULL PRIMARY KEY,
    company_id                 TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- GL accounts (NULL → code default)
    cont_cheltuieli_salarii    TEXT,   -- default 641
    cont_salarii_datorate      TEXT,   -- default 421
    cont_cas                   TEXT,   -- default 4315
    cont_cass                  TEXT,   -- default 4316
    cont_impozit               TEXT,   -- default 444
    cont_cheltuieli_cam        TEXT,   -- default 646
    cont_cam                   TEXT,   -- default 436
    cont_concedii              TEXT,   -- default 4373
    cont_cheltuieli_concedii   TEXT,   -- default 6458
    cont_net_casa              TEXT,   -- default 5311
    cont_net_banca             TEXT,   -- default 5121
    -- Diurnă (stored as TEXT Decimal strings)
    diurna_interna             TEXT,   -- default "23.00" (lei/zi, CF art.76(2)(k))
    diurna_plafon_neimpozabil  TEXT,   -- default "57.50" (2.5 × 23 lei, CF art.142(g))
    diurna_cazare              TEXT,   -- default "265.00" (lei/noapte)
    updated_at                 INTEGER NOT NULL,
    UNIQUE(company_id)
);
