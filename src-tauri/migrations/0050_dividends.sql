-- Dividende repartizate (impozit pe dividende) — Legea 141/2025: 16% pentru dividende DISTRIBUITE
-- de la 01.01.2026 (10% tranzitoriu pentru dividende din situații financiare interimare 2025, chiar
-- dacă sunt plătite în 2026). Fiecare înregistrare postează nota 117/457/446 și alimentează obligația
-- de impozit pe dividende (decl. 100), scadentă pe 25 a lunii următoare plății (ori 25 ian pentru
-- dividende distribuite dar neplătite).
CREATE TABLE IF NOT EXISTS dividends (
    id                TEXT PRIMARY KEY,
    company_id        TEXT NOT NULL,
    distribution_date TEXT NOT NULL,             -- YYYY-MM-DD (determină cota 16%/10%)
    payment_date      TEXT,                       -- YYYY-MM-DD; NULL = distribuit, neplătit încă
    gross_amount      TEXT NOT NULL,              -- rust_decimal serializat ca TEXT
    tax_rate          INTEGER NOT NULL,           -- 16 sau 10
    tax_amount        TEXT NOT NULL,
    net_amount        TEXT NOT NULL,
    interim_2025      INTEGER NOT NULL DEFAULT 0, -- 1 = situații interimare 2025 (cota tranzitorie 10%)
    shareholder       TEXT,
    note              TEXT,
    created_at        INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dividends_company_period
    ON dividends(company_id, distribution_date);
