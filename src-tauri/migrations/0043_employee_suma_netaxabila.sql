-- Suma netaxabilă din salariul minim (art. III OUG 89/2025): 300 lei/lună sem. I 2026,
-- 200 lei/lună sem. II 2026, scutită de impozit + CAS/CASS/CAM pentru salariații cu normă
-- întreagă al căror salariu de bază = salariul minim. Accountant attestation flag (the law's
-- "base = min wage" + "no wage cut between 01.01.2026–31.12.2026" conditions can't be derived
-- from the data the app stores). The gross-ceiling guard (≤4.300/4.600) is applied in code.
ALTER TABLE employees ADD COLUMN beneficiar_suma_netaxabila INTEGER NOT NULL DEFAULT 0;
