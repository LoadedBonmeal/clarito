-- Migration 0041: concedii medicale (OUG 158/2005) — registru de certificate de concediu medical,
-- sursa blocului D112 asiguratD (per certificat) + rollup-urile asiguratB3 (B3_12=ΣD_20 indemnizație
-- angajator, B3_13=ΣD_21 indemnizație FNUASS) și recuperarea FNUASS (angajatorC2). Sumele se
-- stochează ca TEXT (convenția Decimal-as-TEXT); zilele ca întregi.
CREATE TABLE IF NOT EXISTS medical_leaves (
    id              TEXT    PRIMARY KEY NOT NULL,
    company_id      TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    employee_id     TEXT    NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    period_ym       TEXT    NOT NULL,                 -- 'YYYY-MM' luna de raportare
    serie           TEXT    NOT NULL DEFAULT '',      -- D_1 seria certificatului
    numar           TEXT    NOT NULL DEFAULT '',      -- D_2 numărul certificatului
    cod_indemnizatie TEXT   NOT NULL DEFAULT '01',    -- D_9 cod indemnizație (Nomenclator 9)
    data_acordare   TEXT    NOT NULL DEFAULT '',      -- D_5 (YYYY-MM-DD)
    data_inceput    TEXT    NOT NULL DEFAULT '',      -- D_6
    data_sfarsit    TEXT    NOT NULL DEFAULT '',      -- D_7
    zile_angajator  INTEGER NOT NULL DEFAULT 0,       -- D_14 zile suportate de angajator
    zile_fnuass     INTEGER NOT NULL DEFAULT 0,       -- D_15 zile suportate din FNUASS
    baza_calcul     TEXT    NOT NULL DEFAULT '0.00',  -- D_17 baza de calcul (venituri 6 luni)
    zile_baza       INTEGER NOT NULL DEFAULT 0,       -- D_18 nr. zile aferente bazei
    suma_angajator  TEXT    NOT NULL DEFAULT '0.00',  -- D_20 indemnizație suportată de angajator
    suma_fnuass     TEXT    NOT NULL DEFAULT '0.00',  -- D_21 indemnizație suportată din FNUASS
    procent         INTEGER NOT NULL DEFAULT 75,      -- D_28 procent (75/65/55)
    created_at      INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_medical_leaves_period
    ON medical_leaves(company_id, period_ym);
