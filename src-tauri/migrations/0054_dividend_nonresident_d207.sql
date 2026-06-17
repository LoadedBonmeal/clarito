-- D207 (non-resident dividend declaration) needs two fields the model lacked: the beneficiary's
-- country of residence (Stat_R, a 2-char ANAF country code) and their foreign tax id (cifS / NIF).
-- Both are only relevant for non-residents (beneficiary_resident = 0); nullable for residents.
ALTER TABLE dividends ADD COLUMN beneficiary_country TEXT;
ALTER TABLE dividends ADD COLUMN beneficiary_foreign_tax_id TEXT;
