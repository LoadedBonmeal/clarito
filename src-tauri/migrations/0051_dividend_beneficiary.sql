-- D205 (declarația informativă anuală, pe beneficiar — OPANAF 179/2022 mod. 102/2025): impozitul pe
-- dividende reținut la sursă se raportează per persoană fizică beneficiară. Adăugăm CNP-ul (cifR, N13
-- mod-11) + flag rezident (Rezid; 1 = rezident, calea D205; 0 = nerezident, ar merge pe D207) pe fiecare
-- distribuire. Numele beneficiarului (den1) REFOLOSEȘTE coloana `shareholder` (text liber existent).
-- Coloanele sunt opționale (regression-safe): D205 le cere la EXPORT, nu la înregistrare. Vezi
-- src-tauri/D205_EMITTER_DESIGN.md.
ALTER TABLE dividends ADD COLUMN beneficiary_cnp TEXT;
ALTER TABLE dividends ADD COLUMN beneficiary_resident INTEGER NOT NULL DEFAULT 1;
