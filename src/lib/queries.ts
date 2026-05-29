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
