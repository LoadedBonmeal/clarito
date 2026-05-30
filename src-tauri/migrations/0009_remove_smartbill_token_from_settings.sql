-- SEC-01: Remove plaintext SmartBill tokens from settings table.
-- Tokens are now stored exclusively in the OS keychain.
-- Existing users will need to re-enter their SmartBill API token.
DELETE FROM settings WHERE key LIKE 'smartbill_token_%';
