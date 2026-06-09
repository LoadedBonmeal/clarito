-- Migration 0037: per-employee D112 special attributes (part-time, pensionar, insurance/contract
-- type) so the D112 XML emits the correct asiguratA fields instead of hardcoded defaults.
ALTER TABLE employees ADD COLUMN tip_asigurat TEXT    NOT NULL DEFAULT '1';  -- Nomenclator 5 → A_1
ALTER TABLE employees ADD COLUMN pensionar    INTEGER NOT NULL DEFAULT 0;    -- → A_2 (0/1)
ALTER TABLE employees ADD COLUMN tip_contract TEXT    NOT NULL DEFAULT 'N';  -- Nomenclator 12 → A_3 ('N','P1'..'P7')
ALTER TABLE employees ADD COLUMN ore_norma    INTEGER NOT NULL DEFAULT 8;    -- → A_4 (6/7/8)
