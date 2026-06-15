/**
 * DEMO MODE — dev-only browser harness for pixel-verifying the UI against the
 * Claude-Design prototypes WITHOUT the Tauri runtime.
 *
 * Activated only when BOTH: the Vite dev build (`import.meta.env.DEV`) AND the
 * page URL carries `?demo=1`. In that mode `invoke()` (lib/tauri.ts) routes here
 * instead of Tauri IPC and serves fixtures that mirror the design handoff's demo
 * data (Andrei Consulting SRL · RO 41927384 · iunie 2026). Production builds and
 * the real desktop app are completely unaffected.
 */

import type { Company, Contact, Invoice, License, Notification, Paginated, ReceivedInvoice } from "@/types";

export function isDemoMode(): boolean {
  return (
    import.meta.env.DEV &&
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).has("demo")
  );
}

// ── Fixture data (mirrors the design prototype) ───────────────────────────────

const NOW = Math.floor(Date.now() / 1000);
const CO_ID = "demo-co";

const company: Company = {
  id: CO_ID,
  cui: "RO41927384",
  legalName: "Andrei Consulting SRL",
  tradeName: null,
  registryNumber: "J12/3456/2019",
  vatPayer: true,
  cashVat: false,
  address: "Str. Memorandumului nr. 28",
  city: "Cluj-Napoca",
  county: "CJ",
  postalCode: "400114",
  country: "RO",
  email: "andrei@consulting.ro",
  phone: null,
  iban: "RO49AAAA1B31007593840000",
  bankName: "Banca Transilvania",
  isActive: true,
  spvEnabled: true,
  taxRegime: "micro",
  invoiceSeries: "FACT",
  lastInvoiceNumber: 42,
  logoPath: null,
  createdAt: NOW,
  updatedAt: NOW,
} as unknown as Company;

const CLIENTS = [
  "Mavericks SRL", "Delgado Prod SRL", "Nordic Build SRL", "Lumen Studio SRL",
  "Carpat Logistic SRL", "Aurora Trade SRL", "Vertex Media SRL", "Orion Tech SRL",
];
const contacts: Contact[] = CLIENTS.map((name, i) => ({
  id: `demo-ct-${i}`,
  companyId: CO_ID,
  contactType: "CUSTOMER",
  cui: `RO4120010${i}`,
  legalName: name,
  vatPayer: true,
  cashVat: false,
  isIndividual: false,
  address: null,
  city: "Cluj-Napoca",
  county: "CJ",
  country: "RO",
  email: null,
  phone: null,
  currency: null,
  createdAt: NOW,
  updatedAt: NOW,
} as unknown as Contact));

// Monthly emise/primite counts — exactly the design chart (Ian..Iun 2026).
const EMISE_PER_MONTH = [74, 88, 96, 110, 118, 128];
const PRIMITE_PER_MONTH = [38, 42, 51, 48, 53, 54];

function buildInvoices(): Invoice[] {
  const rows: Invoice[] = [];
  let n = 0;
  EMISE_PER_MONTH.forEach((count, mi) => {
    for (let k = 0; k < count; k++) {
      n += 1;
      const day = (k % 27) + 1;
      const net = 700 + ((n * 137) % 900); // varied, deterministic
      const vat = Math.round(net * 0.21 * 100) / 100;
      // Status mix ≈ design: mostly validated, some submitted/queued, a few drafts.
      const r = n % 10;
      const status = r < 6 ? "VALIDATED" : r < 8 ? "SUBMITTED" : r < 9 ? "QUEUED" : "DRAFT";
      rows.push({
        id: `demo-inv-${n}`,
        companyId: CO_ID,
        contactId: contacts[n % contacts.length].id,
        series: "FACT",
        number: n,
        fullNumber: `FACT-2026-${String(n).padStart(4, "0")}`,
        issueDate: `2026-0${mi + 1}-${String(day).padStart(2, "0")}`,
        dueDate: `2026-0${mi + 1}-${String(Math.min(28, day + 14)).padStart(2, "0")}`,
        currency: "RON",
        exchangeRate: null,
        subtotalAmount: net.toFixed(2),
        vatAmount: vat.toFixed(2),
        totalAmount: (net + vat).toFixed(2),
        status,
        anafUploadId: null, anafIndex: null, anafSubmittedAt: null,
        anafValidatedAt: null, anafRejectedAt: null,
        xmlPath: null, pdfPath: null, signatureXmlPath: null,
        rejectionReason: null, rejectionCode: null, notes: null,
        paymentMeansCode: "30", stornoOfInvoiceId: null,
        createdAt: NOW, updatedAt: NOW,
      } as unknown as Invoice);
    }
  });
  return rows.reverse(); // newest first, like the backend
}

function buildReceived(): ReceivedInvoice[] {
  const sup = [["Furnizor Alpha SRL", "RO11220033"], ["Beta Distrib SRL", "RO22330044"], ["Gamma Office SRL", "RO33440055"]];
  const rows: ReceivedInvoice[] = [];
  let n = 0;
  PRIMITE_PER_MONTH.forEach((count, mi) => {
    for (let k = 0; k < count; k++) {
      n += 1;
      const [name, cui] = sup[n % 3];
      const net = 400 + ((n * 73) % 600);
      const vat = Math.round(net * 0.21 * 100) / 100;
      rows.push({
        id: `demo-rcv-${n}`,
        companyId: CO_ID,
        anafDownloadId: `dl-${n}`,
        anafIndex: null,
        issuerCui: cui,
        issuerName: name,
        series: "B", number: n,
        totalAmount: (net + vat).toFixed(2),
        currency: "RON",
        issueDate: `2026-0${mi + 1}-${String((k % 26) + 1).padStart(2, "0")}`,
        xmlPath: "/demo.xml", pdfPath: null,
        status: "POSTED",
        netAmount: net.toFixed(2), vatAmount: vat.toFixed(2),
        exchangeRate: null, intraEuKind: null,
        downloadedAt: NOW, createdAt: NOW,
      } as unknown as ReceivedInvoice);
    }
  });
  return rows.reverse();
}

const invoices = buildInvoices();
const received = buildReceived();

const notifications: Notification[] = [
  { id: "demo-n1", notificationType: "INVOICE_VALIDATED", title: "FACT-2026-0042 trimisă către SPV", body: "acum 2 ore · acceptată", data: null, isRead: false, readAt: null, osNotificationShown: true, createdAt: NOW - 2 * 3600 },
  { id: "demo-n2", notificationType: "ANAF_MESSAGE", title: "Mesaj nou de la ANAF", body: "acum 5 ore · de citit", data: null, isRead: false, readAt: null, osNotificationShown: true, createdAt: NOW - 5 * 3600 },
  { id: "demo-n3", notificationType: "RECEIVED_IMPORTED", title: "3 facturi primite importate", body: "ieri · de la furnizori", data: null, isRead: false, readAt: null, osNotificationShown: true, createdAt: NOW - 26 * 3600 },
  { id: "demo-n4", notificationType: "SYNC_OK", title: "Sincronizare reușită", body: "ieri, 18:30 · 7 documente", data: null, isRead: true, readAt: NOW, osNotificationShown: true, createdAt: NOW - 30 * 3600 },
];

const license: License = {
  id: 1, licenseKey: null, tier: "SOLO" as License["tier"],
  activatedAt: NOW - 90 * 86400, expiresAt: NOW + 275 * 86400,
  machineId: "demo", email: "andrei@consulting.ro",
  lastValidatedAt: NOW, isExpired: false, trialDaysRemaining: null,
};

// ── Command router ────────────────────────────────────────────────────────────

function paginate<T>(rows: T[], args?: Record<string, unknown>): Paginated<T> {
  const page = (args?.filter as { page?: { offset: number; limit: number } } | undefined)?.page;
  const offset = page?.offset ?? 0;
  const limit = page?.limit ?? rows.length;
  return { items: rows.slice(offset, offset + limit), total: rows.length, offset, limit };
}

const ok = { level: "ok", ytdRon: "0", pct: 0 };

/** ?demo=1&fresh=1 → simulate a first run (no companies, no license) so the
 *  onboarding wizard renders in the browser harness. */
const isFresh = () =>
  typeof window !== "undefined" && new URLSearchParams(window.location.search).has("fresh");

const HANDLERS: Record<string, (args?: Record<string, unknown>) => unknown> = {
  list_companies: () => (isFresh() ? [] : [company]),
  get_company: () => company,
  list_contacts: () => contacts,
  get_contact: (a) => contacts.find((c) => c.id === a?.id) ?? contacts[0],
  get_invoice: (a) => {
    const inv = invoices.find((i) => i.id === a?.id) ?? invoices[0];
    return {
      invoice: inv,
      events: [],
      payments: [],
      lines: [
        { id: "demo-l1", invoiceId: (inv as { id: string }).id, position: 1, name: "Servicii consultanță", description: null, quantity: "10.00", unit: "H87", unitPrice: "100.00", vatRate: "21.00", vatCategory: "S", subtotalAmount: "1000.00", vatAmount: "210.00", totalAmount: "1210.00", cpvCode: null, art331Code: null, revenueKind: "goods" },
        { id: "demo-l2", invoiceId: (inv as { id: string }).id, position: 2, name: "Materiale tipărite", description: null, quantity: "5.00", unit: "H87", unitPrice: "40.00", vatRate: "11.00", vatCategory: "S", subtotalAmount: "200.00", vatAmount: "22.00", totalAmount: "222.00", cpvCode: null, art331Code: null, revenueKind: "goods" },
      ],
    };
  },
  list_invoices: (a) => paginate(invoices, a),
  list_received_invoices: (a) => paginate(received, a),
  list_notifications: () => notifications,
  unread_notification_count: () => 3,
  fetch_bnr_rate: () => 5.0985,
  // PDF generation: in the harness there is no filesystem — useOpenPdf detects
  // demo mode and fetches /sample-invoice.pdf, so the returned path is unused.
  generate_invoice_pdf: () => "/demo/invoice.pdf",
  generate_receipt_pdf: () => "/demo/receipt.pdf",
  preview_invoice_template: () => "/demo/template-preview.pdf",
  mark_notification_read: () => null,
  mark_all_notifications_read: () => null,
  get_license: () => (isFresh() ? null : license),
  check_license_validity: () => true,
  anaf_is_authenticated: () => true,
  get_setting: () => null,
  set_setting: () => null,
  check_form_versions: () => [],
  tax_regime_status: () => ({ level: "ok", ytdTurnoverRon: "248.310", ceilingRon: "509.850", pct: 49, note: null, cashVatLevel: "ok", cashVatPlafonRon: "5.000.000", cashVatNote: null }),
  vat_registration_status: () => ({ applicable: false, level: "ok", ytdTurnoverRon: "0", plafonRon: "395.000", pct: 0 }),
  intrastat_status: () => ({ dispatches: ok, arrivals: ok, thresholdRon: "1.000.000" }),
  list_payments: () => [],
  list_payment_summaries: () => [],
  list_receipts: () => [],
  list_products: () => [],
  list_recurring_invoices: () => [],
  list_employees: () => [],
  // Two dividends — one resident (→ D205), one non-resident (→ D207, flagged in the UI).
  list_dividends: () => [
    { id: "dv1", companyId: "demo-co", distributionDate: "2025-03-15", paymentDate: "2025-03-20", grossAmount: "10000.00", taxRate: 10, taxAmount: "1000.00", netAmount: "9000.00", interim2025: false, shareholder: "Andrei Popescu", beneficiaryCnp: "1900101410011", beneficiaryResident: true, note: null, taxDeadline: "2025-04-25" },
    { id: "dv2", companyId: "demo-co", distributionDate: "2025-06-10", paymentDate: "2025-06-15", grossAmount: "8000.00", taxRate: 10, taxAmount: "800.00", netAmount: "7200.00", interim2025: false, shareholder: "John Smith", beneficiaryCnp: null, beneficiaryResident: false, note: null, taxDeadline: "2025-07-25" },
  ],
  list_assets: () => [],
  list_vat_rates: () => [],
  list_accounts: () => [],
  stock_valuation_ledger: () => [],
  // GL / contabilitate — minimal valid object shapes (page expects .rows/.entries etc.)
  journal_register: () => ({ rows: [], totalDebit: "0", totalCredit: "0", balanced: true }),
  trial_balance: () => ({ rows: [], totalOpeningDebit: "0", totalOpeningCredit: "0", totalPeriodDebit: "0", totalPeriodCredit: "0", totalTotalDebit: "0", totalTotalCredit: "0", totalClosingDebit: "0", totalClosingCredit: "0" }),
  profit_and_loss: () => ({ periodFrom: "2026-06-01", periodTo: "2026-06-30", taxRegime: "micro", revenueLines: [], expenseLines: [], operatingRevenue: "0", operatingExpense: "0", grossResult: "0", incomeTax: "0", netResult: "0" }),
  ledger_accounts: () => [],
  list_certificates: () => [],
  get_archive_size: () => 0,
  verify_archive_integrity: () => ({ checked: 0, missing: [], missingUnderRetention: [] }),
  // D100 (micro) demo row + a sample dividend obligation due in the quarter (informational — D100 has
  // no XML/DUK; surfaced so the user is reminded to declare it). Quarter/year come from the view.
  compute_d100: (a) => {
    const q = Math.min(4, Math.max(1, Number(a?.quarter) || 2));
    const year = Number(a?.year) || 2026;
    const pad = (n: number) => String(n).padStart(2, "0");
    const scaMonth = q * 3 + 1;
    const scadenta = scaMonth > 12 ? `25.01.${year + 1}` : `25.${pad(scaMonth)}.${year}`;
    const divMonth = (q - 1) * 3 + 2; // a month inside the quarter
    return {
      applicable: true,
      note: null,
      codOblig: "5",
      label: "Impozit pe veniturile microîntreprinderilor (1%)",
      base: "248310",
      ratePct: "1",
      sumaDatorata: "2483",
      priorPayments: "0",
      sumaDePlata: "2483",
      scadenta,
      dividendObligations: [
        {
          label:
            "Impozit pe dividende distribuite/plătite persoanelor fizice/juridice (art. 97/43 C.fisc.)",
          amount: "3200.00",
          deadline: `25.${pad(divMonth)}.${year}`,
          count: 2,
        },
      ],
    };
  },
};

export function demoInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const h = HANDLERS[cmd];
  if (h) return Promise.resolve(h(args) as T);
  // Unported/unfixtured command — succeed with an empty-ish value so demo pages render.
  return Promise.resolve([] as unknown as T);
}
