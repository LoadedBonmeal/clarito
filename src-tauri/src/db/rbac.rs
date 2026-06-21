//! Role-based access control primitives.
//!
//! This module is **pure logic** — no database, no I/O, no Tauri runtime.
//! The gate in `lib.rs` calls `required_perm` and `role_allows` from a sync
//! closure, so they must be lock-free and infallible.

use serde::{Deserialize, Serialize};

// ─── Role ────────────────────────────────────────────────────────────────────

/// The four application roles, ordered by privilege (Admin is highest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Contabil,
    Operator,
    Viewer,
}

impl Role {
    /// Parse from the string stored in the `users.role` DB column.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "contabil" => Some(Self::Contabil),
            "operator" => Some(Self::Operator),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Contabil => "contabil",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }

    /// Encode as u8 for the AtomicU8 in AppState (lock-free gate read).
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Admin => 0,
            Self::Contabil => 1,
            Self::Operator => 2,
            Self::Viewer => 3,
        }
    }

    /// Decode from u8 stored in AppState AtomicU8.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Admin),
            1 => Some(Self::Contabil),
            2 => Some(Self::Operator),
            3 => Some(Self::Viewer),
            _ => None,
        }
    }
}

// ─── Permission ──────────────────────────────────────────────────────────────

/// Permissions used to gate individual Tauri commands.
/// Only *sensitive* commands require a specific permission; every other
/// authenticated command is allowed with `None` from `required_perm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perm {
    /// Post GL entries, run reconciliation, run depreciation.
    PostGl,
    /// Close a GL/VAT period.
    ClosePeriod,
    /// Actually submit to ANAF (not just generate/preview XML).
    SubmitAnaf,
    /// Manage users (create, update role, deactivate, reset password).
    ManageUsers,
    /// Write application settings.
    WriteSettings,
    /// Create/update any document (invoice draft, etc.).
    CreateDraft,
    /// View any report or ledger.
    ViewReports,
    /// Hard-delete a document.
    Delete,
}

/// Permission matrix (role × perm → bool).
///
/// ```
/// use efactura_desktop_lib::db::rbac::{role_allows, Role, Perm};
/// assert!(role_allows(Role::Admin, Perm::ManageUsers));
/// assert!(!role_allows(Role::Viewer, Perm::PostGl));
/// ```
pub fn role_allows(role: Role, perm: Perm) -> bool {
    match role {
        // Admin: all permissions.
        Role::Admin => true,
        // Contabil: almost everything except user management.
        Role::Contabil => !matches!(perm, Perm::ManageUsers),
        // Operator: create/view + generate ANAF XML; NOT post GL, NOT submit, NOT settings write, NOT manage users.
        Role::Operator => matches!(perm, Perm::CreateDraft | Perm::ViewReports),
        // Viewer: read-only.
        Role::Viewer => matches!(perm, Perm::ViewReports),
    }
}

// ─── Command → Permission map ─────────────────────────────────────────────────

/// Map a Tauri command name to the minimum required permission.
///
/// Returns `None` for commands that any authenticated user may call (reads).
/// Returns `Some(Perm)` for sensitive or write commands.
///
/// **Design: TRUE DENY-BY-DEFAULT.**
/// The function checks explicit high-sensitivity perms first, then the
/// ViewReports bucket, then falls through to `is_read_cmd`.  A command is
/// free-to-read (returns `None`) ONLY when `is_read_cmd` returns `true`.
/// Everything else — including any future command not explicitly listed here —
/// falls to `Some(Perm::CreateDraft)`, which Viewer and Operator (for GL)
/// cannot satisfy.  This means adding a new write command without updating
/// this file is safe: it will be denied to Viewer by default.
///
/// NOTE on official `export_*` commands: `export_d300_official`,
/// `export_d390`, `export_d394_official`, `export_saft_official`,
/// `export_d205_official`, `export_d207_official`, `export_bilant_xml`,
/// `export_d112_xml` are all mapped to `SubmitAnaf` in the explicit map
/// BEFORE `is_read_cmd` is evaluated, so the `export_` read-prefix in
/// `is_read_cmd` only applies to non-official CSV/XLSX exports.
pub fn required_perm(cmd: &str) -> Option<Perm> {
    match cmd {
        // ── PostGl ────────────────────────────────────────────────────────
        // run_payroll posts GL entries — must be PostGl, not just CreateDraft.
        // dev_seed writes demo data — keep out of Viewer/Operator reach.
        "generate_gl_entries"
        | "reconcile_gl"
        | "post_income_tax"
        | "post_annual_close"
        | "run_depreciation"
        | "run_payroll"
        | "dispose_asset"
        | "compute_fx_revaluation"
        | "post_inventory_diffs"
        | "produce"
        | "finalize_inventory_session"
        | "record_stock_receipt"
        | "record_stock_issue"
        | "finalize_nir"
        | "transfer_stock"
        // preview_saft_official_xml is read-PREFIXED but its month/quarter branch internally calls
        // generate_gl_entries (a committing GL DELETE+INSERT), so it must be gated like a GL post —
        // otherwise the preview_ read-prefix would free it to a Viewer who could re-post the ledger.
        | "preview_saft_official_xml"
        | "dev_seed" => Some(Perm::PostGl),

        // ── ClosePeriod ───────────────────────────────────────────────────
        "close_vat_period" | "close_period" => Some(Perm::ClosePeriod),

        // ── SubmitAnaf ────────────────────────────────────────────────────
        // smartbill_push_invoice is an external outbound submission — same gate.
        // NOTE: these official export_* commands must stay here (before the
        // is_read_cmd check) so the `export_` read-prefix does NOT free them.
        "anaf_submit_invoice"
        | "export_d300_official"
        | "export_d390"
        | "export_d394_official"
        | "export_saft_official"
        | "export_d205_official"
        | "export_d207_official"
        | "export_bilant_xml"
        | "etransport_submit"
        | "export_d112_xml"
        | "smartbill_push_invoice" => Some(Perm::SubmitAnaf),

        // ── ManageUsers ───────────────────────────────────────────────────
        "list_users" | "create_user" | "update_user" | "reset_password" => Some(Perm::ManageUsers),

        // ── WriteSettings ─────────────────────────────────────────────────
        // anaf_logout / anaf_refresh_certificate / change_archive_location
        // mutate persisted settings and are not safe for Viewer/Operator.
        "set_setting"
        | "set_smartbill_credentials"
        | "clear_smartbill_credentials"
        | "anaf_set_oauth_client_secret"
        // Connecting/disconnecting the company's ANAF identity is a sensitive settings change
        // (a revoke disconnects e-Factura/SPV) — not for viewer/operator.
        | "anaf_authorize"
        | "anaf_revoke_certificate"
        | "anaf_logout"
        | "anaf_refresh_certificate"
        | "change_archive_location"
        | "set_autostart" => Some(Perm::WriteSettings),

        // ── Delete ────────────────────────────────────────────────────────
        // export_backup is listed here (not in the read-prefix branch below)
        // because it writes a backup archive — not a safe read for Viewer.
        "delete_company"
        | "delete_contact"
        | "delete_product"
        | "delete_product_group"
        | "delete_invoice"
        | "delete_payment"
        | "delete_received_payment"
        | "delete_notification"
        | "delete_all_read_notifications"
        | "delete_recurring_invoice"
        | "delete_employee"
        | "delete_secondary_office"
        | "delete_medical_leave"
        | "delete_dividend"
        | "delete_vat_rate"
        | "delete_account"
        | "delete_receipt"
        | "delete_bank_account"
        | "delete_stock_movement"
        | "delete_manual_journal"
        | "delete_inventory_session"
        | "delete_fixed_asset"
        | "delete_gestiune"
        | "delete_fiscal_receipt"
        | "delete_declaration_filing"
        | "wipe_all_data"
        | "delete_bom"
        | "delete_quote"
        | "delete_order"
        | "delete_contract"
        | "export_backup" => Some(Perm::Delete),

        // ── ViewReports ───────────────────────────────────────────────────
        "generate_vat_report"
        | "export_report"
        | "aging_report"
        | "export_aging_csv"
        | "trial_balance"
        | "profit_and_loss"
        | "bilant"
        | "preview_bilant_xml"
        | "journal_register"
        | "general_ledger"
        | "partner_ledger"
        | "export_sales_journal"
        | "export_purchase_journal"
        | "export_saft_d406" => Some(Perm::ViewReports),

        // ── TRUE DENY-BY-DEFAULT fallback ─────────────────────────────────
        // A command is free (None) ONLY when it is a recognized read.
        // Everything else — including anaf_sync_spv, manual_sync,
        // reparse_received_vat, and any future mutation — requires at least
        // CreateDraft, which Viewer cannot satisfy.
        cmd if is_read_cmd(cmd) => None,

        // Unknown / unrecognised → deny (requires CreateDraft).
        _ => Some(Perm::CreateDraft),
    }
}

/// Returns `true` when `cmd` is a known read-only command.
///
/// A command is a read if EITHER:
///   (a) its name starts with a well-known read-only prefix, OR
///   (b) it appears in the explicit allowlist of non-prefixed reads.
///
/// This function is the sole gate for `None` in `required_perm`.
/// Adding a new name here grants Viewer access — review carefully.
#[inline]
fn is_read_cmd(cmd: &str) -> bool {
    // (a) Read-only name prefixes.
    // NOTE: `export_` only frees non-official exports; official ones are
    // already mapped to SubmitAnaf above and never reach this check.
    let read_prefixes = [
        "get_",
        "list_",
        "preview_",
        "compute_",
        "export_",
        "check_",
        "fetch_",
        "search_",
        "validate_",
        "count_",
        "load_",
    ];
    if read_prefixes.iter().any(|&pfx| cmd.starts_with(pfx)) {
        return true;
    }

    // (b) Explicit allowlist — genuine reads that lack a read-only prefix.
    // Verified against the codebase; extend only for confirmed read commands.
    const READ_ALLOWLIST: &[&str] = &[
        "bilant",
        "profit_and_loss",
        "trial_balance",
        "aging_report",
        "partner_ledger",
        "general_ledger",
        "journal_register",
        "stock_ledger",
        "stock_on_hand",
        "tax_regime_status",
        "vat_registration_status",
        "intrastat_status",
        "cash_vat_plafon_status",
        "vat_rate_note",
        "unread_notification_count",
        "preflight_declaration",
        "verify_archive_integrity",
        "verify_invoice_files",
        "gather_diagnostic",
        "open_archive_folder",
        "open_doc_in_browser",
        "resolve_accounts",
        "etva_fetch_precompletat",
        "nir_from_received_invoice",
    ];
    READ_ALLOWLIST.contains(&cmd)
}

// ─── Gate decision (pure, unit-testable) ────────────────────────────────────

/// The authorization decision returned by [`authorize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Call through to the inner handler.
    Allow,
    /// Not logged in and the command is not in PUBLIC_COMMANDS.
    Unauthorized,
    /// Logged in but the role lacks the required permission.
    Forbidden,
}

/// Public commands that bypass authentication entirely.
/// **This list MUST be complete** — any command missing here is unreachable
/// when the user is not yet authenticated (bricks login).
pub const PUBLIC_COMMANDS: &[&str] = &[
    // License — needed before auth (license screen is pre-login)
    "get_license",
    "check_license_validity",
    "start_trial",
    "activate_license",
    // Auth commands — these ARE the login/setup flow
    "auth_status",
    "auth_setup_admin",
    "auth_login",
    "auth_logout",
    // System info — also pre-login (splash screen, updater)
    "get_app_info",
];

/// Pure gate function: decides whether to allow, reject as unauthorized, or
/// forbid as insufficient role. Factored out of the Tauri invoke closure so
/// it can be unit-tested without a live runtime.
pub fn authorize(cmd: &str, authenticated: bool, role: Option<Role>) -> Decision {
    // 1. Public commands always pass.
    if PUBLIC_COMMANDS.contains(&cmd) {
        return Decision::Allow;
    }
    // 2. Non-public commands require authentication.
    if !authenticated {
        return Decision::Unauthorized;
    }
    // 3. For authenticated users, check role if a perm is required.
    if let Some(perm) = required_perm(cmd) {
        let r = role.unwrap_or(Role::Viewer);
        if !role_allows(r, perm) {
            return Decision::Forbidden;
        }
    }
    Decision::Allow
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── PUBLIC_COMMANDS completeness ─────────────────────────────────────

    #[test]
    fn public_commands_contains_all_license_cmds() {
        let license_cmds = [
            "get_license",
            "check_license_validity",
            "start_trial",
            "activate_license",
        ];
        for cmd in license_cmds {
            assert!(
                PUBLIC_COMMANDS.contains(&cmd),
                "PUBLIC_COMMANDS must contain license command '{cmd}'"
            );
        }
    }

    #[test]
    fn public_commands_contains_all_auth_cmds() {
        let auth_cmds = [
            "auth_status",
            "auth_setup_admin",
            "auth_login",
            "auth_logout",
        ];
        for cmd in auth_cmds {
            assert!(
                PUBLIC_COMMANDS.contains(&cmd),
                "PUBLIC_COMMANDS must contain auth command '{cmd}'"
            );
        }
    }

    // ── Role matrix ──────────────────────────────────────────────────────

    #[test]
    fn admin_has_all_perms() {
        for perm in [
            Perm::PostGl,
            Perm::ClosePeriod,
            Perm::SubmitAnaf,
            Perm::ManageUsers,
            Perm::WriteSettings,
            Perm::CreateDraft,
            Perm::ViewReports,
            Perm::Delete,
        ] {
            assert!(
                role_allows(Role::Admin, perm),
                "Admin must have perm {perm:?}"
            );
        }
    }

    #[test]
    fn viewer_only_view_reports() {
        assert!(role_allows(Role::Viewer, Perm::ViewReports));
        for perm in [
            Perm::PostGl,
            Perm::ClosePeriod,
            Perm::SubmitAnaf,
            Perm::ManageUsers,
            Perm::WriteSettings,
            Perm::CreateDraft,
            Perm::Delete,
        ] {
            assert!(
                !role_allows(Role::Viewer, perm),
                "Viewer must NOT have perm {perm:?}"
            );
        }
    }

    #[test]
    fn operator_no_post_gl_or_submit_anaf() {
        assert!(!role_allows(Role::Operator, Perm::PostGl));
        assert!(!role_allows(Role::Operator, Perm::SubmitAnaf));
        assert!(!role_allows(Role::Operator, Perm::ManageUsers));
        assert!(!role_allows(Role::Operator, Perm::WriteSettings));
        assert!(role_allows(Role::Operator, Perm::CreateDraft));
        assert!(role_allows(Role::Operator, Perm::ViewReports));
    }

    #[test]
    fn contabil_no_manage_users() {
        assert!(!role_allows(Role::Contabil, Perm::ManageUsers));
        assert!(role_allows(Role::Contabil, Perm::PostGl));
        assert!(role_allows(Role::Contabil, Perm::ClosePeriod));
        assert!(role_allows(Role::Contabil, Perm::SubmitAnaf));
        assert!(role_allows(Role::Contabil, Perm::WriteSettings));
        assert!(role_allows(Role::Contabil, Perm::CreateDraft));
        assert!(role_allows(Role::Contabil, Perm::ViewReports));
        assert!(role_allows(Role::Contabil, Perm::Delete));
    }

    // ── authorize() — 4 decision paths ──────────────────────────────────

    #[test]
    fn authorize_public_cmd_when_unauthenticated() {
        // Public commands always pass, even unauthenticated.
        assert_eq!(
            authorize("auth_login", false, None),
            Decision::Allow,
            "Public command must be allowed unauthenticated"
        );
        assert_eq!(authorize("get_license", false, None), Decision::Allow,);
    }

    #[test]
    fn authorize_non_public_cmd_when_unauthenticated() {
        assert_eq!(
            authorize("list_invoices", false, None),
            Decision::Unauthorized,
            "Non-public command must be Unauthorized when not authenticated"
        );
    }

    #[test]
    fn authorize_sensitive_cmd_forbidden_for_wrong_role() {
        // Viewer trying to post GL → Forbidden.
        assert_eq!(
            authorize("generate_gl_entries", true, Some(Role::Viewer)),
            Decision::Forbidden,
        );
        // Operator trying to submit ANAF → Forbidden.
        assert_eq!(
            authorize("anaf_submit_invoice", true, Some(Role::Operator)),
            Decision::Forbidden,
        );
        // Contabil trying to manage users → Forbidden.
        assert_eq!(
            authorize("create_user", true, Some(Role::Contabil)),
            Decision::Forbidden,
        );
    }

    #[test]
    fn authorize_normal_cmd_allowed_for_any_authenticated_user() {
        // list_invoices has no required_perm → any authenticated user allowed.
        assert_eq!(
            authorize("list_invoices", true, Some(Role::Viewer)),
            Decision::Allow,
        );
        assert_eq!(
            authorize("list_invoices", true, Some(Role::Operator)),
            Decision::Allow,
        );
    }

    #[test]
    fn authorize_sensitive_cmd_allowed_for_sufficient_role() {
        assert_eq!(
            authorize("generate_gl_entries", true, Some(Role::Admin)),
            Decision::Allow,
        );
        assert_eq!(
            authorize("generate_gl_entries", true, Some(Role::Contabil)),
            Decision::Allow,
        );
        assert_eq!(
            authorize("create_user", true, Some(Role::Admin)),
            Decision::Allow,
        );
    }

    // ── Role encoding/decoding ───────────────────────────────────────────

    #[test]
    fn role_u8_roundtrip() {
        for role in [Role::Admin, Role::Contabil, Role::Operator, Role::Viewer] {
            assert_eq!(
                Role::from_u8(role.to_u8()),
                Some(role),
                "Role u8 round-trip failed for {role:?}"
            );
        }
    }

    #[test]
    fn role_str_roundtrip() {
        for role in [Role::Admin, Role::Contabil, Role::Operator, Role::Viewer] {
            assert_eq!(
                Role::from_db_str(role.as_str()),
                Some(role),
                "Role str round-trip failed for {role:?}"
            );
        }
    }

    // ── FIX 1: Viewer is truly read-only (deny-write-by-default) ────────

    /// Viewer must be Forbidden on create_invoice_draft; other roles may call it.
    #[test]
    fn viewer_forbidden_on_create_invoice_draft() {
        assert_eq!(
            authorize("create_invoice_draft", true, Some(Role::Viewer)),
            Decision::Forbidden,
            "Viewer must not create invoice drafts"
        );
        assert_eq!(
            authorize("create_invoice_draft", true, Some(Role::Operator)),
            Decision::Allow,
            "Operator must be allowed to create invoice drafts"
        );
        assert_eq!(
            authorize("create_invoice_draft", true, Some(Role::Contabil)),
            Decision::Allow,
        );
        assert_eq!(
            authorize("create_invoice_draft", true, Some(Role::Admin)),
            Decision::Allow,
        );
    }

    /// run_payroll posts GL — Operator (CreateDraft only) must be Forbidden; Contabil OK.
    #[test]
    fn run_payroll_requires_post_gl() {
        assert_eq!(
            authorize("run_payroll", true, Some(Role::Operator)),
            Decision::Forbidden,
            "Operator must not run payroll (PostGl required)"
        );
        assert_eq!(
            authorize("run_payroll", true, Some(Role::Viewer)),
            Decision::Forbidden,
            "Viewer must not run payroll"
        );
        assert_eq!(
            authorize("run_payroll", true, Some(Role::Contabil)),
            Decision::Allow,
            "Contabil must be allowed to run payroll"
        );
        assert_eq!(
            authorize("run_payroll", true, Some(Role::Admin)),
            Decision::Allow,
        );
    }

    /// Pure read commands must remain Allow for Viewer.
    #[test]
    fn viewer_can_read() {
        for cmd in [
            "list_companies",
            "get_invoice",
            "preview_bilant_xml",
            "list_invoices",
            "list_contacts",
            "get_product",
            "list_employees",
            "compute_payroll",
            "compute_d300",
            "check_license_validity",
        ] {
            assert_eq!(
                authorize(cmd, true, Some(Role::Viewer)),
                Decision::Allow,
                "Viewer must be able to read command '{cmd}'"
            );
        }
    }

    /// Deny-write-by-default: a broad set of write commands must all be Forbidden for Viewer.
    #[test]
    fn deny_write_by_default_for_viewer() {
        let write_cmds = [
            "create_invoice_draft",
            "create_contact",
            "create_product",
            "create_employee",
            "create_manual_journal",
            "update_invoice_draft",
            "update_company",
            "update_contact",
            "update_product",
            "update_employee",
            "set_invoice_status",
            "set_vat_rate_active",
            "set_stock_valuation",
            "storno_invoice",
            "duplicate_invoice",
            "add_payment",
            "add_received_payment",
            "import_backup",
            "import_invoices_csv",
            "import_invoice_xml",
            "import_wave_c_commit",
            "match_bank_txn",
            "unmatch_bank_txn",
            "toggle_recurring_active",
            "mark_notification_read",
            "mark_all_notifications_read",
            "settle_fiscal_receipt_pos",
            "commit_batch",
            "seed_standard_accounts",
            "export_backup",
        ];
        for cmd in write_cmds {
            assert_eq!(
                authorize(cmd, true, Some(Role::Viewer)),
                Decision::Forbidden,
                "Viewer must be Forbidden on write command '{cmd}'"
            );
        }
    }

    /// Contabil and Admin must still be allowed on all write commands.
    #[test]
    fn contabil_and_admin_unaffected_by_deny_write_default() {
        let write_cmds = [
            "create_invoice_draft",
            "update_invoice_draft",
            "set_invoice_status",
            "storno_invoice",
            "add_payment",
            "import_backup",
            "match_bank_txn",
            "export_backup",
            "toggle_recurring_active",
        ];
        for cmd in write_cmds {
            assert_eq!(
                authorize(cmd, true, Some(Role::Contabil)),
                Decision::Allow,
                "Contabil must be allowed on write command '{cmd}'"
            );
            assert_eq!(
                authorize(cmd, true, Some(Role::Admin)),
                Decision::Allow,
                "Admin must be allowed on write command '{cmd}'"
            );
        }
    }

    // ── FIX A: True deny-by-default — audit-found leaky commands ────────

    /// The 9 audit-found commands + a made-up future command must all be
    /// Forbidden for Viewer (deny-by-default).
    #[test]
    fn deny_by_default_audit_found_commands_viewer_forbidden() {
        let cmds = [
            // Three that previously slipped through (no matching write prefix):
            "anaf_sync_spv",
            "manual_sync",
            "reparse_received_vat",
            // Six explicitly added in this fix:
            "smartbill_push_invoice",
            "change_archive_location",
            "anaf_logout",
            "anaf_refresh_certificate",
            "dev_seed",
            // Unknown future mutation — must also be denied by default:
            "frobnicate_widgets",
        ];
        for cmd in cmds {
            assert_eq!(
                authorize(cmd, true, Some(Role::Viewer)),
                Decision::Forbidden,
                "Viewer must be Forbidden on command '{cmd}'"
            );
        }
    }

    /// Read commands from reports, ledgers, and dashboard must remain Allow for Viewer.
    #[test]
    fn reads_still_allowed_for_viewer() {
        let read_cmds = [
            // Non-prefixed reads in the explicit allowlist:
            "bilant",
            "profit_and_loss",
            "trial_balance",
            "aging_report",
            "general_ledger",
            "partner_ledger",
            "stock_on_hand",
            // Prefixed reads:
            "get_invoice",
            "list_companies",
            "preview_bilant_xml",
            "compute_d300",
            "check_license_validity",
            "fetch_bnr_rate",
            "search_contacts",
            "validate_invoice",
            "count_invoices",
            "load_settings",
            "export_aging_csv",
            "export_sales_journal",
        ];
        for cmd in read_cmds {
            assert_eq!(
                authorize(cmd, true, Some(Role::Viewer)),
                Decision::Allow,
                "Viewer must be allowed to call read command '{cmd}'"
            );
        }
    }

    /// smartbill_push_invoice requires SubmitAnaf:
    ///   Operator → Forbidden; Contabil → Allow.
    #[test]
    fn smartbill_push_invoice_requires_submit_anaf() {
        assert_eq!(
            required_perm("smartbill_push_invoice"),
            Some(Perm::SubmitAnaf),
            "smartbill_push_invoice must require SubmitAnaf"
        );
        assert_eq!(
            authorize("smartbill_push_invoice", true, Some(Role::Operator)),
            Decision::Forbidden,
            "Operator must be Forbidden on smartbill_push_invoice"
        );
        assert_eq!(
            authorize("smartbill_push_invoice", true, Some(Role::Contabil)),
            Decision::Allow,
            "Contabil must be allowed to call smartbill_push_invoice"
        );
    }

    /// anaf_logout requires WriteSettings:
    ///   Operator → Forbidden; Contabil → Allow.
    #[test]
    fn anaf_logout_requires_write_settings() {
        assert_eq!(
            required_perm("anaf_logout"),
            Some(Perm::WriteSettings),
            "anaf_logout must require WriteSettings"
        );
        assert_eq!(
            authorize("anaf_logout", true, Some(Role::Operator)),
            Decision::Forbidden,
            "Operator must be Forbidden on anaf_logout"
        );
        assert_eq!(
            authorize("anaf_logout", true, Some(Role::Contabil)),
            Decision::Allow,
            "Contabil must be allowed to call anaf_logout"
        );
    }

    /// anaf_refresh_certificate requires WriteSettings.
    #[test]
    fn anaf_refresh_certificate_requires_write_settings() {
        assert_eq!(
            required_perm("anaf_refresh_certificate"),
            Some(Perm::WriteSettings),
            "anaf_refresh_certificate must require WriteSettings"
        );
        assert_eq!(
            authorize("anaf_refresh_certificate", true, Some(Role::Operator)),
            Decision::Forbidden,
        );
        assert_eq!(
            authorize("anaf_refresh_certificate", true, Some(Role::Contabil)),
            Decision::Allow,
        );
    }

    /// change_archive_location requires WriteSettings.
    #[test]
    fn change_archive_location_requires_write_settings() {
        assert_eq!(
            required_perm("change_archive_location"),
            Some(Perm::WriteSettings),
            "change_archive_location must require WriteSettings"
        );
        assert_eq!(
            authorize("change_archive_location", true, Some(Role::Operator)),
            Decision::Forbidden,
        );
        assert_eq!(
            authorize("change_archive_location", true, Some(Role::Contabil)),
            Decision::Allow,
        );
    }

    /// dev_seed requires PostGl:
    ///   Operator → Forbidden; Contabil → Allow.
    #[test]
    fn dev_seed_requires_post_gl() {
        assert_eq!(
            required_perm("dev_seed"),
            Some(Perm::PostGl),
            "dev_seed must require PostGl"
        );
        assert_eq!(
            authorize("dev_seed", true, Some(Role::Operator)),
            Decision::Forbidden,
            "Operator must be Forbidden on dev_seed"
        );
        assert_eq!(
            authorize("dev_seed", true, Some(Role::Contabil)),
            Decision::Allow,
            "Contabil must be allowed to call dev_seed"
        );
    }

    /// preview_saft_official_xml is read-prefixed but internally re-posts GL → must require PostGl,
    /// NOT be freed by the preview_ read-prefix (the 4th-pass audit leak).
    #[test]
    fn preview_saft_official_xml_requires_post_gl() {
        assert_eq!(
            required_perm("preview_saft_official_xml"),
            Some(Perm::PostGl),
            "preview_saft_official_xml calls generate_gl_entries → must require PostGl, not be a free read"
        );
        assert_eq!(
            authorize("preview_saft_official_xml", true, Some(Role::Viewer)),
            Decision::Forbidden,
            "Viewer must NOT be able to re-post GL via the SAF-T preview"
        );
        assert_eq!(
            authorize("preview_saft_official_xml", true, Some(Role::Operator)),
            Decision::Forbidden,
            "Operator (no PostGl) must be Forbidden"
        );
        assert_eq!(
            authorize("preview_saft_official_xml", true, Some(Role::Contabil)),
            Decision::Allow,
            "Contabil may preview/regenerate the SAF-T"
        );
    }

    /// Unknown future command returns CreateDraft from required_perm.
    #[test]
    fn unknown_command_requires_create_draft() {
        assert_eq!(
            required_perm("frobnicate_widgets"),
            Some(Perm::CreateDraft),
            "Unknown commands must default to CreateDraft (deny-by-default)"
        );
        // Viewer → Forbidden; Operator → Allow (has CreateDraft).
        assert_eq!(
            authorize("frobnicate_widgets", true, Some(Role::Viewer)),
            Decision::Forbidden,
        );
        assert_eq!(
            authorize("frobnicate_widgets", true, Some(Role::Operator)),
            Decision::Allow,
        );
    }
}
