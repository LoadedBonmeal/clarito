-- Migration 0038: backfill 6458 «Alte cheltuieli privind asigurările și protecția socială» into
-- existing companies' charts — the account the payroll posting uses for the employer-borne part-time
-- minimum-base CAS/CASS difference (art. 146 (5^6) Cod fiscal). Idempotent via the NOT EXISTS guard.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, '6458',
       'Alte cheltuieli privind asigurările și protecția socială', 6
FROM companies c
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = '6458'
);
