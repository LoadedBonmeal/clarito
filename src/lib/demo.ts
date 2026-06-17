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
  // Two dividends — one resident (→ D205), one non-resident (→ D207, with country + foreign NIF).
  list_dividends: () => [
    { id: "dv1", companyId: "demo-co", distributionDate: "2025-03-15", paymentDate: "2025-03-20", grossAmount: "10000.00", taxRate: 10, taxAmount: "1000.00", netAmount: "9000.00", interim2025: false, shareholder: "Andrei Popescu", beneficiaryCnp: "1900101410011", beneficiaryResident: true, beneficiaryType: "PF", beneficiaryCountry: null, beneficiaryForeignTaxId: null, note: null, taxDeadline: "2025-04-25" },
    { id: "dv2", companyId: "demo-co", distributionDate: "2025-06-10", paymentDate: "2025-06-15", grossAmount: "8000.00", taxRate: 10, taxAmount: "800.00", netAmount: "7200.00", interim2025: false, shareholder: "John Smith", beneficiaryCnp: null, beneficiaryResident: false, beneficiaryType: "PF", beneficiaryCountry: "GB", beneficiaryForeignTaxId: "GB123456789", note: null, taxDeadline: "2025-07-25" },
  ],
  // DIV-01: in-place beneficiary edit — echo the update onto a dividend-shaped object (demo is mock).
  update_dividend_beneficiary: (a) => {
    const u = (a?.update ?? {}) as Record<string, unknown>;
    return { id: u.id ?? "dv1", companyId: "demo-co", distributionDate: "2025-03-15", paymentDate: u.paymentDate ?? null, grossAmount: "10000.00", taxRate: 10, taxAmount: "1000.00", netAmount: "9000.00", interim2025: false, shareholder: u.shareholder ?? null, beneficiaryCnp: u.beneficiaryCnp ?? null, beneficiaryResident: u.beneficiaryResident ?? true, beneficiaryType: u.beneficiaryType ?? "PF", beneficiaryCountry: u.beneficiaryCountry ?? null, beneficiaryForeignTaxId: u.beneficiaryForeignTaxId ?? null, note: u.note ?? null, taxDeadline: "2025-04-25" };
  },
  // D207 (non-resident dividends) — demo mock.
  preview_d207_xml: () => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<declaratie207 xmlns=\"mfp:anaf:dgti:d207:declaratie:v2\" luna=\"12\" an=\"2025\" d_rec=\"0\" cui=\"12345678\" den=\"Demo SRL\" adresa=\"București\" nume_declar=\"Demo\" prenume_declar=\"-\" functie_declar=\"Administrator\" totalPlata_A=\"8801\">\n  <sect_II tip_venit=\"01\" nrben=\"1\" Tscutit=\"0\" Tbaza=\"8000\" Timp=\"800\" Timps=\"0\"/>\n  <benef id_inreg=\"1\" tip_venit1=\"01\" den1=\"John Smith\" Stat_R=\"GB\" cifS=\"GB123456789\" baza1=\"8000\" imp1=\"800\" imps1=\"0\" Act_N=\"1\"/>\n</declaratie207>",
  export_d207_official: () => ({ path: "d207-2025.xml", written: true, dukAvailable: false, dukPassed: false, issues: [] }),
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
          codOblig: "604",
          label: "Impozit pe veniturile din dividende distribuite persoanelor fizice (art. 97 C.fisc.)",
          amount: "3200.00",
          deadline: `25.${pad(divMonth)}.${year}`,
          count: 2,
        },
      ],
    };
  },
  // D205 XML preview for the in-app XML viewer/editor (demo harness has no Rust emitter). Mirrors the
  // real validator-verified schema: <declaratie205> header attrs → self-closing <sect_II> → <benef> siblings.
  preview_d205_xml: (a) => {
    const year = Number(a?.year) || 2025;
    return [
      '<?xml version="1.0" encoding="UTF-8"?>',
      `<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="${year}" d_rec="0"` +
        ' cui="40268319" adresa="Str. Victoriei 10, București, B" den="DEMO Tehnologii SRL"' +
        ' nume_declar="DEMO Tehnologii SRL" prenume_declar="-" functie_declar="Administrator"' +
        ' totalPlata_A="6401">',
      '  <sect_II tip_venit="08" nrben="1" Tcastig="0" Tpierd="0" T_VB="0" T_GAR="0" Tbaza="40000" Timp="6400"/>',
      '  <benef id_inreg="1" tip_venit1="08" tip_plata="2" Rezid="1" cifR="1960101410014"' +
        ' den1="Popescu Andrei" baza1="40000" imp1="6400" divid_D="40000" divid_P="40000"/>',
      "</declaratie205>",
    ].join("\n");
  },
  // Re-validate stub — the demo harness has no Java/DUK runtime; report the happy path so the editor's
  // "re-validate with DUK" flow can be exercised in ?demo=1.
  validate_declaration_xml: () => ({ available: true, passed: true, issues: [] }),
  // Preview stubs for the XML viewer on every export page (demo harness has no Rust emitters). Each
  // returns a short, representative sample so the viewer/editor + DUK re-validate can be exercised.
  preview_d300_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<declaratie300 xmlns="mfp:anaf:dgti:d300:declaratie:v12" luna="6" an="2026" d_rec="0" cui="40268319" totalPlata_A="1234">',
      '  <rand_cod_300 cod="1" baza="6500" tva="1235"/>',
      '  <rand_cod_300 cod="20" baza="4000" tva="760"/>',
      "</declaratie300>",
    ].join("\n"),
  compute_d394: () => ({
    companyCui: "RO40268319",
    periodFrom: "2026-06-01",
    periodTo: "2026-06-30",
    partners: [
      { partnerCui: "RO12345674", partnerName: "Client Demo SRL", vatCategory: "S", vatRate: "21", invoiceCount: 4, base: "20000.00", vat: "4200.00" },
      { partnerCui: "RO98765438", partnerName: "Beta Distribuție SRL", vatCategory: "S", vatRate: "11", invoiceCount: 2, base: "5000.00", vat: "550.00" },
    ],
    totalBase: "25000.00",
    totalVat: "4750.00",
    invoiceCount: 6,
    purchasePartners: [
      { partnerCui: "RO11111110", partnerName: "Furnizor Demo SRL", vatCategory: "S", vatRate: "21", invoiceCount: 3, base: "8000.00", vat: "1680.00" },
    ],
    totalPurchaseBase: "8000.00",
    totalPurchaseVat: "1680.00",
    purchaseInvoiceCount: 3,
    purchaseUnparsedCount: 0,
  }),
  preview_d394_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<declaratie394 xmlns="mfp:anaf:dgti:d394:declaratie:v5" luna="6" an="2026" cui="40268319">',
      '  <facturi>',
      '    <rezumatD>nrFacturi="5" baza="6500" tva="1235"</rezumatD>',
      "  </facturi>",
      "</declaratie394>",
    ].join("\n"),
  preview_saft_official_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<AuditFile xmlns="mfp:anaf:dgti:d406:declaratie:v1">',
      "  <Header>",
      "    <AuditFileVersion>2.4.9</AuditFileVersion>",
      "    <AuditFileCountry>RO</AuditFileCountry>",
      "    <AuditFileDateCreated>2026-07-01</AuditFileDateCreated>",
      "    <SoftwareID>efactura-desktop</SoftwareID><SoftwareVersion>0.7.0</SoftwareVersion>",
      "    <Company>",
      "      <RegistrationNumber>40268319</RegistrationNumber><Name>DEMO Tehnologii SRL</Name>",
      "      <Address><StreetName>Str. Victoriei 10</StreetName><City>București</City></Address>",
      "      <TaxRegistration><TaxRegistrationNumber>40268319</TaxRegistrationNumber></TaxRegistration>",
      "    </Company>",
      "    <DefaultCurrencyCode>RON</DefaultCurrencyCode>",
      "    <SelectionCriteria><SelectionStartDate>2026-06-01</SelectionStartDate><SelectionEndDate>2026-06-30</SelectionEndDate></SelectionCriteria>",
      "    <HeaderComment>L</HeaderComment><TaxAccountingBasis>A</TaxAccountingBasis>",
      "  </Header>",
      "  <MasterFiles>",
      "    <GeneralLedgerAccounts><Account/><Account/><Account/></GeneralLedgerAccounts>",
      "    <Customers><Customer/><Customer/></Customers><Suppliers><Supplier/></Suppliers>",
      "    <TaxTable><TaxTableEntry/><TaxTableEntry/></TaxTable><Products><Product/></Products>",
      "  </MasterFiles>",
      "  <GeneralLedgerEntries><NumberOfEntries>42</NumberOfEntries><TotalDebit>125000.00</TotalDebit><TotalCredit>125000.00</TotalCredit><Journal/></GeneralLedgerEntries>",
      "  <SourceDocuments>",
      "    <SalesInvoices><NumberOfEntries>8</NumberOfEntries><TotalDebit>59500.00</TotalDebit><TotalCredit>50000.00</TotalCredit><Invoice/></SalesInvoices>",
      "    <PurchaseInvoices><NumberOfEntries>5</NumberOfEntries><TotalDebit>21000.00</TotalDebit><TotalCredit>24990.00</TotalCredit><Invoice/></PurchaseInvoices>",
      "    <Payments><NumberOfEntries>11</NumberOfEntries><TotalDebit>74490.00</TotalDebit><TotalCredit>74490.00</TotalCredit><Payment/></Payments>",
      "  </SourceDocuments>",
      "</AuditFile>",
    ].join("\n"),
  preview_d112_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<declaratie113 xmlns="mfp:anaf:dgti:declaratieUnica:v7" luna="6" an="2026">',
      '  <declaratie113 cui="40268319" caen="6201">',
      '    <angajati nrAsig="3" totalVenitBruto="26700"/>',
      "  </declaratie113>",
      "</declaratie113>",
    ].join("\n"),
  preview_d390_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<declaratie390 xmlns="mfp:anaf:dgti:d390:declaratie:v3" luna="6" an="2026" cui="40268319">',
      '  <operatiune tip="L" tara="DE" cod="123456789" denumire="Partner GmbH" baza="5000"/>',
      "</declaratie390>",
    ].join("\n"),
  preview_bilant_xml: () =>
    [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<bilant xmlns="mfp:anaf:dgti:bilant:declaratie:v1" an="2025" cui="40268319" forma="UU">',
      "  <f10><rand cod=\"F10_350\" valoare=\"125000\"/></f10>",
      "  <f20><rand cod=\"F20_42\" valoare=\"38000\"/></f20>",
      "</bilant>",
    ].join("\n"),
};

export function demoInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const h = HANDLERS[cmd];
  if (h) return Promise.resolve(h(args) as T);
  // Unported/unfixtured command — succeed with an empty-ish value so demo pages render.
  return Promise.resolve([] as unknown as T);
}
