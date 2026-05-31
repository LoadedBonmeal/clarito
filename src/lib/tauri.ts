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
  DataExportResult,
  DiagnosticReport,
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

/**
 * Returnează true dacă rulăm în interiorul ferestrei Tauri (nu în browser obișnuit).
 * `window.__TAURI_INTERNALS__` este injectat de runtime-ul Tauri.
 */
export function isTauriContext(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Folosește direct când ai nevoie de o comandă neacoperită încă. */
export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriContext()) {
    return Promise.reject({
      message:
        "Aplicația trebuie deschisă ca aplicație nativă (nu din browser). " +
        "Porniți RoFactura din Finder, Dock sau meniu Start.",
    });
  }
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
  /** R14 Wave A: companyId is required — cross-company update returns NotFound. */
  update: (id: string, companyId: string, input: UpdateContactInput) =>
    invoke<Contact>("update_contact", { id, companyId, input }),
  /** R14 Wave A: companyId is required — cross-company deletion returns NotFound. */
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_contact", { id, companyId }),
  search: (query: string, companyId: string) =>
    invoke<Contact[]>("search_contacts", { query, companyId }),
};

// ─── Invoices ─────────────────────────────────────────────────────────────

export const invoices = {
  list: (filter?: InvoiceFilter) =>
    invoke<Paginated<Invoice>>("list_invoices", { filter }),
  /** R13 Wave G: companyId is required — cross-company access returns NotFound. */
  get: (id: string, companyId: string) =>
    invoke<InvoiceWithLines>("get_invoice", { id, companyId }),
  createDraft: (input: CreateInvoiceInput) =>
    invoke<Invoice>("create_invoice_draft", { input }),
  /** R14 Wave A: companyId is required — cross-company update returns NotFound. */
  updateDraft: (id: string, companyId: string, input: CreateInvoiceInput) =>
    invoke<Invoice>("update_invoice_draft", { id, companyId, input }),
  validateDraft: (id: string) =>
    invoke<{ isValid: boolean; errors: string[]; warnings: string[] }>(
      "validate_invoice_draft",
      { id }
    ),
  /** R13 Wave G: companyId is required — cross-company deletion returns NotFound. */
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_invoice", { id, companyId }),
  /** R13 Wave G: companyId is required — cross-company mutation returns NotFound. */
  setStatus: (id: string, companyId: string, status: InvoiceStatus, message?: string) =>
    invoke<void>("set_invoice_status", { id, companyId, status, message }),
  /** R14 Wave A: companyId is required — cross-company storno returns NotFound. */
  storno: (invoiceId: string, companyId: string, reason: string) =>
    invoke<Invoice>("storno_invoice", { invoiceId, companyId, reason }),
  /** R14 Wave A: companyId is required — cross-company duplication returns NotFound. */
  duplicate: (invoiceId: string, companyId: string) =>
    invoke<string>("duplicate_invoice", { invoiceId, companyId }),
};

// ─── Received ─────────────────────────────────────────────────────────────

export const received = {
  list: (filter?: ReceivedFilter) =>
    invoke<Paginated<ReceivedInvoice>>("list_received_invoices", { filter }),
  get: (id: string, companyId: string) =>
    invoke<ReceivedInvoice>("get_received_invoice", { id, companyId }),
  updateStatus: (id: string, companyId: string, status: ReceivedStatus) =>
    invoke<void>("update_received_status", { id, companyId, status }),
  reparseVat: (companyId?: string) =>
    invoke<number>("reparse_received_vat", { companyId: companyId ?? null }),
};

// ─── Notifications ────────────────────────────────────────────────────────

export const notifications = {
  list: (onlyUnread = false) =>
    invoke<Notification[]>("list_notifications", { onlyUnread }),
  unreadCount: () => invoke<number>("unread_notification_count"),
  markRead: (id: string) => invoke<void>("mark_notification_read", { id }),
  markAllRead: () => invoke<void>("mark_all_notifications_read"),
  deleteOne: (id: string) => invoke<void>("delete_notification", { id }),
  deleteAllRead: () => invoke<number>("delete_all_read_notifications"),
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
  /** R14 Wave E: companyId is required — cross-company XML generation returns NotFound. */
  generateXml: (invoiceId: string, companyId: string) =>
    invoke<string>("generate_invoice_xml", { invoiceId, companyId }),
  /** R14 Wave E: companyId is required — cross-company PDF generation returns NotFound. */
  generatePdf: (invoiceId: string, companyId: string) =>
    invoke<string>("generate_invoice_pdf", { invoiceId, companyId }),
  /** R14 Wave E: companyId is required — cross-company XML validation returns NotFound. */
  validateXml: (invoiceId: string, companyId: string) =>
    invoke<ValidationResult>("validate_invoice_xml", { invoiceId, companyId }),
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
  invoiceXml: (xmlContent: string, companyId: string) =>
    invoke<{
      imported: number;
      invoiceNumber?: string;
      supplierName?: string;
      supplierCui?: string;
      issueDate?: string;
      totalAmount?: string;
      errors: string[];
    }>("import_invoice_xml", { xmlContent, companyId }),
  /** Preferred: citește fișierul în Rust, ocolind scope-ul FS plugin. */
  invoiceXmlFromFile: (filePath: string, companyId: string) =>
    invoke<{
      imported: number;
      invoiceNumber?: string;
      supplierName?: string;
      supplierCui?: string;
      issueDate?: string;
      totalAmount?: string;
      errors: string[];
    }>("import_invoice_xml_from_file", { filePath, companyId }),
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
  exportSagaCsv: (companyId: string, dateFrom: string, dateTo: string, outputPath?: string) =>
    invoke<string>("export_saga_csv", { companyId, dateFrom, dateTo, outputPath: outputPath ?? null }),
  exportWinmentorCsv: (companyId: string, dateFrom: string, dateTo: string, outputPath?: string) =>
    invoke<string>("export_winmentor_csv", { companyId, dateFrom, dateTo, outputPath: outputPath ?? null }),
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

// ─── Payments ─────────────────────────────────────────────────────────────

export interface Payment {
  id: string;
  invoiceId: string;
  companyId: string;
  amount: string;
  currency: string;
  paidAt: string;
  method: string;
  reference?: string;
  notes?: string;
  createdAt: number;
}

export interface PaymentSummary {
  invoiceId: string;
  totalAmount: string;
  paidAmount: string;
  paymentStatus: "UNPAID" | "PARTIAL" | "PAID";
  payments: Payment[];
}

export interface AddPaymentArgs {
  invoiceId: string;
  companyId: string;
  amount: string;
  currency?: string;
  paidAt: string;
  method?: string;
  reference?: string;
  notes?: string;
}

export const payments = {
  add: (args: AddPaymentArgs) => invoke<Payment>("add_payment", { args }),
  list: (invoiceId: string, companyId: string) =>
    invoke<Payment[]>("list_payments", { invoiceId, companyId }),
  delete: (paymentId: string, companyId: string) =>
    invoke<void>("delete_payment", { paymentId, companyId }),
  summary: (invoiceId: string, companyId: string) =>
    invoke<PaymentSummary>("get_payment_summary", { invoiceId, companyId }),
  listSummaries: (companyId: string) =>
    invoke<PaymentSummary[]>("list_payment_summaries", { companyId }),
};

// ─── Recurring invoices ────────────────────────────────────────────────────

export interface RecurringInvoice {
  id: string;
  companyId: string;
  templateName: string;
  clientId: string;
  frequency: "monthly" | "quarterly" | "annual";
  nextIssueDate: string;
  dayOfMonth: number;
  autoSubmitAnaf: boolean;
  active: boolean;
  series: string;
  linesJson: string;
  notes?: string;
  createdAt: number;
  updatedAt: number;
}

export interface CreateRecurringArgs {
  companyId: string;
  templateName: string;
  clientId: string;
  frequency: string;
  nextIssueDate: string;
  dayOfMonth: number;
  autoSubmitAnaf: boolean;
  series: string;
  linesJson: string;
  notes?: string;
}

export interface UpdateRecurringArgs {
  id: string;
  companyId: string;
  templateName: string;
  frequency: string;
  nextIssueDate: string;
  dayOfMonth: number;
  autoSubmitAnaf: boolean;
  active: boolean;
  series: string;
  linesJson: string;
  notes?: string | null;
}

export const recurring = {
  create: (args: CreateRecurringArgs) =>
    invoke<RecurringInvoice>("create_recurring_invoice", { args }),
  list: (companyId: string) =>
    invoke<RecurringInvoice[]>("list_recurring_invoices", { companyId }),
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_recurring_invoice", { id, companyId }),
  update: (args: UpdateRecurringArgs) =>
    invoke<void>("update_recurring_invoice", { args }),
  toggleActive: (id: string, companyId: string, active: boolean) =>
    invoke<void>("toggle_recurring_active", { id, companyId, active }),
};

// ─── Feedback ─────────────────────────────────────────────────────────────

export const feedback = {
  gather: () => invoke<DiagnosticReport>("gather_diagnostic"),
  mailto: (report: DiagnosticReport, userMessage?: string) =>
    invoke<string>("build_feedback_mailto", { report, userMessage }),
};

// ─── SAF-T ────────────────────────────────────────────────────────────────

export const saft = {
  exportD406: (companyId: string, year: number, month?: number) =>
    invoke<string>("export_saft_d406", {
      params: { companyId, year, month: month ?? null },
    }),
};

// ─── Declarations (D300) ──────────────────────────────────────────────────

export const declarations = {
  /** Calculează decontul D300 — TVA colectat (vânzări) pentru o perioadă. */
  compute: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").D300Report>("compute_d300", { companyId, periodFrom, periodTo }),
  /** Generează XML D300 și îl salvează la destPath. Returnează calea. */
  export: (companyId: string, periodFrom: string, periodTo: string, destPath: string) =>
    invoke<string>("export_d300", { companyId, periodFrom, periodTo, destPath }),
};

// ─── D394 — Declarație informativă livrări/achiziții ─────────────────────

export const d394 = {
  /** Calculează declarația D394 — livrări (vânzări) grupate pe partener. */
  compute: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").D394Report>("compute_d394", { companyId, periodFrom, periodTo }),
  /** Generează XML D394 și îl salvează la destPath. Returnează calea. */
  export: (companyId: string, periodFrom: string, periodTo: string, destPath: string) =>
    invoke<string>("export_d394", { companyId, periodFrom, periodTo, destPath }),
};

// ─── Jurnale contabile ────────────────────────────────────────────────────

export const journals = {
  /** Exportă jurnalul de vânzări CSV pentru o perioadă. Returnează calea fișierului. */
  exportSales: (companyId: string, dateFrom: string, dateTo: string, destPath: string) =>
    invoke<string>("export_sales_journal", { companyId, dateFrom, dateTo, destPath }),
  /** Exportă jurnalul de cumpărări CSV pentru o perioadă. Returnează calea fișierului. */
  exportPurchases: (companyId: string, dateFrom: string, dateTo: string, destPath: string) =>
    invoke<string>("export_purchase_journal", { companyId, dateFrom, dateTo, destPath }),
};

// ─── GDPR / data portability ──────────────────────────────────────────────

export const gdpr = {
  /** Export all user data (DB + archive) as a ZIP to the chosen path. */
  exportAll: (destPath: string) =>
    invoke<DataExportResult>("export_all_my_data", { destPath }),
  /** Irreversibly wipe all local data. Frontend MUST double-confirm. */
  wipeAll: () => invoke<void>("wipe_all_data"),
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
  payments,
  recurring,
  saft,
  feedback,
  gdpr,
  declarations,
  d394,
  journals,
};
