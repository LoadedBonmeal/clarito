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
  subtotalAmount: number;
  vatAmount: number;
  totalAmount: number;
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
  quantity: number;
  unit: string;
  unitPrice: number;
  vatRate: number;
  vatCategory: VatCategory;
  subtotalAmount: number;
  vatAmount: number;
  totalAmount: number;
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
  totalAmount: number;
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
  baseAmount: number;
  vatAmount: number;
  invoiceCount: number;
}

export interface VatReport {
  dateFrom: string;
  dateTo: string;
  companyId: string | null;
  totalBase: number;
  totalVat: number;
  totalAmount: number;
  invoiceCount: number;
  vatGroups: VatGroup[];
  generatedAt: number;
}

export interface ExportReportParams {
  dateFrom?: string;
  dateTo?: string;
  companyId?: string;
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
