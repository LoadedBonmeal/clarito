-- Migration 0049: backfill 4373 «Contribuția pentru concedii și indemnizații» and
-- 4382 «Alte creanțe privind asigurările sociale» (FNUASS receivable) into existing
-- companies' charts — both accounts are referenced by payroll GL postings but were
-- absent from the seed. Idempotent via NOT EXISTS guard. Mirrors 0048_payroll_concedii_4373.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, '4373',
       'Contribuția pentru concedii și indemnizații', 4
FROM companies c
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = '4373'
);

INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, '4382',
       'Alte creanțe privind asigurările sociale', 4
FROM companies c
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = '4382'
);
