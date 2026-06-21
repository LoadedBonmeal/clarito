/**
 * Configurația TanStack Router pentru Tauri (memory history).
 *
 * Folosim memory history pentru că aplicația rulează într-un WebView fără
 * URL bar real. Beneficii: back/forward funcționează în interior, dar nu
 * persistă între restart-uri (ceea ce e dorit pentru un desktop app).
 */

import {
  createRootRoute,
  createRoute,
  createRouter,
  createMemoryHistory,
  Outlet,
} from "@tanstack/react-router";

// ─── Report view search param type ───────────────────────────────────────────

export type ReportView =
  | "tva"
  | "etva"
  | "d390"
  | "d394"
  | "d101"
  | "d100"
  | "salariu"
  | "saft"
  | "sales-journal"
  | "purchase-journal"
  | "accounting-export"
  | "aging";

const REPORT_VIEWS: ReportView[] = [
  "tva",
  "etva",
  "d390",
  "d394",
  "d101",
  "d100",
  "salariu",
  "saft",
  "sales-journal",
  "purchase-journal",
  "accounting-export",
  "aging",
];

import { AppShell } from "@/components/layout/AppShell";
import { DashboardPage } from "@/pages/Dashboard";
import { CompaniesPage } from "@/pages/Companies";
import { CompanyDetailPage } from "@/pages/CompanyDetail";
import { CompanyNewPage } from "@/pages/CompanyNew";
import { CompanyEditPage } from "@/pages/CompanyEdit";
import { InvoicesPage } from "@/pages/Invoices";
import { InvoiceDetailPage } from "@/pages/InvoiceDetail";
import { InvoiceNewPage } from "@/pages/InvoiceNew";
import { InvoiceEditPage } from "@/pages/InvoiceEdit";
import { ReceivedPage } from "@/pages/Received";
import { ReceivedDetailPage } from "@/pages/ReceivedDetail";
import { NotificationsPage } from "@/pages/Notifications";
import { ContactsPage } from "@/pages/Contacts";
import { PayrollPage } from "@/pages/Payroll";
import { AssetsPage } from "@/pages/Assets";
import { Dividends } from "@/pages/Dividends";
import { ReportsPage } from "@/pages/Reports";
import { EtransportPage } from "@/pages/Etransport";
import { SettingsPage } from "@/pages/Settings";
import { DocumentsPage } from "@/pages/Documents";
import { HelpPage } from "@/pages/Help";
import { AccountPage } from "@/pages/Account";
import { PaymentsPage } from "@/pages/Payments";
import { RecurringPage } from "@/pages/Recurring";
import { ChartOfAccountsPage } from "@/pages/ChartOfAccounts";
import { ProductsPage } from "@/pages/Products";
import { ReceiptsPage } from "@/pages/Receipts";
import { VatRatesPage } from "@/pages/VatRates";
import { DeclarationsPage } from "@/pages/Declarations";
import { GlLedgerPage } from "@/pages/GlLedger";
import { BankPage } from "@/pages/Bank";
import { BankImportPage } from "@/pages/BankImport";
import { StornatePage } from "@/pages/Stornate";
import { GestiuniPage } from "@/pages/Gestiuni";
import { InventoryPage } from "@/pages/Inventory";
import { InventoryRegisterPage } from "@/pages/InventoryRegister";
import { NirPage } from "@/pages/Nir";
import { StockTransferPage } from "@/pages/StockTransfer";

// ─── Layout root ──────────────────────────────────────────────────────────

const rootRoute = createRootRoute({
  component: () => (
    <AppShell>
      <Outlet />
    </AppShell>
  ),
});

// ─── Index → Dashboard ────────────────────────────────────────────────────

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: DashboardPage,
});

// ─── Companies ────────────────────────────────────────────────────────────

const companiesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/companies",
  component: CompaniesPage,
});

const companyNewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/companies/new",
  component: CompanyNewPage,
});

const companyDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/companies/$id",
  component: CompanyDetailPage,
});

const companyEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/companies/$id/edit",
  component: CompanyEditPage,
});

// ─── Restul paginilor (placeholders încă) ─────────────────────────────────

type InvoiceView = "storned";

const invoicesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/invoices",
  component: InvoicesPage,
  validateSearch: (search: Record<string, unknown>): { view?: InvoiceView } => ({
    view: search.view === "storned" ? "storned" : undefined,
  }),
});

const invoiceDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/invoices/$id",
  component: InvoiceDetailPage,
});

const invoiceNewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/invoices/new",
  component: InvoiceNewPage,
});

const invoiceEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/invoices/$id/edit",
  component: InvoiceEditPage,
});

const receivedRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/received",
  component: ReceivedPage,
});

const receivedDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/received/$id",
  component: ReceivedDetailPage,
});

const notificationsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/notifications",
  component: NotificationsPage,
});

const etransportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/etransport",
  component: EtransportPage,
});

const contactsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/contacts",
  component: ContactsPage,
});

const payrollRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/payroll",
  component: PayrollPage,
});

const assetsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/assets",
  component: AssetsPage,
});

const dividendsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/dividends",
  component: Dividends,
});

const reportsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/reports",
  component: ReportsPage,
  validateSearch: (search: Record<string, unknown>): { view?: ReportView } => ({
    view:
      typeof search.view === "string" &&
      REPORT_VIEWS.includes(search.view as ReportView)
        ? (search.view as ReportView)
        : undefined,
  }),
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

const documentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/documents",
  component: DocumentsPage,
});

const helpRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/help",
  component: HelpPage,
});

const accountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/account",
  component: AccountPage,
});

const paymentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/payments",
  component: PaymentsPage,
});

const recurringRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/recurring",
  component: RecurringPage,
});

const productsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/products",
  component: ProductsPage,
});

const declarationsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/declarations",
  component: DeclarationsPage,
});

const vatRatesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/vat-rates",
  component: VatRatesPage,
});

const receiptsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/receipts",
  component: ReceiptsPage,
});

const accountsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts",
  component: ChartOfAccountsPage,
});

const bankRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/bank",
  component: BankPage,
});

const bankImportRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/bank-import",
  component: BankImportPage,
});

const stornateRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/stornate",
  component: StornatePage,
});

const glLedgerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/ledger",
  component: GlLedgerPage,
});

const gestiuniRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/gestiuni",
  component: GestiuniPage,
});

const inventoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory",
  component: InventoryPage,
});

const inventoryRegisterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory-register",
  component: InventoryRegisterPage,
});

const nirRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/nir",
  component: NirPage,
});

const stockTransferRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/stock-transfer",
  component: StockTransferPage,
});

// ─── Build tree + router ──────────────────────────────────────────────────

const routeTree = rootRoute.addChildren([
  indexRoute,
  companiesRoute,
  companyNewRoute,
  companyDetailRoute,
  companyEditRoute,
  invoicesRoute,
  invoiceNewRoute,
  invoiceEditRoute,
  invoiceDetailRoute,
  receivedRoute,
  etransportRoute,
  receivedDetailRoute,
  notificationsRoute,
  contactsRoute,
  payrollRoute,
  assetsRoute,
  dividendsRoute,
  productsRoute,
  receiptsRoute,
  vatRatesRoute,
  accountsRoute,
  reportsRoute,
  settingsRoute,
  documentsRoute,
  helpRoute,
  accountRoute,
  paymentsRoute,
  recurringRoute,
  declarationsRoute,
  glLedgerRoute,
  bankRoute,
  bankImportRoute,
  stornateRoute,
  gestiuniRoute,
  inventoryRoute,
  inventoryRegisterRoute,
  nirRoute,
  stockTransferRoute,
]);

export const router = createRouter({
  routeTree,
  history: createMemoryHistory({ initialEntries: ["/"] }),
  defaultPreload: "intent",
  defaultPreloadStaleTime: 0,
});

// ─── Type augmentation (autocompletare Cmd+Click peste rute) ──────────────

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
