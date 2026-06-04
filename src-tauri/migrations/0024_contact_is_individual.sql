-- Migration 0024: contacts.is_individual — mark a contact as an individual/consumer
-- (persoană fizică) for B2C e-Factura.
--
-- B2C e-Factura is mandatory in 2026. An individual consumer has no CUI; per ANAF's
-- convention the buyer is identified with the placeholder "0000000000000" (13 zeros)
-- in the UBL. When this flag is set, rule BR-RO-016 stops requiring a CUI for the
-- buyer and the generator emits the placeholder; B2B (company) buyers still require a
-- CUI, preserving the "you forgot the CUI" safety net.
--
-- Default 0 (false): existing contacts are treated as companies (backward-compatible).

ALTER TABLE contacts ADD COLUMN is_individual INTEGER NOT NULL DEFAULT 0;
