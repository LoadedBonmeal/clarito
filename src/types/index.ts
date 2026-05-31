/**
 * Tipuri care oglindesc structurile DB din `src-tauri/src/db/*`.
 *
 * Convenție serde: backend-ul folosește `#[serde(rename_all = "camelCase")]`
 * pe structuri, deci totul ajunge în JSON ca `camelCase`.
 *
 * Status enums sunt UPPERCASE pe wire.
 */

// ─── Enums ────────────────────────────────────────────────────────────────

export type InvoiceStatus =
  | "DRAFT"
  | "QUEUED"
  | "SUBMITTED"
  | "VALIDATED"
  | "REJECTED"
  | "STORNED";

export type ReceivedStatus =
  | "NEW"
  | "REVIEWED"
  | "APPROVED"
  | "REJECTED"
  | "ARCHIVED";

export type ContactType = "CUSTOMER" | "SUPPLIER" | "BOTH";

export type LicenseTier = "TRIAL" | "SOLO" | "ACCOUNTANT" | "FIRM";

export type VatCategory = "S" | "Z" | "E" | "AE" | "K" | "G" | "O";

// ─── Pagination ───────────────────────────────────────────────────────────

export interface Page {
  offset: number;
  limit: number;
}

export interface Paginated<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

// ─── Company ──────────────────────────────────────────────────────────────

export interface Company {
  id: string;
  cui: string;
  legalName: string;
  tradeName: string | null;
  registryNumber: string | null;
  vatPayer: boolean;

  address: string;
  city: string;
  county: string;
  postalCode: string | null;
  country: string;

  email: string | null;
  phone: string | null;
  iban: string | null;
  bankName: string | null;

  isActive: boolean;
  spvEnabled: boolean;

  invoiceSeries: string;
  lastInvoiceNumber: number;

  logoPath: string | null;

  createdAt: number;
  updatedAt: number;
}

export interface CreateCompanyInput {
  cui: string;
  legalName: string;
  tradeName?: string;
  registryNumber?: string;
  vatPayer?: boolean;
  address: string;
  city: string;
  county: string;
  postalCode?: string;
  country?: string;
  email?: string;
  phone?: string;
  iban?: string;
  bankName?: string;
  invoiceSeries?: string;
}

export type UpdateCompanyInput = Partial<
  Omit<CreateCompanyInput, "cui">
> & {
  isActive?: boolean;
  spvEnabled?: boolean;
  logoPath?: string;
};

// ─── Contact ──────────────────────────────────────────────────────────────

export interface Contact {
  id: string;
  companyId: string;
  contactType: ContactType;
  cui: string | null;
  legalName: string;
  vatPayer: boolean;
  address: string | null;
  city: string | null;
  county: string | null;
  country: string;
  email: string | null;
  phone: string | null;
  currency: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreateContactInput {
  companyId: string;
  contactType: ContactType;
  cui?: string;
  legalName: string;
  vatPayer?: boolean;
  address?: string;
  city?: string;
  county?: string;
  country?: string;
  email?: string;
  phone?: string;
  currency?: string;
}

export type UpdateContactInput = Partial<Omit<CreateContactInput, "companyId">>;

export interface ContactFilter {
  companyId?: string;
  query?: string;
}

// ─── Invoice ──────────────────────────────────────────────────────────────

export interface Invoice {
  id: string;
  companyId: string;
  contactId: string;
  series: string;
  number: number;
  fullNumber: string;
  issueDate: string;
  dueDate: string;
  currency: string;
  exchangeRate: number | null;
  subtotalAmount: string;
  vatAmount: string;
  totalAmount: string;
  status: InvoiceStatus;
  anafUploadId: string | null;
  anafIndex: string | null;
  anafSubmittedAt: number | null;
  anafValidatedAt: number | null;
  anafRejectedAt: number | null;
  xmlPath: string | null;
  pdfPath: string | null;
  signatureXmlPath: string | null;
  rejectionReason: string | null;
  rejectionCode: string | null;
  notes: string | null;
  paymentMeansCode: string;
  createdAt: number;
  updatedAt: number;
}

export interface LineItem {
  id: string;
  invoiceId: string;
  position: number;
  name: string;
  description: string | null;
  quantity: string;
  unit: string;
  unitPrice: string;
  vatRate: string;
  vatCategory: VatCategory;
  subtotalAmount: string;
  vatAmount: string;
  totalAmount: string;
  cpvCode: string | null;
}

export interface InvoiceEvent {
  id: string;
  invoiceId: string;
  eventType: string;
  message: string;
  metadata: string | null;
  createdAt: number;
}

export interface InvoiceWithLines {
  invoice: Invoice;
  lines: LineItem[];
  events: InvoiceEvent[];
}

export interface CreateLineInput {
  name: string;
  description?: string;
  quantity: number;
  unit: string;
  unitPrice: number;
  vatRate: number;
  vatCategory: VatCategory;
  cpvCode?: string;
}

export interface CreateInvoiceInput {
  companyId: string;
  contactId: string;
  series: string;
  number: number;
  issueDate: string;
  dueDate: string;
  currency?: string;
  exchangeRate?: number;
  notes?: string;
  paymentMeansCode?: string;
  lines: CreateLineInput[];
}

export interface InvoiceFilter {
  companyId?: string;
  statuses?: InvoiceStatus[];
  dateFrom?: string;
  dateTo?: string;
  query?: string;
  page?: Page;
}

// ─── Received Invoice ─────────────────────────────────────────────────────

export interface ReceivedInvoice {
  id: string;
  companyId: string;
  anafDownloadId: string;
  anafIndex: string | null;
  issuerCui: string;
  issuerName: string;
  series: string | null;
  number: string | null;
  totalAmount: string;
  currency: string;
  issueDate: string;
  xmlPath: string;
  pdfPath: string | null;
  status: ReceivedStatus;
  downloadedAt: number;
  createdAt: number;
}

export interface ReceivedFilter {
  companyId?: string;
  statuses?: ReceivedStatus[];
  page?: Page;
}

// ─── Notification ─────────────────────────────────────────────────────────

export interface Notification {
  id: string;
  notificationType: string;
  title: string;
  body: string;
  data: string | null;
  isRead: boolean;
  readAt: number | null;
  osNotificationShown: boolean;
  createdAt: number;
}

// ─── License ──────────────────────────────────────────────────────────────

export interface License {
  id: number;
  licenseKey: string | null;
  tier: LicenseTier;
  activatedAt: number | null;
  expiresAt: number;
  machineId: string;
  email: string | null;
  lastValidatedAt: number | null;
  /** True if `expiresAt` is in the past. Computed by the backend on each fetch. */
  isExpired: boolean;
  /** Days remaining in a TRIAL period (negative when expired). Null for non-TRIAL tiers. */
  trialDaysRemaining: number | null;
}

// ─── System ───────────────────────────────────────────────────────────────

export interface AppInfo {
  name: string;
  version: string;
  dbPath: string;
  appDataDir: string;
}

export interface SyncResult {
  statusPolls: number;
  newReceived: number;
  updatedAt: number;
}

// ─── UBL / XML validation ─────────────────────────────────────────────────

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

// ─── Certificate ANAF ────────────────────────────────────────────────────

export interface Certificate {
  id: string;
  companyId: string;
  keychainRef: string;
  issuedAt: number;
  expiresAt: number;
  refreshableUntil: number;
  isActive: boolean;
  lastRefreshedAt: number | null;
  lastUsedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AnafCompanyData {
  cui: string;
  legalName: string;
  address: string;
  city: string;
  county: string;
  postalCode: string | null;
  registryNumber: string | null;
  phone: string | null;
  vatPayer: boolean;
  active: boolean;
}

export interface AnafStatusResult {
  stare: string;
  descriere: string | null;
  anafIndex: string | null;
}

// ─── Reports ─────────────────────────────────────────────────────────────

export interface VatGroup {
  rate: number;
  baseAmount: string;
  vatAmount: string;
  invoiceCount: number;
}

export interface VatReport {
  dateFrom: string;
  dateTo: string;
  companyId: string | null;
  totalBase: string;
  totalVat: string;
  totalAmount: string;
  invoiceCount: number;
  vatGroups: VatGroup[];
  generatedAt: number;
}

export interface ExportReportParams {
  dateFrom?: string;
  dateTo?: string;
  companyId?: string;
}

// ─── D300 Decont TVA ─────────────────────────────────────────────────────────

/** Un grup de TVA colectat (cotă + categorie) din D300. */
export interface D300Group {
  vatRate: string;
  vatCategory: string;
  base: string;
  vat: string;
}

/** Raportul D300 — TVA colectat (vânzări), calculat din facturi VALIDATED. */
export interface D300Report {
  companyCui: string;
  periodFrom: string;
  periodTo: string;
  groups: D300Group[];
  totalBase: string;
  totalVat: string;
  invoiceCount: number;
}

// ─── D394 Declarație informativă livrări/achiziții ───────────────────────────

/** Un partener (client) din declarația D394 — livrări (vânzări). */
export interface D394Partner {
  /** CUI-ul partenerului. Poate fi "" dacă nu a fost completat. */
  partnerCui: string;
  /** Denumirea legală a partenerului. */
  partnerName: string;
  /** Numărul de facturi VALIDATED emise către partener în perioadă. */
  invoiceCount: number;
  /** Baza impozabilă totală (net), 2 zecimale. */
  base: string;
  /** TVA colectat total, 2 zecimale. */
  vat: string;
}

/** Raportul D394 — livrări (vânzări) per partener, calculat din facturi VALIDATED. */
export interface D394Report {
  companyCui: string;
  periodFrom: string;
  periodTo: string;
  /** Parteneri sortați descrescător după baza impozabilă. */
  partners: D394Partner[];
  totalBase: string;
  totalVat: string;
  invoiceCount: number;
}

// ─── Feedback / Diagnostic ────────────────────────────────────────────────

export interface DiagnosticReport {
  appVersion: string;
  os: string;
  arch: string;
  machineIdHash: string;
  logTail: string[];
  licenseSummary: { tier: string; daysRemaining: number | null };
}

// ─── GDPR ────────────────────────────────────────────────────────────────────

export interface DataExportResult {
  path: string;
  bytes: number;
}

// ─── Error (din backend) ──────────────────────────────────────────────────

export interface AppErrorPayload {
  kind:
    | "NotFound"
    | "Validation"
    | "Database"
    | "Migration"
    | "Io"
    | "Json"
    | "Tauri"
    | "Conflict"
    | "Xml"
    | "Pdf"
    | "Http"
    | "Other";
  message: string;
}
