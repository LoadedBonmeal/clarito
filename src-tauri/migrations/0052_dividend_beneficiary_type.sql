-- D100: impozitul pe dividende se declară pe DOUĂ obligații distincte din Nomenclator, după tipul
-- beneficiarului: persoană FIZICĂ (art. 97 Cod fiscal) = poziția 6 → cod_oblig 604; persoană JURIDICĂ
-- (art. 43) = poziția 4 → cod_oblig 150. Adăugăm tipul beneficiarului ca să separăm rândurile D100 și
-- să excludem persoanele juridice din D205 (care raportează doar persoane fizice, ca și nerezidenții).
-- Implicit 'PF' (cazul uzual SRL → asociat persoană fizică); regression-safe pentru datele existente.
ALTER TABLE dividends ADD COLUMN beneficiary_type TEXT NOT NULL DEFAULT 'PF';
