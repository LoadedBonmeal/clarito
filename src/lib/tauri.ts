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

import { demoInvoke, isDemoMode } from "@/lib/demo";

import type {
  Account,
  AccountInput,
  AnafCompanyData,
  TaxRegimeStatus,
  AppInfo,
  Certificate,
  Company,
  Contact,
  ContactFilter,
  CreateCompanyInput,
  CreateContactInput,
  CreateInvoiceInput,
  D300Submission,
  D394Submission,
  DataExportResult,
  DiagnosticReport,
  FormStaleness,
  GlPostResult,
  Invoice,
  InvoiceFilter,
  InvoiceStatus,
  InvoiceWithLines,
  License,
  ManualJournalView,
  ManualLineInput,
  Notification,
  Paginated,
  Product,
  ProductInput,
  Receipt,
  ReceiptInput,
  ReceivedFilter,
  ReceivedInvoice,
  ReceivedStatus,
  ReconcileReport,
  VatSettlementResult,
  TrialBalance,
  ProfitLoss,
  ClosePeriodResult,
  BilantReport,
  IncomeTaxResult,
  AnnualCloseResult,
  JournalRegister,
  LedgerAccount,
  SyncResult,
  UpdateAccountInput,
  UpdateCompanyInput,
  UpdateContactInput,
  UpdateProductInput,
  ValidationResult,
  VatRate,
  VatRateInput,
  UpdateVatRateInput,
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
  // Dev-only browser harness (?demo=1): serve design-handoff fixtures instead of
  // Tauri IPC, so the UI can be pixel-verified against the prototypes headlessly.
  if (isDemoMode()) {
    return demoInvoke<T>(cmd, args);
  }
  if (!isTauriContext()) {
    return Promise.reject({
      message:
        "Aplicația trebuie deschisă ca aplicație nativă (nu din browser). " +
        "Porniți Clarito din Finder, Dock sau meniu Start.",
    });
  }
  return rawInvoke<T>(cmd, args);
}

// ─── Companies ────────────────────────────────────────────────────────────

/** Result returned by the VIES REST check-vat-number endpoint. */
export interface ViesResult {
  /** True when the number is registered in VIES. */
  valid: boolean;
  /** Legal name — null when the member state masks the data ("---"). */
  name: string | null;
  /** Registered address — null when masked. */
  address: string | null;
  /** 2-letter country code (always uppercase). */
  countryCode: string;
  /** VAT number without the country prefix. */
  vatNumber: string;
}

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
  /** Validate an EU VAT number via the VIES REST API (backend — no CORS). */
  validateVies: (countryCode: string, vatNumber: string) =>
    invoke<ViesResult>("validate_vies", { countryCode, vatNumber }),
  /** Micro-ceiling status (turnover vs 100.000 EUR) for a company in `year`; `eurRon` = EUR→RON. */
  taxRegimeStatus: (companyId: string, year: number, eurRon: number) =>
    invoke<TaxRegimeStatus>("tax_regime_status", { companyId, year, eurRon }),
  /** Plafonul de scutire TVA (art. 310, 395.000 lei) — doar pentru neplătitori de TVA. */
  vatRegistrationStatus: (companyId: string, year: number) =>
    invoke<{ applicable: boolean; level: string; ytdTurnoverRon: string; plafonRon: string; pct: number }>(
      "vat_registration_status",
      { companyId, year },
    ),
};

// ─── Contacts ─────────────────────────────────────────────────────────────

export const contacts = {
  list: (filter?: ContactFilter) =>
    invoke<Contact[]>("list_contacts", { filter }),
  /** S1: companyId is required — cross-company fetch returns NotFound. */
  get: (id: string, companyId: string) =>
    invoke<Contact>("get_contact", { id, companyId }),
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
  /** G3: companyId is required — cross-company validation returns NotFound. */
  validateDraft: (id: string, companyId: string) =>
    invoke<{ isValid: boolean; errors: string[]; warnings: string[] }>(
      "validate_invoice_draft",
      { id, companyId }
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
  /** Export a selection of received invoices as CSV text. Returns the CSV string. */
  exportCsv: (companyId: string, ids: string[]) =>
    invoke<string>("export_received_csv", { companyId, ids }),
  /** Setează tipul achiziției intra-UE: "goods" (R5/R18) sau "services" (R7/R20). */
  setIntraEuKind: (receivedInvoiceId: string, companyId: string, kind: "goods" | "services") =>
    invoke<void>("set_received_intra_eu_kind", { receivedInvoiceId, companyId, kind }),
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
  exportBackup: (destPath?: string) => invoke<string>("export_backup", { destPath: destPath ?? null }),
  setAutostart: (enabled: boolean) =>
    invoke<void>("set_autostart", { enabled }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  getActivityLog: (companyId: string) =>
    invoke<
      Array<{ id: string; entityId: string; metadata: string; createdAt: number }>
    >("get_activity_log", { companyId }),
  exportActivityLogCsv: (companyId: string) =>
    invoke<string>("export_activity_log_csv", { companyId }),
  checkFormVersions: () => invoke<FormStaleness[]>("check_form_versions"),
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
  /** Previzualizare șablon: PDF demo cu identitatea reală a companiei; returnează calea temp. */
  previewInvoiceTemplate: (
    companyId: string,
    tpl: {
      preset: string;
      accentHex: string;
      headerNote: string;
      footerNote: string;
      showWords: boolean;
      showVatDetail: boolean;
    },
  ) =>
    invoke<string>("preview_invoice_template", { companyId, ...tpl }),
};

// ─── ANAF ─────────────────────────────────────────────────────────────────

export const anaf = {
  authorize: (companyId: string) =>
    invoke<boolean>("anaf_authorize", { companyId }),
  isAuthenticated: (companyId: string) =>
    invoke<boolean>("anaf_is_authenticated", { companyId }),
  logout: (companyId: string) =>
    invoke<void>("anaf_logout", { companyId }),
  /** Save (or clear, if empty) the OAuth client_secret in the OS keychain. */
  setOauthClientSecret: (secret: string) =>
    invoke<void>("anaf_set_oauth_client_secret", { secret }),
  /** True if an OAuth client_secret is stored (value never returned to JS). */
  hasOauthClientSecret: () =>
    invoke<boolean>("anaf_has_oauth_client_secret"),
  submitInvoice: (companyId: string, invoiceId: string, testMode = false) =>
    invoke<string>("anaf_submit_invoice", { companyId, invoiceId, testMode }),
  checkStatus: (companyId: string, invoiceId: string, testMode = false) =>
    invoke<string>("anaf_check_invoice_status", { companyId, invoiceId, testMode }),
  syncSpv: (companyId: string, testMode = false) =>
    invoke<number>("anaf_sync_spv", { companyId, testMode }),
  /** General SPV inbox (SPVWS2): recipise, notificări, somații, decizii. Read-only. */
  listSpvInbox: (companyId: string, days = 60, testMode = false) =>
    invoke<import("@/types").SpvInboxItem[]>("anaf_list_spv_inbox", { companyId, days, testMode }),
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
  verifyIntegrity: (companyId: string) =>
    invoke<{ checked: number; missing: string[]; ok: boolean; missingUnderRetention: number }>(
      "verify_archive_integrity",
      { companyId }
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

// ─── Aging report types ────────────────────────────────────────────────────

export type AgingDirection = "RECEIVABLE" | "PAYABLE";

export interface AgingRow {
  partnerCui: string;
  partnerName: string;
  totalOutstanding: string;
  current: string;
  d130: string;
  d3160: string;
  d6190: string;
  over90: string;
}

export interface AgingReport {
  asOf: string;
  direction: AgingDirection;
  rows: AgingRow[];
  totals: AgingRow;
}

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
  /** Balanță cu vechime sold (AR/AP aging report). */
  aging: (companyId: string, direction: AgingDirection, asOf: string) =>
    invoke<AgingReport>("aging_report", { companyId, direction, asOf }),
  /** Exportă raportul de aging ca CSV. Returnează calea salvată. */
  exportAgingCsv: (
    companyId: string,
    direction: AgingDirection,
    asOf: string,
    outputPath: string,
  ) =>
    invoke<string>("export_aging_csv", { companyId, direction, asOf, outputPath }),
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
  /** Payment-date BNR rate (foreign-currency invoices) → books FX gain/loss 665/765. */
  exchangeRate?: number;
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

// ─── Supplier payments (payments-out, buyer-side TVA la încasare) ───────────

export interface ReceivedPayment {
  id: string;
  receivedInvoiceId: string;
  companyId: string;
  amount: string;
  currency: string;
  paidAt: string;
  method: string;
  reference?: string;
  notes?: string;
  createdAt: number;
}

export interface ReceivedPaymentSummary {
  receivedInvoiceId: string;
  totalAmount: string;
  paidAmount: string;
  paymentStatus: "UNPAID" | "PARTIAL" | "PAID";
  payments: ReceivedPayment[];
}

export interface AddReceivedPaymentArgs {
  receivedInvoiceId: string;
  companyId: string;
  amount: string;
  currency?: string;
  paidAt: string;
  method?: string;
  reference?: string;
  notes?: string;
  /** Payment-date BNR rate (foreign-currency invoices) → books FX gain/loss 665/765. */
  exchangeRate?: number;
}

export const receivedPayments = {
  add: (args: AddReceivedPaymentArgs) =>
    invoke<ReceivedPayment>("add_received_payment", { args }),
  list: (receivedInvoiceId: string, companyId: string) =>
    invoke<ReceivedPayment[]>("list_received_payments", { receivedInvoiceId, companyId }),
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_received_payment", { id, companyId }),
  summary: (receivedInvoiceId: string, companyId: string) =>
    invoke<ReceivedPaymentSummary>("get_received_payment_summary", {
      receivedInvoiceId,
      companyId,
    }),
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
  /**
   * Exportă D406 oficial (complet, schema-conformant) la destPath.
   * Auto-postează GL înainte de generare (idempotent).
   * Rust command `export_saft_official` preia un struct `SaftOfficialParams`
   * ca argument `params` (nu flat args):
   *   params: { companyId, year, month?, destPath }
   */
  exportSaftOfficial: (
    companyId: string,
    year: number,
    month: number | null,
    destPath: string,
    skipDukOverride = false,
    // Trimestrial: pasați quarter (1-4) cu month=null → D406 periodic pe trimestru. month=null +
    // quarter=null → declarația anuală (profil A).
    quarter: number | null = null,
  ) =>
    invoke<OfficialExportResult>("export_saft_official", {
      params: { companyId, year, month, quarter, destPath },
      skipDukOverride,
    }),
  /**
   * Construiește XML-ul D406 oficial (SAF-T) FĂRĂ a-l scrie pe disc — pentru vizualizatorul/editorul
   * XML din aplicație (re-validare DUK separat cu `declarations.validateDeclarationXml("D406", …)`).
   */
  previewSaftOfficial: (params: {
    companyId: string;
    year: number;
    month: number | null;
    quarter: number | null;
    destPath: string;
  }) => invoke<string>("preview_saft_official_xml", { params }),
};

// ─── Dividende (impozit pe dividende, Legea 141/2025) ───────────────────────

export interface Dividend {
  id: string;
  companyId: string;
  distributionDate: string;
  paymentDate: string | null;
  grossAmount: string;
  taxRate: number;
  taxAmount: string;
  netAmount: string;
  interim2025: boolean;
  /** Numele beneficiarului (reutilizat ca D205 den1). */
  shareholder: string | null;
  /** CNP-ul beneficiarului (D205 cifR, N13 mod-11) — opțional, cerut la export D205. */
  beneficiaryCnp: string | null;
  /** Rezident fiscal RO (D205 Rezid; nerezident → D207). */
  beneficiaryResident: boolean;
  /** Tip beneficiar: "PF" (art. 97 → D100 cod 604, intră în D205) sau "PJ" (art. 43 → D100 cod 150). */
  beneficiaryType: string;
  /** D207 (nerezidenți): codul de țară (Stat_R) al beneficiarului nerezident — cerut la export D207. */
  beneficiaryCountry: string | null;
  /** D207 (nerezidenți): codul fiscal din străinătate (cifS / NIF). */
  beneficiaryForeignTaxId: string | null;
  note: string | null;
  taxDeadline: string;
}
export interface DividendInput {
  companyId: string;
  distributionDate: string;
  paymentDate?: string | null;
  grossAmount: string;
  interim2025?: boolean;
  shareholder?: string | null;
  beneficiaryCnp?: string | null;
  beneficiaryResident?: boolean;
  beneficiaryType?: "PF" | "PJ";
  note?: string | null;
}
/** DIV-01: editare in-place a identității beneficiarului (CNP/nume/rezidență/tip) + dată plată/notă.
 *  NU schimbă sumele (brut/impozit postează GL); pentru a corecta un CNP fără a șterge înregistrarea. */
export interface DividendBeneficiaryUpdate {
  id: string;
  companyId: string;
  paymentDate?: string | null;
  shareholder?: string | null;
  beneficiaryCnp?: string | null;
  beneficiaryResident?: boolean;
  beneficiaryType?: "PF" | "PJ";
  /** D207 (nerezidenți): țara de rezidență (Stat_R, cod 2 litere) + codul fiscal străin (cifS). */
  beneficiaryCountry?: string | null;
  beneficiaryForeignTaxId?: string | null;
  note?: string | null;
}
export const dividends = {
  list: (companyId: string) => invoke<Dividend[]>("list_dividends", { companyId }),
  create: (input: DividendInput) => invoke<Dividend>("create_dividend", { input }),
  updateBeneficiary: (update: DividendBeneficiaryUpdate) =>
    invoke<Dividend>("update_dividend_beneficiary", { update }),
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_dividend", { id, companyId }),
  /**
   * Exportă D205 (informativă anuală, pe beneficiar) pentru anul de venit `year` — capitolul
   * dividende, validat cu `D205Validator.jar` (DUK). Returnează `OfficialExportResult` (ca D406/D112);
   * `written=false` + `issues` dacă DUK raportează erori (re-apelați cu `skipDukOverride=true`).
   */
  exportD205Official: (
    companyId: string,
    year: number,
    destPath: string,
    skipDukOverride = false,
    isRectificative = false,
  ) =>
    invoke<OfficialExportResult>("export_d205_official", {
      params: { companyId, year, destPath, isRectificative },
      skipDukOverride,
    }),
  /**
   * Construiește XML-ul D205 (`:v3`) pentru anul de venit `year` FĂRĂ a-l scrie pe disc — pentru
   * vizualizatorul/editorul XML din aplicație. Re-validarea DUK se face cu `validateDeclarationXml`.
   */
  previewD205Xml: (companyId: string, year: number, isRectificative = false) =>
    invoke<string>("preview_d205_xml", { companyId, year, isRectificative }),
  /**
   * Exportă D207 (informativă anuală, beneficiari NEREZIDENȚI) pentru anul de venit `year`. D207 nu
   * are validator DUK — conformitatea e garantată de XSD-ul oficial. Scrie XML-ul la `destPath`.
   */
  exportD207Official: (companyId: string, year: number, destPath: string, isRectificative = false) =>
    invoke<OfficialExportResult>("export_d207_official", {
      params: { companyId, year, destPath, isRectificative },
    }),
  /** Construiește XML-ul D207 fără a-l scrie — pentru vizualizatorul XML din aplicație. */
  previewD207Xml: (companyId: string, year: number, isRectificative = false) =>
    invoke<string>("preview_d207_xml", { companyId, year, isRectificative }),
};

// ─── GL — Jurnal contabil ──────────────────────────────────────────────────

export const gl = {
  /**
   * Generează (sau re-generează idempotent) notele contabile GL pentru o perioadă.
   * Rust command `generate_gl_entries` (flat args): company_id, period_from, period_to
   */
  generateEntries: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<GlPostResult>("generate_gl_entries", { companyId, periodFrom, periodTo }),
  /**
   * Reconciliază GL cu D300 pentru o perioadă.
   * Rust command `reconcile_gl` (flat args): company_id, period_from, period_to
   */
  reconcile: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<ReconcileReport>("reconcile_gl", { companyId, periodFrom, periodTo }),
  /**
   * Închiderea/regularizarea TVA: netează 4426/4427 → 4423 (de plată) / 4424 (de recuperat).
   * Rust command `close_vat_period` (flat args): company_id, period_from, period_to
   */
  closeVat: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<VatSettlementResult>("close_vat_period", { companyId, periodFrom, periodTo }),
  /**
   * Balanța de verificare (cod 14-6-30, patru egalități) pentru perioadă.
   * Rust command `trial_balance` (flat args): company_id, period_from, period_to
   */
  trialBalance: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<TrialBalance>("trial_balance", { companyId, periodFrom, periodTo }),
  /** Contul de profit și pierdere (P&L) + notele de închidere 6/7 → 121. */
  profitAndLoss: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<ProfitLoss>("profit_and_loss", { companyId, periodFrom, periodTo }),
  /** Bilanț contabil (balance sheet) pentru perioadă. */
  bilant: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<BilantReport>("bilant", { companyId, periodFrom, periodTo }),
  /** Exportă bilanțul XML oficial ANAF (S1005 micro) la destPath. Returnează calea. */
  exportBilantXml: (
    companyId: string,
    year: number,
    caen: string,
    avgEmployees: number | null,
    formOverride: string | null,
    priorYearForm: string | null,
    destPath: string,
  ) =>
    invoke<string>("export_bilant_xml", {
      companyId, year, caen, avgEmployees, formOverride, priorYearForm, destPath,
    }),
  /** Construiește XML-ul bilanțului fără a-l scrie — pentru vizualizatorul XML (fără DUK). */
  previewBilantXml: (
    companyId: string,
    year: number,
    caen: string,
    avgEmployees: number | null,
    formOverride: string | null,
    priorYearForm: string | null,
  ) =>
    invoke<string>("preview_bilant_xml", {
      companyId, year, caen, avgEmployees, formOverride, priorYearForm,
    }),
  /** Postează impozitul pe venit/profit (698/691 → 4418/4411); amount = override opțional. */
  postIncomeTax: (companyId: string, periodFrom: string, periodTo: string, amount?: string) =>
    invoke<IncomeTaxResult>("post_income_tax", { companyId, periodFrom, periodTo, amount: amount ?? null }),
  /** Închiderea anuală 121 → 117 «Rezultatul reportat». */
  postAnnualClose: (companyId: string, year: number) =>
    invoke<AnnualCloseResult>("post_annual_close", { companyId, year }),
  /** Postează închiderea conturilor 6/7 → 121 (idempotent per perioadă). */
  closePeriod: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<ClosePeriodResult>("close_period", { companyId, periodFrom, periodTo }),
  /** Registru-jurnal (cod 14-1-1). */
  journalRegister: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<JournalRegister>("journal_register", { companyId, periodFrom, periodTo }),
  /** Cartea mare (cod 14-1-3) — câte o filă pe cont. */
  generalLedger: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<LedgerAccount[]>("general_ledger", { companyId, periodFrom, periodTo }),
  /** Fișă de cont pe furnizor/client (fișă analitică terți) — filele de cont ale partenerului. */
  partnerLedger: (companyId: string, partnerCui: string, periodFrom: string, periodTo: string) =>
    invoke<LedgerAccount[]>("partner_ledger", { companyId, partnerCui, periodFrom, periodTo }),

  // ── Note contabile manuale (cod 14-6-2A) ─────────────────────────────────────

  /**
   * Creează o notă contabilă manuală.
   * Rust command `create_manual_journal`: company_id, date, description, lines.
   * Returnează source_id-ul (UUID) al notei create.
   */
  createManualJournal: (
    companyId: string,
    date: string,
    description: string,
    lines: ManualLineInput[],
  ) =>
    invoke<string>("create_manual_journal", { companyId, date, description, lines }),

  /**
   * Listează notele contabile manuale dintr-o perioadă.
   * Rust command `list_manual_journals`: company_id, period_from, period_to.
   */
  listManualJournals: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<ManualJournalView[]>("list_manual_journals", { companyId, periodFrom, periodTo }),

  /**
   * Șterge o notă contabilă manuală (ONLY source_type='MANUAL').
   * Rust command `delete_manual_journal`: company_id, source_id.
   * Returnează true dacă nota a fost ștearsă, false dacă nu exista.
   */
  deleteManualJournal: (companyId: string, sourceId: string) =>
    invoke<boolean>("delete_manual_journal", { companyId, sourceId }),
};

// ─── Declarations (D300) ──────────────────────────────────────────────────

/** A single pre-export validation finding from the Rust preflight engine. */
export interface PreflightIssue {
  severity: "error" | "warning";
  code: string;
  message: string;
  hint: string;
}

/** Tipuri de declarație pe care le validează DUK (din editorul XML din aplicație). */
export type XmlDeclKind = "D300" | "D394" | "D406" | "D112" | "D205";

/** Verdictul re-validării cu DUK a unui șir XML (vezi `declarations.validateDeclarationXml`). */
export interface XmlDukValidation {
  /** A fost disponibil validatorul (jar + runtime Java)? Dacă nu, nu blocăm editarea. */
  available: boolean;
  /** A trecut validarea (relevant doar când `available`). */
  passed: boolean;
  issues: PreflightIssue[];
}

/** Result of an official export attempt — includes DUK gate outcome. */
export interface OfficialExportResult {
  /** Written file path, or empty string if blocked by DUK. */
  path: string;
  written: boolean;
  /** Whether a DUK runtime was available to validate. */
  dukAvailable: boolean;
  /** Whether DUK reported clean (only meaningful when dukAvailable). */
  dukPassed: boolean;
  issues: PreflightIssue[];
}

export const declarations = {
  /** Calculează decontul D300 — TVA colectat (vânzări) pentru o perioadă. */
  compute: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").D300Report>("compute_d300", { companyId, periodFrom, periodTo }),
  /** RO e-TVA: reconciliază D300 calculat vs decontul precompletat (P300ETVA) — self-check. */
  reconcileEtva: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    precompletat: import("@/types").EtvaPrecompletat,
  ) =>
    invoke<import("@/types").EtvaReconciliation>("reconcile_etva", {
      companyId,
      periodFrom,
      periodTo,
      precompletat,
    }),
  /** Fetch the e-TVA decont precompletat (P300ETVA) zip from ANAF → its JSON files (raw). */
  fetchEtvaPrecompletat: (companyId: string, an: number, luna: number, testMode = false) =>
    invoke<import("@/types").EtvaPrecompletatFile[]>("etva_fetch_precompletat", {
      companyId,
      an,
      luna,
      testMode,
    }),
  /** Calcul salariu (nucleul D112): brut → net + contribuții, ratele 2026. */
  computePayroll: (input: import("@/types").PayrollInput) =>
    invoke<import("@/types").PayrollResult>("compute_payroll", { input }),
  /** Intrastat threshold monitor (1.000.000 lei per flow, Ord. INS 1604/2025). */
  intrastatStatus: (companyId: string, asOf: string) =>
    invoke<import("@/types").IntrastatStatus>("intrastat_status", { companyId, asOf }),
  /** D100 (obligații de plată) quarterly row — micro 121 / profit 103, from the period P&L. */
  computeD100: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    quarter: number,
    year: number,
    priorPayments: string,
  ) =>
    invoke<import("@/types").D100Result>("compute_d100", {
      companyId, periodFrom, periodTo, quarter, year, priorPayments,
    }),
  /**
   * Re-validează un șir XML ARBITRAR (posibil editat) cu validatorul OFICIAL ANAF (DUK) pentru tipul
   * declarației — pentru butonul „re-validează cu DUK" din editorul XML. `available=false` dacă jar-ul
   * validatorului lipsește din build sau runtime-ul Java nu e disponibil (nu blocăm editarea).
   */
  validateDeclarationXml: (declKind: XmlDeclKind, xml: string) =>
    invoke<XmlDukValidation>("validate_declaration_xml", { declKind, xml }),
  /**
   * Exportă declarația ca tabel XLSX (citibil în Excel/Numbers) din modelul de tabele produs de
   * `xmlToTables`. `.xml`-ul canonic rămâne neschimbat (depunere SPV); acesta e un fișier separat.
   */
  exportDeclarationXlsx: (
    tables: import("@/lib/xml-to-tables").DeclTable[],
    destPath: string,
  ) => invoke<string>("export_declaration_xlsx", { tables, destPath }),
  /** Write a print-ready HTML document to the app cache dir and open it in the default browser
   *  (where window.print() → "Save as PDF" works; the WKWebView can't print). */
  openDocInBrowser: (html: string, fileName: string) =>
    invoke<void>("open_doc_in_browser", { html, fileName }),
  /** D101 (impozit pe profit) worksheet: base from the period P&L + the supplied adjustments. */
  computeD101: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    input: import("@/types").D101Input,
  ) =>
    invoke<import("@/types").D101Result>("compute_d101", {
      companyId,
      periodFrom,
      periodTo,
      input,
    }),
  /**
   * Generează XML D300 și îl salvează la destPath. Returnează calea.
   * R4: `manualDeductibleVat` — when provided, overrides the server-computed
   * total_deductible_vat so the exported XML matches what the user sees on screen.
   * When omitted (undefined/null), the server-computed value is used.
   */
  export: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    destPath: string,
    manualDeductibleVat?: string | null,
  ) =>
    invoke<string>("export_d300", {
      companyId,
      periodFrom,
      periodTo,
      destPath,
      manualDeductibleVat: manualDeductibleVat ?? null,
    }),
  /** Construiește XML-ul D300 oficial (schema v12) fără a-l scrie — pentru vizualizatorul XML (re-validare DUK separat). */
  previewD300Xml: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    submission: D300Submission,
  ) => invoke<string>("preview_d300_xml", { companyId, periodFrom, periodTo, submission }),
  /**
   * Exportă XML D300 oficial ANAF (schema v12) la destPath.
   * `submission` conține câmpurile completate de utilizator (declarant, CAEN, bancă etc.).
   * Parametrii Rust (snake_case → camelCase Tauri):
   *   company_id, period_from, period_to, submission (D300Submission), dest_path
   */
  exportD300Official: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    destPath: string,
    submission: D300Submission,
    skipDukOverride = false,
  ) =>
    invoke<OfficialExportResult>("export_d300_official", {
      companyId,
      periodFrom,
      periodTo,
      submission,
      destPath,
      skipDukOverride,
    }),
  /**
   * Pre-export validation — runs pure-Rust checks and returns friendly Romanian
   * messages for common DUKIntegrator-fatal issues.
   * `kind` is one of: "D300", "D394", "D406".
   */
  preflight: (
    companyId: string,
    kind: string,
    periodFrom: string,
    periodTo: string,
  ) =>
    invoke<PreflightIssue[]>("preflight_declaration", {
      companyId,
      kind,
      periodFrom,
      periodTo,
    }),
  /** Listează depunerile înregistrate pentru o firmă (cele mai recente primele). */
  listFilings: (companyId: string) =>
    invoke<import("@/types").Filing[]>("list_declaration_filings", { companyId }),
  /** Șterge o depunere din istoric (company-scoped). */
  deleteFiling: (id: string, companyId: string) =>
    invoke<void>("delete_declaration_filing", { id, companyId }),
};

// ─── e-Transport (UIT) ───────────────────────────────────────────────────

export const etransport = {
  /** Validează o declarație e-Transport. Returnează lista de probleme (gol = valid). */
  validate: (declaration: import("@/types").EtransportDeclaration) =>
    invoke<string[]>("etransport_validate", { declaration }),
  /** Validează + generează XML-ul e-Transport (schema v2). */
  generateXml: (declaration: import("@/types").EtransportDeclaration) =>
    invoke<string>("etransport_generate_xml", { declaration }),
  /** Trimite declarația la ANAF (live). Returnează indexul + Cod UIT. */
  submit: (
    companyId: string,
    declaration: import("@/types").EtransportDeclaration,
    testMode = false,
  ) =>
    invoke<import("@/types").EtransportUploadResponse>("etransport_submit", {
      companyId,
      declaration,
      testMode,
    }),
  /** Evidența declarațiilor transmise (UIT + termen de valabilitate). */
  listDeclarations: (companyId: string) =>
    invoke<import("@/types").EtransportDeclRecord[]>("list_etransport_declarations", { companyId }),
};

// ─── D390 — Declarație recapitulativă (VIES) intra-UE ────────────────────

export const d390 = {
  /** Calculează D390 — operațiuni intra-UE grupate pe partener + tip (L/A/P/S). */
  compute: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").D390Doc>("compute_d390", { companyId, periodFrom, periodTo }),
  /** Generează XML D390 (declaratie390 v3) și îl salvează la destPath. Returnează calea. */
  export: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    destPath: string,
    submission?: import("@/types").D390Submission,
  ) =>
    invoke<string>("export_d390", { companyId, periodFrom, periodTo, destPath, submission }),
  /** Construiește XML-ul D390 fără a-l scrie — pentru vizualizatorul XML (D390 nu are validator DUK). */
  previewD390Xml: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    submission?: import("@/types").D390Submission,
  ) =>
    invoke<string>("preview_d390_xml", {
      companyId,
      periodFrom,
      periodTo,
      submission: submission ?? null,
    }),
};

// ─── D394 — Declarație informativă livrări/achiziții ─────────────────────

export const d394 = {
  /** Calculează declarația D394 — livrări (vânzări) grupate pe partener. */
  compute: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").D394Report>("compute_d394", { companyId, periodFrom, periodTo }),
  /** Generează XML D394 și îl salvează la destPath. Returnează calea. */
  export: (companyId: string, periodFrom: string, periodTo: string, destPath: string) =>
    invoke<string>("export_d394", { companyId, periodFrom, periodTo, destPath }),
  /** Construiește XML-ul D394 oficial fără a-l scrie — pentru vizualizatorul XML (re-validare DUK separat). */
  previewD394Xml: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    submission: D394Submission,
  ) => invoke<string>("preview_d394_xml", { companyId, periodFrom, periodTo, submission }),
  /**
   * Exportă XML D394 oficial ANAF (schema v5) la destPath.
   * `submission` conține câmpurile completate de utilizator (CAEN, reprezentant etc.).
   * Parametrii Rust (snake_case → camelCase Tauri):
   *   company_id, period_from, period_to, submission (D394Submission), dest_path
   */
  exportD394Official: (
    companyId: string,
    periodFrom: string,
    periodTo: string,
    destPath: string,
    submission: D394Submission,
    skipDukOverride = false,
  ) =>
    invoke<OfficialExportResult>("export_d394_official", {
      companyId,
      periodFrom,
      periodTo,
      submission,
      destPath,
      skipDukOverride,
    }),
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

// ─── Products (articole / catalog) ────────────────────────────────────────

/** R15: All product commands are company_id-scoped. */
export const products = {
  /** List products for a company, with optional name/code search. */
  list: (companyId: string, query?: string) =>
    invoke<Product[]>("list_products", { companyId, query: query ?? null }),
  /** Get a single product. Returns NotFound for wrong company. */
  get: (id: string, companyId: string) =>
    invoke<Product>("get_product", { id, companyId }),
  /** Create a product for the given company. */
  create: (companyId: string, input: ProductInput) =>
    invoke<Product>("create_product", { companyId, input }),
  /** Update a product. Cross-company update returns NotFound. */
  update: (id: string, companyId: string, input: UpdateProductInput) =>
    invoke<Product>("update_product", { id, companyId, input }),
  /** Delete a product. Cross-company deletion returns NotFound. */
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_product", { id, companyId }),
  /** Search products by name/code for the picker. Scoped to company. */
  search: (companyId: string, query: string) =>
    invoke<Product[]>("search_products", { companyId, query }),
};

// ─── VAT Rates — global editable catalog (R15 Wave 2) ────────────────────

/**
 * R15 Wave 2: All vatRates commands operate on the GLOBAL `vat_rates` table.
 * No company_id is passed — Romanian VAT rates are national (same for all
 * companies). This is the deliberate exception to the company-scoping rule.
 */
export const stockValuation = {
  recordReceipt: (input: import("@/types").StockMovementInput) =>
    invoke<string | null>("record_stock_receipt", { input }),
  recordIssue: (input: import("@/types").StockMovementInput) =>
    invoke<string | null>("record_stock_issue", { input }),
  ledger: (companyId: string, productId: string) =>
    invoke<import("@/types").StockLedgerRow[]>("stock_ledger", { companyId, productId }),
  setValuation: (companyId: string, productId: string, method: string, stockAccount: string) =>
    invoke<void>("set_stock_valuation", { companyId, productId, method, stockAccount }),
};

export const assets = {
  list: (companyId: string) =>
    invoke<import("@/types").FixedAsset[]>("list_fixed_assets", { companyId }),
  create: (companyId: string, input: import("@/types").FixedAssetInput) =>
    invoke<import("@/types").FixedAsset>("create_fixed_asset", { companyId, input }),
  update: (id: string, companyId: string, input: import("@/types").FixedAssetInput) =>
    invoke<import("@/types").FixedAsset>("update_fixed_asset", { id, companyId, input }),
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_fixed_asset", { id, companyId }),
  runDepreciation: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").DepreciationRun>("run_depreciation", { companyId, periodFrom, periodTo }),
  dispose: (companyId: string, assetId: string, disposalDate: string) =>
    invoke<void>("dispose_asset", { companyId, assetId, disposalDate }),
};

export const payroll = {
  list: (companyId: string) =>
    invoke<import("@/types").Employee[]>("list_employees", { companyId }),
  create: (input: import("@/types").CreateEmployeeInput) =>
    invoke<import("@/types").Employee>("create_employee", { input }),
  update: (id: string, companyId: string, input: import("@/types").UpdateEmployeeInput) =>
    invoke<import("@/types").Employee>("update_employee", { id, companyId, input }),
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_employee", { id, companyId }),
  listSedii: (companyId: string) =>
    invoke<import("@/types").SecondaryOffice[]>("list_secondary_offices", { companyId }),
  createSediu: (companyId: string, cif: string, name: string) =>
    invoke<import("@/types").SecondaryOffice>("create_secondary_office", { companyId, cif, name }),
  deleteSediu: (id: string, companyId: string) =>
    invoke<void>("delete_secondary_office", { id, companyId }),
  listConcedii: (companyId: string, periodYm: string) =>
    invoke<import("@/types").MedicalLeave[]>("list_medical_leaves", { companyId, periodYm }),
  createConcediu: (input: import("@/types").MedicalLeaveInput) =>
    invoke<import("@/types").MedicalLeave>("create_medical_leave", { input }),
  deleteConcediu: (id: string, companyId: string) =>
    invoke<void>("delete_medical_leave", { id, companyId }),
  run: (companyId: string, periodFrom: string, periodTo: string) =>
    invoke<import("@/types").PayrollRun>("run_payroll", { companyId, periodFrom, periodTo }),
  /** Exportă D112 (XML) pentru luna dată la destPath. Returnează calea. */
  /**
   * Exportă D112 (XML `:v7`) la destPath, cu gate DUK: XML-ul e validat cu validatorul OFICIAL ANAF
   * `D112Validator.jar` inclus în app (`-v D112`) înainte de scriere. Dacă DUK raportează ERORI,
   * `written=false` + `issues` (re-apelați cu `skipDukOverride=true` pentru a forța scrierea).
   * Returnează `OfficialExportResult` (ca D300/D394/D406).
   */
  exportD112Xml: (
    companyId: string,
    year: number,
    month: number,
    caen: string,
    destPath: string,
    skipDukOverride = false,
    isRectificative = false,
  ) =>
    invoke<OfficialExportResult>("export_d112_xml", {
      params: { companyId, year, month, caen, destPath, isRectificative },
      skipDukOverride,
    }),
  /** Construiește XML-ul D112 (`:v7`) fără a-l scrie — pentru vizualizatorul XML (re-validare DUK separat). */
  previewD112Xml: (companyId: string, year: number, month: number, caen: string, isRectificative = false) =>
    invoke<string>("preview_d112_xml", { companyId, year, month, caen, isRectificative }),
};

export const vatRates = {
  /** List all (or only active) VAT rates from the global catalog. */
  list: (activeOnly?: boolean) =>
    invoke<VatRate[]>("list_vat_rates", { activeOnly: activeOnly ?? null }),
  /** Get a single VAT rate by id. Returns NotFound if missing. */
  get: (id: string) => invoke<VatRate>("get_vat_rate", { id }),
  /** Legislative advisory (Legea 141/2025) for a rate on an issue date — null if none. */
  note: (ratePct: number, issueDate: string) =>
    invoke<string | null>("vat_rate_note", { ratePct, issueDate }),
  /** Create a new VAT rate entry. */
  create: (input: VatRateInput) => invoke<VatRate>("create_vat_rate", { input }),
  /** Update an existing VAT rate entry. */
  update: (id: string, input: UpdateVatRateInput) =>
    invoke<VatRate>("update_vat_rate", { id, input }),
  /** Delete a VAT rate entry by id. */
  delete: (id: string) => invoke<void>("delete_vat_rate", { id }),
  /** Activate or deactivate a VAT rate entry. */
  setActive: (id: string, active: boolean) =>
    invoke<VatRate>("set_vat_rate_active", { id, active }),
};

// ─── Receipts (chitanțe) — R15 Wave 3 ────────────────────────────────────

/** R15 Wave 3: All receipt commands are company_id-scoped. */
export const receipts = {
  /** List all receipts for a company. */
  list: (companyId: string) =>
    invoke<Receipt[]>("list_receipts", { companyId }),
  /** Get a single receipt. Returns NotFound for wrong company. */
  get: (id: string, companyId: string) =>
    invoke<Receipt>("get_receipt", { id, companyId }),
  /** Create a receipt for the given company. */
  create: (companyId: string, input: ReceiptInput) =>
    invoke<Receipt>("create_receipt", { companyId, input }),
  /** Delete a receipt. Cross-company deletion returns NotFound. */
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_receipt", { id, companyId }),
  /** Generate and save a chitanță PDF. Returns the file path. */
  generatePdf: (id: string, companyId: string) =>
    invoke<string>("generate_receipt_pdf", { id, companyId }),
};

// ─── Accounts — plan de conturi (R15 Wave 4) ──────────────────────────────

/** R15 Wave 4: All account commands are company_id-scoped. */
export const accounts = {
  /** List all accounts for a company, ordered by account_code. */
  list: (companyId: string) =>
    invoke<Account[]>("list_accounts", { companyId }),
  /** Get a single account. Returns NotFound for wrong company. */
  get: (id: string, companyId: string) =>
    invoke<Account>("get_account", { id, companyId }),
  /** Create an account for the given company. */
  create: (companyId: string, input: AccountInput) =>
    invoke<Account>("create_account", { companyId, input }),
  /** Update an account. Cross-company update returns NotFound. */
  update: (id: string, companyId: string, input: UpdateAccountInput) =>
    invoke<Account>("update_account", { id, companyId, input }),
  /** Delete an account. Cross-company deletion returns NotFound. */
  delete: (id: string, companyId: string) =>
    invoke<void>("delete_account", { id, companyId }),
  /** Seed the standard Romanian chart of accounts (idempotent). */
  seedStandard: (companyId: string) =>
    invoke<number>("seed_standard_accounts", { companyId }),
};

// ─── BNR — curs valutar oficial (R17 Wave 2) ──────────────────────────────

/** Returnează cursul oficial BNR (RON per 1 unitate valutară) la data cerută. */
export const bnr = {
  fetchRate: (currency: string, date: string) =>
    invoke<number>("fetch_bnr_rate", { currency, date }),
};

// ─── GDPR / data portability ──────────────────────────────────────────────

export const gdpr = {
  /** Export all user data (DB + archive) as a ZIP to the chosen path. */
  exportAll: (destPath: string) =>
    invoke<DataExportResult>("export_all_my_data", { destPath }),
  /** Irreversibly wipe all local data. Frontend MUST double-confirm. */
  wipeAll: (acknowledgeRetention?: boolean) =>
    invoke<void>("wipe_all_data", { acknowledgeRetention: acknowledgeRetention ?? null }),
};

// ─── Import Wave C ────────────────────────────────────────────────────────

/** A discovered column/key name with a sample value (from SAGA DBF header). */
export interface DetectedColumn {
  name: string;
  sample: string;
}

/** Per-entity resolution counts from the staging resolver. */
export interface EntityCounts {
  new: number;
  matched: number;
  dupInBatch: number;
  review: number;
  error: number;
}

/** Per-entity rollup returned by stage + preview. */
export interface BatchCounts {
  contacts: EntityCounts;
  products: EntityCounts;
  accounts: EntityCounts;
  invoices: EntityCounts;
}

/** Result of import_wave_c_stage. */
export interface StageResult {
  batchId: string;
  counts: BatchCounts;
  warnings: string[];
}

/** Sample rows + counts returned by import_wave_c_preview. */
export interface PreviewResult {
  batchId: string;
  counts: BatchCounts;
  sampleContacts: Record<string, string | null>[];
  sampleProducts: Record<string, string | null>[];
  sampleAccounts: Record<string, string | null>[];
  sampleInvoices: Record<string, string | null>[];
}

/** Per-entity breakdown from import_wave_c_commit. */
export interface EntityReport {
  created: number;
  matched: number;
  skipped: number;
  errors: number;
}

/** Full commit result returned by import_wave_c_commit. */
export interface CommitReport {
  batchId: string;
  contacts: EntityReport;
  products: EntityReport;
  accounts: EntityReport;
  invoices: EntityReport;
  errors: string[];
}

export const importWaveC = {
  /** Sniff column names + sample values from the first file (only useful for SAGA DBF). */
  detectColumns: (companyId: string, source: string, filePaths: string[]) =>
    invoke<DetectedColumn[]>("import_wave_c_detect_columns", { companyId, source, filePaths }),
  /** Parse + stage files, run the resolver, return batch_id + counts. */
  stage: (
    companyId: string,
    source: string,
    filePaths: string[],
    columnMap?: Record<string, string>,
  ) =>
    invoke<StageResult>("import_wave_c_stage", {
      companyId,
      source,
      filePaths,
      columnMap: columnMap ?? null,
    }),
  /** Return resolution counts + sample rows for a staged batch. */
  preview: (batchId: string) =>
    invoke<PreviewResult>("import_wave_c_preview", { batchId }),
  /** Commit all NEW rows in a batch and return the commit report. */
  commit: (batchId: string) =>
    invoke<CommitReport>("import_wave_c_commit", { batchId }),
};

// ─── API umbrella ─────────────────────────────────────────────────────────

export const api = {
  accounts,
  anaf,
  archive,
  bnr,
  certificates,
  companies,
  contacts,
  d390,
  d394,
  declarations,
  etransport,
  feedback,
  gdpr,
  gl,
  importData,
  importWaveC,
  integrations,
  invoices,
  journals,
  license,
  notifications,
  payments,
  products,
  receipts,
  received,
  receivedPayments,
  recurring,
  reports,
  assets,
  stockValuation,
  payroll,
  saft,
  dividends,
  settings,
  system,
  ubl,
  vatRates,
};
