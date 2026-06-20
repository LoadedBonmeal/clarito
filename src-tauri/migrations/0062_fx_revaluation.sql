-- Migration 0062: FX revaluation table (reevaluare valutară lunară)
-- Stochează rezultatul reevaluării per factură per perioadă.
-- Idempotentă: UNIQUE(company_id, period, invoice_id, invoice_kind) + ON CONFLICT REPLACE.

CREATE TABLE IF NOT EXISTS fx_revaluation (
    id              TEXT NOT NULL PRIMARY KEY,
    company_id      TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- Perioada de reevaluare: "YYYY-MM"
    period          TEXT NOT NULL,
    -- Factura reevaluată
    invoice_id      TEXT NOT NULL,
    -- "ISSUED" = factură emisă (creanță 4111) / "RECEIVED" = factură primită (datorie 401)
    invoice_kind    TEXT NOT NULL CHECK (invoice_kind IN ('ISSUED','RECEIVED')),
    -- Valuta facturii (ex. "EUR")
    currency        TEXT NOT NULL,
    -- Sold valutar deschis (foreign_total - foreign_paid), TEXT/Decimal
    foreign_outstanding TEXT NOT NULL,
    -- Cursul BNR din ultima zi bancară a perioadei
    month_end_rate  TEXT NOT NULL,
    -- Cursul la care soldul era evaluat ÎNAINTE de această reevaluare
    -- (= month_end_rate din reevaluarea anterioară SAU exchange_rate booking dacă e prima)
    prior_rate      TEXT NOT NULL,
    -- Valoarea în lei la cursul month_end_rate (round2)
    revalued_lei    TEXT NOT NULL,
    -- Valoarea în lei la cursul prior_rate (round2)
    prior_lei       TEXT NOT NULL,
    -- Diferența (signed): revalued_lei - prior_lei
    diff_lei        TEXT NOT NULL,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),

    UNIQUE(company_id, period, invoice_id, invoice_kind)
);

CREATE INDEX IF NOT EXISTS fx_reval_company_period
    ON fx_revaluation(company_id, period);
