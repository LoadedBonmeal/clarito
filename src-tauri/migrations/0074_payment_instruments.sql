-- Migration 0074: payment instruments (CEC + BO) register
-- Legea 58/1934 (CEC) + Legea 59/1934 (BO) + OMFP 1802/2014 monografie

CREATE TABLE payment_instruments (
    id TEXT PRIMARY KEY NOT NULL,
    company_id TEXT NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('CEC', 'BO')),
    direction TEXT NOT NULL CHECK(direction IN ('received', 'issued')),
    partner_id TEXT,
    partner_cui TEXT,
    -- Număr instrument (nr. cec / nr. bilet la ordin)
    number TEXT,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'RON',
    issue_date TEXT NOT NULL,
    -- Legea 59/1934 art.29: CEC = plătibil la vedere → NO scadenta; BO MUST have scadenta
    scadenta TEXT,
    status TEXT NOT NULL DEFAULT 'registered'
        CHECK(status IN ('registered', 'deposited', 'discounted', 'collected', 'paid', 'dishonored')),
    -- Discount amount (only for BO direction=received discounted at bank, 5114 path)
    discount_amount TEXT,
    -- Commission amount (optional bank commission for discounting/deposit)
    commission_amount TEXT,
    notes TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- CEC: fara scadenta (Legea 58/1934 art.32 — plătibil la vedere)
    -- BO: scadenta obligatorie (Legea 59/1934)
    CHECK((kind = 'CEC' AND scadenta IS NULL) OR (kind = 'BO' AND scadenta IS NOT NULL))
);

CREATE INDEX idx_pi_company ON payment_instruments(company_id);
CREATE INDEX idx_pi_partner_cui ON payment_instruments(company_id, partner_cui);
CREATE INDEX idx_pi_status ON payment_instruments(company_id, status);

-- Backfill chart-of-accounts for existing companies (idempotent via NOT EXISTS)
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '5112' AS code, 'Cecuri de încasat' AS name, 5 AS class
    UNION ALL SELECT '5113', 'Efecte de încasat', 5
    UNION ALL SELECT '5114', 'Efecte remise spre scontare', 5
    UNION ALL SELECT '413',  'Efecte de primit de la clienți', 4
    UNION ALL SELECT '403',  'Efecte de plătit', 4
    UNION ALL SELECT '405',  'Efecte de plătit pentru imobilizări', 4
    UNION ALL SELECT '667',  'Cheltuieli privind sconturile acordate', 6
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
