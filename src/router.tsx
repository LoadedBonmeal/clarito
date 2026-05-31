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
  | "d394"
  | "saft"
  | "sales-journal"
  | "purchase-journal"
  | "accounting-export";

const REPORT_VIEWS: ReportView[] = [
  "tva",
  "d394",
  "saft",
  "sales-journal",
  "purchase-journal",
  "accounting-export",
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
import { ReportsPage } from "@/pages/Reports";
import { SettingsPage } from "@/pages/Settings";
import { PaymentsPage } from "@/pages/Payments";
import { RecurringPage } from "@/pages/Recurring";
import { ProductsPage } from "@/pages/Products";
import { ReceiptsPage } from "@/pages/Receipts";
import { VatRatesPage } from "@/pages/VatRates";
import { DeclarationsPage } from "@/pages/Declarations";

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

const invoicesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/invoices",
  component: InvoicesPage,
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

const contactsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/contacts",
  component: ContactsPage,
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
  receivedDetailRoute,
  notificationsRoute,
  contactsRoute,
  productsRoute,
  receiptsRoute,
  vatRatesRoute,
  reportsRoute,
  settingsRoute,
  paymentsRoute,
  recurringRoute,
  declarationsRoute,
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
