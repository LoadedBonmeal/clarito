-- D112 asiguratD emission: the two required certificate fields the register didn't capture yet.
-- D_10 = locul de prescriere (Nomenclator 8; live XSD types IntInt1_4 → valid domain 1–4:
-- 1 medic de familie, 2 spital, 3 ambulatoriu, 4 CAS). D_23 = codul de boală (diagnostic) de pe
-- certificat, max 3 caractere; "RM" pentru risc maternal (D_9=15). NOTE: structura v7 names this
-- D_23, OPANAF 299/2025 names it D_22 — the emitter pins the attribute token to a single constant.
-- Defaults keep pre-existing rows valid (loc=1 medic familie, cod="RM").
ALTER TABLE medical_leaves ADD COLUMN loc_prescriere INTEGER NOT NULL DEFAULT 1;
ALTER TABLE medical_leaves ADD COLUMN cod_boala      TEXT    NOT NULL DEFAULT 'RM';
