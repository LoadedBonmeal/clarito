-- Migration 0085: FX treasury revaluation table (reevaluare trezoreriei valutare lunară)
-- Stochează rezultatul reevaluării per cont de trezorerie per perioadă (5124/5314).
-- Separat de fx_revaluation (care ține per-factură) dar parte din același jurnal FX_REVAL.
--
-- OMFP 1802/2014 pct.304(3)-(4) + art.322: disponibilitățile valutare se reevaluează
-- la cursul BNR din ultima zi lucrătoare a lunii. Diferentele se înregistrează la 665/765.

CREATE TABLE IF NOT EXISTS fx_treasury_revaluation (
    id              TEXT NOT NULL PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- Perioada de reevaluare: "YYYY-MM"
    period          TEXT NOT NULL,
    -- "BANK" (5124 — cont bancar valutare) sau "CASH" (5314 — casă în valută)
    treasury_kind   TEXT NOT NULL CHECK (treasury_kind IN ('BANK','CASH')),
    -- Referința contului: bank_account.id pentru BANK, contul analitic pentru CASH
    account_ref     TEXT NOT NULL,
    -- Codul GL (5124 sau 5314)
    gl_account      TEXT NOT NULL,
    -- Valuta (ex. "EUR")
    currency        TEXT NOT NULL,
    -- Soldul valutar la sfârșitul perioadei (TEXT/Decimal)
    foreign_balance TEXT NOT NULL,
    -- Cursul BNR din ultima zi bancară a perioadei
    month_end_rate  TEXT NOT NULL,
    -- Valoarea în lei la care soldul era evaluat ÎNAINTE de această reevaluare
    -- (= revalued_lei din reevaluarea anterioară, sau sold_lei din GL dacă e prima dată)
    prior_lei       TEXT NOT NULL,
    -- Valoarea în lei la cursul month_end_rate (round2)
    revalued_lei    TEXT NOT NULL,
    -- Diferența (signed): revalued_lei - prior_lei
    diff_lei        TEXT NOT NULL,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),

    UNIQUE(company_id, period, treasury_kind, account_ref, currency)
);

CREATE INDEX IF NOT EXISTS fx_treasury_reval_company_period
    ON fx_treasury_revaluation(company_id, period);
