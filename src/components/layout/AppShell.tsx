/**
 * AppShell — layout principal modern:
 *   ┌──────────────────────────────────────┐
 *   │ Sidebar (white, grouped)             │
 *   ├──────────────────────────────────────┤
 *   │ TopBar (breadcrumb + actions)        │
 *   │ Content (Outlet)                     │
 *   │ StatusBar                            │
 *   └──────────────────────────────────────┘
 *
 * PRESERVED verbatim:
 *  - CompanySwitcher modal (inline)
 *  - CommandPalette
 *  - ShortcutsDialog
 *  - OnboardingGate
 *  - 4 Tauri event listeners (new_notification, invoice_status_changed,
 *    sync_completed, oauth_completed) + tray_navigate
 *  - Global keyboard shortcuts (Ctrl+K, Ctrl+N, F5, Ctrl+/)
 *  - companies query + activeCompany resolution
 *  - useTheme()
 */

import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";

import { TopBar } from "./TopBar";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { CommandPalette } from "./CommandPalette";
import { OnboardingGate } from "@/components/onboarding/OnboardingGate";
import { Icon } from "@/components/shared/Icon";
import { ShortcutsDialog } from "@/components/shared/ShortcutsDialog";
import { useTheme } from "@/hooks/use-theme";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { Company } from "@/types";

// ─── Company Switcher ─────────────────────────────────────────────────────

interface CompanySwitcherProps {
  companies: Company[];
  activeCompanyId: string | null;
  onSelect: (id: string) => void;
  onClose: () => void;
}

function CompanySwitcher({ companies, activeCompanyId, onSelect, onClose }: CompanySwitcherProps) {
  const [search, setSearch] = useState("");
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const filtered = companies.filter((c) => {
    const q = search.toLowerCase();
    return (
      c.legalName.toLowerCase().includes(q) ||
      (c.cui ?? "").toLowerCase().includes(q)
    );
  });

  // Auto-focus search input when opened
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Reset cursor when filtered list changes
  useEffect(() => {
    setCursor(0);
  }, [search]);

  // Scroll cursor item into view
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${cursor}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(c + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const selected = filtered[cursor];
      if (selected) onSelect(selected.id);
    }
  };

  return (
    <div
      className="palette-scrim"
      onClick={onClose}
      style={{ alignItems: "flex-start", paddingTop: 54 }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Selectare companie activă"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
        style={{
          background: "var(--bg-content)",
          border: "1px solid var(--border)",
          minWidth: 300,
          maxWidth: 420,
          boxShadow: "0 4px 16px rgba(0,0,0,0.12)",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {/* Header */}
        <div style={{ padding: "8px 12px", fontSize: 10.5, color: "var(--text-muted)", fontWeight: 600, letterSpacing: "0.06em", borderBottom: "1px solid var(--border-soft)" }}>
          COMPANIE ACTIVĂ
        </div>

        {/* Search input */}
        <div style={{ padding: "6px 10px", borderBottom: "1px solid var(--border-soft)", display: "flex", alignItems: "center", gap: 6 }}>
          <Icon name="search" size={12} />
          <input
            ref={inputRef}
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Caută companie sau CUI…"
            style={{
              flex: 1,
              border: "none",
              outline: "none",
              background: "transparent",
              fontSize: 11.5,
              color: "var(--text)",
            }}
          />
        </div>

        {/* Company list with max-height + scroll */}
        <div
          ref={listRef}
          style={{
            maxHeight: 280,
            overflowY: "auto",
            overflowX: "hidden",
          }}
        >
          {filtered.map((c, idx) => (
            <button
              key={c.id}
              type="button"
              data-idx={idx}
              onClick={() => onSelect(c.id)}
              style={{
                display: "flex",
                width: "100%",
                alignItems: "center",
                gap: 10,
                padding: "8px 12px",
                background:
                  idx === cursor
                    ? "var(--accent-soft)"
                    : c.id === activeCompanyId
                    ? "var(--bg-hover)"
                    : "none",
                border: "none",
                borderBottom: "1px solid var(--border-soft)",
                cursor: "pointer",
                textAlign: "left",
                fontSize: 11.5,
              }}
              onMouseEnter={() => setCursor(idx)}
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
          {filtered.length === 0 && (
            <div style={{ padding: "12px", fontSize: 11, color: "var(--text-muted)", textAlign: "center" }}>
              {companies.length === 0 ? "Nicio companie configurată." : "Nicio companie găsită."}
            </div>
          )}
        </div>

        {/* Footer hint */}
        <div style={{ padding: "5px 12px", fontSize: 10, color: "var(--text-muted)", borderTop: "1px solid var(--border-soft)", display: "flex", gap: 12 }}>
          <span><kbd style={{ fontFamily: "var(--font-mono)" }}>↑↓</kbd> navighează</span>
          <span><kbd style={{ fontFamily: "var(--font-mono)" }}>Enter</kbd> selectează</span>
          <span><kbd style={{ fontFamily: "var(--font-mono)" }}>Esc</kbd> închide</span>
        </div>
      </div>
    </div>
  );
}

// ─── AppShell ─────────────────────────────────────────────────────────────────

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
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
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
      if ((e.ctrlKey || e.metaKey) && e.key === "/") {
        e.preventDefault();
        setShortcutsOpen(true);
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
        <Sidebar onOpenCompanySwitcher={() => setSwitcherOpen(true)} />
        <div className="app-main">
          <TopBar />
          <div className="rf-content">
            {children}
          </div>
          <StatusBar
            activeCompanyName={activeCompanyName}
            activeCompanyId={activeCompanyId ?? undefined}
            companyCount={companies.length}
          />
        </div>
        <CommandPalette />
        <ShortcutsDialog open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
        {switcherOpen && (
          <CompanySwitcher
            companies={companies}
            activeCompanyId={activeCompanyId}
            onSelect={(id) => {
              setActiveCompanyId(id);
              setSwitcherOpen(false);
            }}
            onClose={() => setSwitcherOpen(false)}
          />
        )}
      </div>
    </OnboardingGate>
  );
}
