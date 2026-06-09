-- Migration 0030: align the VAT-rate catalog with Legea 141/2025 (rates effective 1 Aug 2025).
--
-- From 1 Aug 2025: standard rate 21%, a SINGLE reduced rate 11% (replacing the old 5%/9% split),
-- and a transitional 9% only for qualifying housing until 31 Jul 2026 (art. III). The 19% and 5%
-- rates are ABOLISHED for new operations.
--
-- We DEACTIVATE 19% and 5% so they no longer appear in the rate picker for new invoices, but keep
-- the rows in the catalog so: (a) storno / credit notes of pre-1-Aug-2025 invoices still resolve
-- (they copy the original rate), (b) historical invoices render, and (c) VALID_VAT_RATES still
-- accepts them. 9% stays active (transitional housing) but is relabelled to make its scope clear.
UPDATE vat_rates SET active = 0, label = 'Standard 19% (abrogat 01.08.2025)' WHERE id = 'vat-19';
UPDATE vat_rates SET active = 0, label = 'Redus 5% (abrogat 01.08.2025)'     WHERE id = 'vat-5';
UPDATE vat_rates SET label = 'Standard 21%',                                 sort_order = 0 WHERE id = 'vat-21';
UPDATE vat_rates SET label = 'Redus 11%',                                    sort_order = 1 WHERE id = 'vat-11';
UPDATE vat_rates SET label = 'Redus 9% (locuințe, până la 31.07.2026)',      sort_order = 2 WHERE id = 'vat-9';
UPDATE vat_rates SET label = 'Cotă zero 0%',                                 sort_order = 3 WHERE id = 'vat-0';
