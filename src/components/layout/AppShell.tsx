/**
 * AppShell — layout principal Win32:
 *   ┌─────────────────────────────────┐
 *   │ MenuBar (26px)                  │
 *   │ Ribbon (86px)                   │
 *   ├──────┬──────────────────────────┤
 *   │ Side │  Content (Outlet)        │
 *   │ bar  │                          │
 *   ├──────┴──────────────────────────┤
 *   │ Status bar (22px)               │
 *   └─────────────────────────────────┘
 */

import { useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";

import { MenuBar } from "./MenuBar";
import { Ribbon } from "./Ribbon";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { CommandPalette } from "./CommandPalette";
import { OnboardingGate } from "@/components/onboarding/OnboardingGate";
import { Icon } from "@/components/shared/Icon";
import { useTheme } from "@/hooks/use-theme";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps) {
  useTheme();
  const navigate = useNavigate();
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const queryClient = useQueryClient();

  // Tray navigation events
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<string>("tray_navigate", (event) => {
      void navigate({ to: event.payload as "/" });
    }).then(fn => { unlisten = fn; });
    return () => unlisten?.();
  }, [navigate]);

  // Backend reactive events — invalidate queries so UI updates without polling
  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    // new_notification: refresh notification badge + list
    void listen("new_notification", () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    }).then(fn => unlisteners.push(fn));

    // invoice_status_changed: refresh invoice list + detail
    void listen<{ invoiceId: string; newStatus: string }>("invoice_status_changed", ({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.list() });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(payload.invoiceId) });
    }).then(fn => unlisteners.push(fn));

    // sync_completed: refresh settings (last_sync_at) + received invoices
    void listen("sync_completed", () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get("last_sync_at") });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.list() });
    }).then(fn => unlisteners.push(fn));

    // oauth_completed: refresh company auth state
    void listen<{ companyId: string; success: boolean }>("oauth_completed", ({ payload }) => {
      if (payload.success) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
        void queryClient.invalidateQueries({ queryKey: queryKeys.companies.detail(payload.companyId) });
      }
    }).then(fn => unlisteners.push(fn));

    return () => unlisteners.forEach(fn => fn());
  }, [queryClient]);

  // Global keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        e.preventDefault();
        setCommandOpen(true);
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "n") {
        e.preventDefault();
        void navigate({ to: "/invoices/new" });
      }
      if (e.key === "F5") {
        e.preventDefault();
        void queryClient.refetchQueries({ type: "active" });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [setCommandOpen, navigate, queryClient]);

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const activeCompanyName = activeCompany?.legalName ?? "—";

  return (
    <OnboardingGate>
      <div className="app">
        <MenuBar
          activeCompanyName={activeCompanyName}
          activeCompanyCui={activeCompany?.cui}
          onOpenCompanySwitcher={() => setSwitcherOpen(true)}
        />
        <Ribbon onOpenPalette={() => setCommandOpen(true)} />
        <div className="workspace">
          <Sidebar />
          <div className="content-shell">{children}</div>
        </div>
        <StatusBar
          activeCompanyName={activeCompanyName}
          activeCompanyId={activeCompanyId ?? undefined}
          companyCount={companies.length}
        />
        <CommandPalette />
        {switcherOpen && (
          <div
            className="palette-scrim"
            onClick={() => setSwitcherOpen(false)}
            style={{ alignItems: "flex-start", paddingTop: 54 }}
          >
            <div
              onClick={(e) => e.stopPropagation()}
              style={{
                background: "var(--bg-content)",
                border: "1px solid var(--border)",
                minWidth: 280,
                maxHeight: 320,
                overflow: "auto",
                boxShadow: "0 4px 16px rgba(0,0,0,0.12)",
              }}
            >
              <div style={{ padding: "8px 12px", fontSize: 10.5, color: "var(--text-muted)", fontWeight: 600, letterSpacing: "0.06em", borderBottom: "1px solid var(--border-soft)" }}>
                COMPANIE ACTIVĂ
              </div>
              {companies.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => {
                    setActiveCompanyId(c.id);
                    setSwitcherOpen(false);
                  }}
                  style={{
                    display: "flex",
                    width: "100%",
                    alignItems: "center",
                    gap: 10,
                    padding: "8px 12px",
                    background: c.id === activeCompanyId ? "var(--accent-soft)" : "none",
                    border: "none",
                    borderBottom: "1px solid var(--border-soft)",
                    cursor: "pointer",
                    textAlign: "left",
                    fontSize: 11.5,
                  }}
                >
                  <div style={{ flex: 1 }}>
                    <div style={{ fontWeight: 600, color: "var(--text)" }}>{c.legalName}</div>
                    <div style={{ fontSize: 10, color: "var(--text-muted)" }}>{c.cui}</div>
                  </div>
                  {c.id === activeCompanyId && (
                    <Icon name="check" size={12} style={{ color: "var(--accent)" }} />
                  )}
                </button>
              ))}
              {companies.length === 0 && (
                <div style={{ padding: "12px", fontSize: 11, color: "var(--text-muted)" }}>
                  Nicio companie configurată.
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </OnboardingGate>
  );
}
