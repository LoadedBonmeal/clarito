/**
 * AppShell — design grid (clarito-shell.css): topbar spans the top, sidebar
 * below-left, page content in <main class="main">. The company switcher + profile
 * menu are inline pops in the Sidebar; no separate StatusBar (the design has none).
 *
 * PRESERVED: CommandPalette, ShortcutsDialog, OnboardingGate, the 4 Tauri event
 * listeners (new_notification, invoice_status_changed, sync_completed,
 * oauth_completed) + tray_navigate, the global shortcuts (Ctrl+K/N, F5, Ctrl+/),
 * useTheme().
 */

import { useEffect, useState, type ReactNode } from "react";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";

import { TopBar } from "./TopBar";
import { Sidebar } from "./Sidebar";
import { CommandPalette } from "./CommandPalette";
import { OnboardingGate } from "@/components/onboarding/OnboardingGate";
import { Banner } from "@/components/shared/Banner";
import { ShortcutsDialog } from "@/components/shared/ShortcutsDialog";
import { useTheme } from "@/hooks/use-theme";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps) {
  useTheme();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const sidebarCollapsed = useAppStore((s) => s.sidebarCollapsed);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const queryClient = useQueryClient();

  // Tray navigation
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<string>("tray_navigate", (event) => {
      void navigate({ to: event.payload as "/" });
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, [navigate]);

  // Backend reactive events → invalidate queries
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void listen("new_notification", () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    }).then((fn) => unlisteners.push(fn));
    void listen<{ invoiceId: string; newStatus: string }>("invoice_status_changed", ({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.list() });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(payload.invoiceId) });
    }).then((fn) => unlisteners.push(fn));
    void listen("sync_completed", () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get("last_sync_at") });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.list() });
    }).then((fn) => unlisteners.push(fn));
    void listen<{ companyId: string; success: boolean }>("oauth_completed", ({ payload }) => {
      if (payload.success) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
        void queryClient.invalidateQueries({ queryKey: queryKeys.companies.detail(payload.companyId) });
      }
    }).then((fn) => unlisteners.push(fn));
    return () => unlisteners.forEach((fn) => fn());
  }, [queryClient]);

  // Global keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") { e.preventDefault(); setCommandOpen(true); }
      if ((e.ctrlKey || e.metaKey) && e.key === "n") { e.preventDefault(); void navigate({ to: "/invoices/new" }); }
      if (e.key === "F5") { e.preventDefault(); void queryClient.refetchQueries({ type: "active" }); }
      if ((e.ctrlKey || e.metaKey) && e.key === "/") { e.preventDefault(); setShortcutsOpen(true); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [setCommandOpen, navigate, queryClient]);

  // ANAF form-version staleness banner
  const [stalenessDismissed, setStalenessDismissed] = useState(false);
  const { data: stalenessForms = [] } = useQuery({
    queryKey: ["anaf", "formVersions"],
    queryFn: () => api.system.checkFormVersions().catch(() => []),
    staleTime: 60 * 60 * 1000,
  });

  return (
    <OnboardingGate>
      <div className={`app${sidebarCollapsed ? " sidebar-collapsed" : ""}`}>
        <TopBar />
        <Sidebar />
        <main className="main">
          {stalenessForms.length > 0 && !stalenessDismissed && (
            <div style={{ padding: "12px 32px 0" }}>
              <Banner
                variant="warning"
                actions={
                  <button
                    type="button"
                    onClick={() => setStalenessDismissed(true)}
                    style={{ background: "none", border: "none", cursor: "pointer", padding: "0 4px", lineHeight: 1, color: "var(--rf-text-dim)" }}
                    aria-label={t("shell.banner.close")}
                  >×</button>
                }
              >
                {t("shell.banner.formStale", { forms: stalenessForms.map((f) => f.form).join(", ") })}
              </Banner>
            </div>
          )}
          <div className="cl-in" key={location.pathname}>{children}</div>
        </main>
        <CommandPalette />
        <ShortcutsDialog open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
      </div>
    </OnboardingGate>
  );
}
