-- Migration 0048: backfill 4373 «Contribuția pentru concedii și indemnizații» into existing companies'
-- charts — the liability account the payroll posting credits for the employer 0.85% CCI contribution
-- (OUG 158/2005 art. 4 alin. (2); D 6458 / C 4373). Idempotent via the NOT EXISTS guard. Matches the
-- 0038_payroll_6458 backfill pattern.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, '4373',
       'Contribuția pentru concedii și indemnizații', 4
FROM companies c
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = '4373'
);
