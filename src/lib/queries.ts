/**
 * TanStack Query client + chei standardizate.
 *
 * Convenție pentru chei: `[entitate, operație, ...args]`.
 * Centralizat aici ca să evităm string-uri scattered prin pagini.
 */

import { QueryClient } from "@tanstack/react-query";

import type { ContactFilter, InvoiceFilter, ReceivedFilter } from "@/types";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 5 * 60_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
    mutations: {
      retry: 0,
    },
  },
});

export const queryKeys = {
  companies: {
    all: ["companies"] as const,
    list: () => [...queryKeys.companies.all, "list"] as const,
    detail: (id: string) => [...queryKeys.companies.all, "detail", id] as const,
  },
  contacts: {
    all: ["contacts"] as const,
    list: (filter?: ContactFilter) =>
      [...queryKeys.contacts.all, "list", filter] as const,
    detail: (id: string) => [...queryKeys.contacts.all, "detail", id] as const,
  },
  invoices: {
    all: ["invoices"] as const,
    list: (filter?: InvoiceFilter) =>
      [...queryKeys.invoices.all, "list", filter] as const,
    detail: (id: string) => [...queryKeys.invoices.all, "detail", id] as const,
  },
  received: {
    all: ["received"] as const,
    list: (filter?: ReceivedFilter) =>
      [...queryKeys.received.all, "list", filter] as const,
    detail: (id: string) => [...queryKeys.received.all, "detail", id] as const,
  },
  notifications: {
    all: ["notifications"] as const,
    list: (onlyUnread: boolean) =>
      [...queryKeys.notifications.all, "list", onlyUnread] as const,
    unreadCount: () => [...queryKeys.notifications.all, "unreadCount"] as const,
  },
  license: ["license"] as const,
  licenseValidity: ["license", "validity"] as const,
  licenseExisting: ["license", "existing"] as const,

  certificates: {
    list: (companyId: string) => ["certificates", companyId] as const,
  },

  invoiceValidation: {
    get: (id: string) => ["validation", id] as const,
  },

  payments: {
    summary: (invoiceId: string, companyId: string) =>
      ["payments", "summary", invoiceId, companyId] as const,
    summaries: (companyId: string) =>
      ["payment_summaries", companyId] as const,
  },

  products: {
    all: ["products"] as const,
    list: (companyId: string, query?: string) =>
      [...(["products"] as const), "list", companyId, query] as const,
    detail: (id: string) => [...(["products"] as const), "detail", id] as const,
  },

  recurring: {
    list: (companyId: string) => ["recurringInvoices", companyId] as const,
  },

  vatReport: {
    get: (year: number, month: number | string, companyId: string) =>
      ["vatReport", year, month, companyId] as const,
  },

  vatRates: {
    all: ["vatRates"] as const,
    list: (activeOnly?: boolean) =>
      [...(["vatRates"] as const), "list", activeOnly] as const,
    detail: (id: string) => [...(["vatRates"] as const), "detail", id] as const,
  },

  receipts: {
    all: ["receipts"] as const,
    list: (companyId: string) =>
      [...(["receipts"] as const), "list", companyId] as const,
    detail: (id: string) => [...(["receipts"] as const), "detail", id] as const,
  },

  accounts: {
    all: ["accounts"] as const,
    list: (companyId: string) =>
      [...(["accounts"] as const), "list", companyId] as const,
    detail: (id: string) => [...(["accounts"] as const), "detail", id] as const,
  },

  appInfo: ["appInfo"] as const,
  settings: {
    get: (key: string) => ["settings", key] as const,
    all: ["settings"] as const,
  },
  anaf: {
    auth: (companyId: string) => ["anaf", "auth", companyId] as const,
    testMode: ["settings", "use_anaf_test_env"] as const,
  },
  system: {
    archiveSize: ["archive-size"] as const,
    autostart: ["autostart"] as const,
    activityLog: ["activity-log"] as const,
    licenseStatus: ["license-status"] as const,
  },
} as const;
