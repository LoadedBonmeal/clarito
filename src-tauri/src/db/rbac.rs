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
/// Returns `None` for commands that any authenticated user may call.
/// Returns `Some(Perm)` for sensitive commands.
///
/// **Curated list** (derived from reading commands/):
///
/// PostGl:
///   generate_gl_entries, reconcile_gl, post_income_tax, post_annual_close,
///   run_depreciation, dispose_asset, compute_fx_revaluation,
///   post_inventory_diffs, produce, finalize_inventory_session
///
/// ClosePeriod:
///   close_vat_period, close_period
///
/// SubmitAnaf:
///   anaf_submit_invoice, export_d300_official, export_d390, export_d394_official,
///   export_saft_official, export_d205_official, export_d207_official,
///   export_bilant_xml, etransport_submit, export_d112_xml
///
/// ManageUsers:
///   list_users, create_user, update_user, reset_password
///
/// WriteSettings:
///   set_setting, set_smartbill_credentials, clear_smartbill_credentials,
///   anaf_set_oauth_client_secret, set_autostart
///
/// Delete:
///   delete_company, delete_contact, delete_product, delete_invoice,
///   delete_payment, delete_received_payment, delete_notification,
///   delete_all_read_notifications, delete_recurring_invoice,
///   delete_employee, delete_secondary_office, delete_medical_leave,
///   delete_dividend, delete_vat_rate, delete_account, delete_receipt,
///   delete_bank_account, delete_stock_movement, delete_manual_journal,
///   delete_inventory_session, delete_fixed_asset, delete_gestiune,
///   delete_fiscal_receipt, delete_declaration_filing, wipe_all_data,
///   delete_bom, delete_productie (delete_stock_movement handled above)
///
/// ViewReports: reports, GL ledger, journals, aging.
pub fn required_perm(cmd: &str) -> Option<Perm> {
    match cmd {
        // ── PostGl ────────────────────────────────────────────────────────
        "generate_gl_entries"
        | "reconcile_gl"
        | "post_income_tax"
        | "post_annual_close"
        | "run_depreciation"
        | "dispose_asset"
        | "compute_fx_revaluation"
        | "post_inventory_diffs"
        | "produce"
        | "finalize_inventory_session"
        | "record_stock_receipt"
        | "record_stock_issue"
        | "finalize_nir"
        | "transfer_stock" => Some(Perm::PostGl),

        // ── ClosePeriod ───────────────────────────────────────────────────
        "close_vat_period" | "close_period" => Some(Perm::ClosePeriod),

        // ── SubmitAnaf ────────────────────────────────────────────────────
        "anaf_submit_invoice"
        | "export_d300_official"
        | "export_d390"
        | "export_d394_official"
        | "export_saft_official"
        | "export_d205_official"
        | "export_d207_official"
        | "export_bilant_xml"
        | "etransport_submit"
        | "export_d112_xml" => Some(Perm::SubmitAnaf),

        // ── ManageUsers ───────────────────────────────────────────────────
        "list_users" | "create_user" | "update_user" | "reset_password" => Some(Perm::ManageUsers),

        // ── WriteSettings ─────────────────────────────────────────────────
        "set_setting"
        | "set_smartbill_credentials"
        | "clear_smartbill_credentials"
        | "anaf_set_oauth_client_secret"
        // Connecting/disconnecting the company's ANAF identity is a sensitive settings change
        // (a revoke disconnects e-Factura/SPV) — not for viewer/operator.
        | "anaf_authorize"
        | "anaf_revoke_certificate"
        | "set_autostart" => Some(Perm::WriteSettings),

        // ── Delete ────────────────────────────────────────────────────────
        "delete_company"
        | "delete_contact"
        | "delete_product"
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
        | "delete_bom" => Some(Perm::Delete),

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

        // Any other authenticated command — no specific perm required.
        _ => None,
    }
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
}
