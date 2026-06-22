-- Migration 0086: Add FX (foreign-currency) columns to gl_entry.
--
-- These columns track the foreign-currency amount and currency code for entries
-- posted on treasury accounts (5124/5314) in foreign currency. They are nullable:
-- NULL means the entry is in RON and no foreign amount is applicable.
--
-- Required for:
--   - Month-end FX revaluation of 5124/5314 (OMFP 1802/2014 pct.304(3))
--   - SAF-T D406 AmountCurrencyDebit/Credit fields

ALTER TABLE gl_entry ADD COLUMN amount_fx_foreign TEXT;
-- ISO 4217 currency code (e.g. "EUR", "USD"); NULL for RON entries
ALTER TABLE gl_entry ADD COLUMN currency_code TEXT;

CREATE INDEX IF NOT EXISTS idx_gl_entry_currency
    ON gl_entry(currency_code)
    WHERE currency_code IS NOT NULL;
