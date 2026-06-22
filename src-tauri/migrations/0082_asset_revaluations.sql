-- Migration 0082: reevaluare imobilizări corporale (OMFP 1802/2014, pct.100).
--
-- Tabel: asset_revaluations
--   Înregistrează fiecare eveniment de reevaluare per activ.
--   Valorile monetare sunt TEXT (convenția Decimal-as-TEXT).
--
-- Metoda valorii nete (net-value method): se elimină amortizarea cumulată contra contului 21x,
-- apoi valoarea netă este ajustată la valoarea justă.
--
-- Reguli de compensare per activ (pct.100):
--   CREȘTERE fără deficit anterior → C 105 (rezervă din reevaluare)
--   CREȘTERE care acoperă un deficit 655 anterior → C 7558 (până la nivelul deficitului), C 105 rest
--   DESCREȘTERE cu rezervă 105 disponibilă → D 105 (până la rezerva disponibilă), D 655 rest
--   DESCREȘTERE fără rezervă → D 655 (cheltuieli din reevaluare)

CREATE TABLE IF NOT EXISTS asset_revaluations (
    id              TEXT    NOT NULL PRIMARY KEY,
    company_id      TEXT    NOT NULL,
    asset_id        TEXT    NOT NULL REFERENCES fixed_assets(id) ON DELETE CASCADE,
    revaluation_date TEXT   NOT NULL,          -- YYYY-MM-DD
    fair_value      TEXT    NOT NULL,          -- valoarea justă (Decimal-as-TEXT)
    prior_net_value TEXT    NOT NULL,          -- valoarea netă contabilă înainte (cost − amort)
    prior_cost      TEXT    NOT NULL,          -- costul de intrare înainte
    prior_accumulated TEXT  NOT NULL,          -- amortizarea cumulată înainte
    surplus_or_deficit TEXT NOT NULL,          -- fair_value − prior_net_value (pozitiv = surplus)
    reserve_movement TEXT   NOT NULL DEFAULT '0', -- cât s-a debitat/creditat pe 105 (+ = credit, - = debit)
    income_amount   TEXT    NOT NULL DEFAULT '0', -- suma pe 7558 (venituri din reevaluare)
    expense_amount  TEXT    NOT NULL DEFAULT '0', -- suma pe 655 (cheltuieli din reevaluare)
    method          TEXT    NOT NULL DEFAULT 'net_value', -- metoda contabilă
    created_at      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_asset_revaluations_company_asset
    ON asset_revaluations(company_id, asset_id, revaluation_date);

-- Conturi necesare pentru reevaluare (adăugate la planul de conturi al companiei via seed_standard;
-- companiile existente le vor dobândi la următoarea migrare a bazei de date prin seed adăugat în
-- standard_accounts() — migrarea nu poate insera per-companie fără a cunoaște company_id-urile).
-- Nu inserăm în chart_of_accounts din migrare (nu cunoaștem company_id-urile existente).
-- Conturile sunt adăugate în standard_accounts() din accounts.rs și vor fi disponibile
-- companiilor noi sau după un re-seed manual.
