-- SEC-07/08: scope the activity log to a company. audit_log had no company_id, so
-- get_activity_log() / export_activity_log_csv() returned EVERY company's invoice / ANAF /
-- delete activity to whoever was viewing — a cross-company data-isolation leak in the
-- multi-tenant setup. company_id is now populated by log_user_action for company-scoped
-- events; global maintenance events (action = 'background_task_run') stay NULL and remain
-- visible to every company. Pre-migration rows have NULL company_id (shown to none, except
-- the background_task_run carve-out) — acceptable: the activity log is a recent, non-fiscal feature.
ALTER TABLE audit_log ADD COLUMN company_id TEXT;
CREATE INDEX IF NOT EXISTS idx_audit_company ON audit_log(company_id);
