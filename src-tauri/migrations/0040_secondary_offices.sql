-- Migration 0040: sedii secundare / puncte de lucru (D112 angajatorF2). Impozitul pe veniturile din
-- salarii datorat la un sediu secundar (punct de lucru cu ≥ 5 salariați) se declară separat în D112,
-- per CIF sediu secundar. Salariații se repartizează la sedii prin employees.sediu_cif ('' = sediul
-- principal). F2 din D112 are doar (cif, nr.crt, sume) — numele e doar pentru UI.
CREATE TABLE IF NOT EXISTS secondary_offices (
    id          TEXT    PRIMARY KEY NOT NULL,
    company_id  TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    cif         TEXT    NOT NULL,
    name        TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(company_id, cif)
);

ALTER TABLE employees ADD COLUMN sediu_cif TEXT NOT NULL DEFAULT '';
