/**
 * Wrapper typed peste `@tauri-apps/api/core.invoke`.
 *
 * Folosește `api.companies.list()` în loc de `invoke('list_companies')` —
 * tipuri inferate corect și autocompletare în IDE.
 *
 * Convenție: numele Rust al comenzii e snake_case; aici expunem grupuri
 * de funcții pe entitate.
 */

import { invoke as rawInvoke } from "@tauri-apps/api/core";

import type {
  AnafCompanyData,
  AppInfo,
  Certificate,
  Company,
  Contact,
  ContactFilter,
  CreateCompanyInput,
  CreateContactInput,
  CreateInvoiceInput,
  Invoice,
  InvoiceFilter,
  InvoiceStatus,
  InvoiceWithLines,
  License,
  Notification,
  Paginated,
  ReceivedFilter,
  ReceivedInvoice,
  ReceivedStatus,
  SyncResult,
  UpdateCompanyInput,
  UpdateContactInput,
  ValidationResult,
} from "@/types";

// ─── Helpers ──────────────────────────────────────────────────────────────

/** Folosește direct când ai nevoie de o comandă neacoperită încă. */
export function invoke<T>(cmd: string, args?: Record<string, unknown>) {
  return rawInvoke<T>(cmd, args);
}

// ─── Companies ────────────────────────────────────────────────────────────

export const companies = {
  list: () => invoke<Company[]>("list_companies"),
  get: (id: string) => invoke<Company>("get_company", { id }),
  create: (input: CreateCompanyInput) =>
    invoke<Company>("create_company", { input }),
  update: (id: string, input: UpdateCompanyInput) =>
    invoke<Company>("update_company", { id, input }),
  delete: (id: string) => invoke<void>("delete_company", { id }),
  getNextInvoiceNumber: (companyId: string) =>
    invoke<number>("get_next_invoice_number", { companyId }),
  fetchAnafData: (cui: string) =>
    invoke<AnafCompanyData>("fetch_anaf_company_data", { cui }),
};

// ─── Contacts ─────────────────────────────────────────────────────────────

export const contacts = {
  list: (filter?: ContactFilter) =>
    invoke<Contact[]>("list_contacts", { filter }),
  get: (id: string) => invoke<Contact>("get_contact", { id }),
  create: (input: CreateContactInput) =>
    invoke<Contact>("create_contact", { input }),
  update: (id: string, input: UpdateContactInput) =>
    invoke<Contact>("update_contact", { id, input }),
  delete: (id: string) => invoke<void>("delete_contact", { id }),
  search: (query: string) => invoke<Contact[]>("search_contacts", { query }),
};

// ─── Invoices ─────────────────────────────────────────────────────────────

export const invoices = {
  list: (filter?: InvoiceFilter) =>
    invoke<Paginated<Invoice>>("list_invoices", { filter }),
  get: (id: string) => invoke<InvoiceWithLines>("get_invoice", { id }),
  createDraft: (input: CreateInvoiceInput) =>
    invoke<Invoice>("create_invoice_draft", { input }),
  updateDraft: (id: string, input: CreateInvoiceInput) =>
    invoke<Invoice>("update_invoice_draft", { id, input }),
  validateDraft: (id: string) =>
    invoke<{ isValid: boolean; errors: string[]; warnings: string[] }>(
      "validate_invoice_draft",
      { id }
    ),
  delete: (id: string) => invoke<void>("delete_invoice", { id }),
  setStatus: (id: string, status: InvoiceStatus, message?: string) =>
    invoke<void>("set_invoice_status", { id, status, message }),
  storno: (invoiceId: string, reason: string) =>
    invoke<Invoice>("storno_invoice", { invoiceId, reason }),
};

// ─── Received ─────────────────────────────────────────────────────────────

export const received = {
  list: (filter?: ReceivedFilter) =>
    invoke<Paginated<ReceivedInvoice>>("list_received_invoices", { filter }),
  get: (id: string) =>
    invoke<ReceivedInvoice>("get_received_invoice", { id }),
  updateStatus: (id: string, status: ReceivedStatus) =>
    invoke<void>("update_received_status", { id, status }),
};

// ─── Notifications ────────────────────────────────────────────────────────

export const notifications = {
  list: (onlyUnread = false) =>
    invoke<Notification[]>("list_notifications", { onlyUnread }),
  unreadCount: () => invoke<number>("unread_notification_count"),
  markRead: (id: string) => invoke<void>("mark_notification_read", { id }),
  markAllRead: () => invoke<void>("mark_all_notifications_read"),
};

// ─── Settings ─────────────────────────────────────────────────────────────

export const settings = {
  get: (key: string) => invoke<string | null>("get_setting", { key }),
  set: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
  getAll: () => invoke<Record<string, string>>("get_all_settings"),
};

// ─── License ──────────────────────────────────────────────────────────────

export const license = {
  get: () => invoke<License | null>("get_license"),
  startTrial: (email: string) =>
    invoke<License>("start_trial", { email }),
  activate: (key: string, email: string) =>
    invoke<License>("activate_license", { key, email }),
  checkLicenseValidity: () => invoke<boolean>("check_license_validity"),
};

// ─── System ───────────────────────────────────────────────────────────────

export const system = {
  appInfo: () => invoke<AppInfo>("get_app_info"),
  dbPath: () => invoke<string>("get_db_path"),
  manualSync: () => invoke<SyncResult>("manual_sync"),
  devSeed: () => invoke<void>("dev_seed"),
  openArchiveFolder: () => invoke<void>("open_archive_folder"),
  exportBackup: () => invoke<string>("export_backup"),
  setAutostart: (enabled: boolean) =>
    invoke<void>("set_autostart", { enabled }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  getActivityLog: () =>
    invoke<
      Array<{ id: string; entityId: string; metadata: string; createdAt: number }>
    >("get_activity_log"),
  exportActivityLogCsv: () => invoke<string>("export_activity_log_csv"),
};

// ─── UBL ──────────────────────────────────────────────────────────────────

export const ubl = {
  generateXml: (invoiceId: string) =>
    invoke<string>("generate_invoice_xml", { invoiceId }),
  generatePdf: (invoiceId: string) =>
    invoke<string>("generate_invoice_pdf", { invoiceId }),
  validateXml: (invoiceId: string) =>
    invoke<ValidationResult>("validate_invoice_xml", { invoiceId }),
};

// ─── ANAF ─────────────────────────────────────────────────────────────────

export const anaf = {
  authorize: (companyId: string) =>
    invoke<boolean>("anaf_authorize", { companyId }),
  isAuthenticated: (companyId: string) =>
    invoke<boolean>("anaf_is_authenticated", { companyId }),
  logout: (companyId: string) =>
    invoke<void>("anaf_logout", { companyId }),
  submitInvoice: (companyId: string, invoiceId: string, testMode = false) =>
    invoke<string>("anaf_submit_invoice", { companyId, invoiceId, testMode }),
  checkStatus: (companyId: string, invoiceId: string, testMode = false) =>
    invoke<string>("anaf_check_invoice_status", { companyId, invoiceId, testMode }),
  syncSpv: (companyId: string, testMode = false) =>
    invoke<number>("anaf_sync_spv", { companyId, testMode }),
};

// ─── Certificates ─────────────────────────────────────────────────────────

export const certificates = {
  list: (companyId: string) => invoke<Certificate[]>("anaf_get_certificates", { companyId }),
  refresh: (companyId: string) => invoke<boolean>("anaf_refresh_certificate", { companyId }),
  revoke: (companyId: string) => invoke<void>("anaf_revoke_certificate", { companyId }),
};

// ─── Archive ──────────────────────────────────────────────────────────────

export const archive = {
  exportZip: (companyId: string) =>
    invoke<string>("export_invoices_zip", { companyId }),
  verifyIntegrity: () =>
    invoke<{ totalChecked: number; missingFiles: string[]; ok: boolean }>(
      "verify_archive_integrity"
    ),
  getSize: () => invoke<number>("get_archive_size"),
  importBackup: (path: string) => invoke<void>("import_backup", { path }),
  changeArchiveLocation: (newPath: string) =>
    invoke<void>("change_archive_location", { newPath }),
};

// ─── Import ───────────────────────────────────────────────────────────────

export const importData = {
  invoicesCsv: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_invoices_csv", {
      content,
      companyId,
      dryRun: false,
    }),
  contactsCsv: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_contacts_csv", {
      content,
      companyId,
      dryRun: false,
    }),
  invoicesCsvTemplate: () => invoke<string>("get_invoices_csv_template"),
  contactsCsvTemplate: () => invoke<string>("get_contacts_csv_template"),
  invoicesCsvDryRun: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_invoices_csv", {
      content,
      companyId,
      dryRun: true,
    }),
  contactsCsvDryRun: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_contacts_csv", {
      content,
      companyId,
      dryRun: true,
    }),
};

// ─── Integrations ─────────────────────────────────────────────────────────

export interface SmartBillCredentials {
  user: string;
  token: string;
  configured: boolean;
}

export const integrations = {
  smartbillPush: (companyId: string, invoiceId: string) =>
    invoke<string>("smartbill_push_invoice", { companyId, invoiceId }),
  exportSagaCsv: (companyId: string, dateFrom: string, dateTo: string) =>
    invoke<string>("export_saga_csv", { companyId, dateFrom, dateTo }),
  exportWinmentorCsv: (companyId: string, dateFrom: string, dateTo: string) =>
    invoke<string>("export_winmentor_csv", { companyId, dateFrom, dateTo }),
  getSmartbillCredentials: (companyId: string) =>
    invoke<SmartBillCredentials>("get_smartbill_credentials", { companyId }),
  exportInvoicesXlsx: (filter: { companyId?: string; dateFrom?: string; dateTo?: string }, outputPath: string) =>
    invoke<void>("export_invoices_xlsx", { filter, outputPath }),
};

// ─── Reports ──────────────────────────────────────────────────────────────

export const reports = {
  generateVatReport: (dateFrom: string, dateTo: string, companyId?: string) =>
    invoke<import("@/types").VatReport>("generate_vat_report", {
      dateFrom,
      dateTo,
      companyId: companyId ?? null,
    }),
  exportReport: (
    reportType: string,
    params: import("@/types").ExportReportParams,
    format: "csv" | "json",
    outputPath: string
  ) =>
    invoke<string>("export_report", { reportType, params, format, outputPath }),
};

// ─── API umbrella ─────────────────────────────────────────────────────────

export const api = {
  companies,
  contacts,
  invoices,
  received,
  notifications,
  settings,
  license,
  system,
  ubl,
  anaf,
  archive,
  certificates,
  integrations,
  importData,
  reports,
};
