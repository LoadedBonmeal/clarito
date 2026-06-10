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
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";

import { TopBar } from "./TopBar";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { CommandPalette } from "./CommandPalette";
import { OnboardingGate } from "@/components/onboarding/OnboardingGate";
import { Banner } from "@/components/rf";
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
        className="rf-switcher"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        {/* Header */}
        <div className="rf-switcher-head">
          COMPANIE ACTIVĂ
        </div>

        {/* Search input */}
        <div className="rf-switcher-search">
          <Icon name="search" size={13} style={{ color: "var(--rf-text-dim)", flexShrink: 0 }} />
          <input
            ref={inputRef}
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Caută companie sau CUI…"
          />
        </div>

        {/* Company list with max-height + scroll */}
        <div ref={listRef} className="rf-switcher-list">
          {filtered.map((c, idx) => (
            <button
              key={c.id}
              type="button"
              data-idx={idx}
              onClick={() => onSelect(c.id)}
              className={`rf-switcher-item${idx === cursor ? " highlighted" : ""}`}
              onMouseEnter={() => setCursor(idx)}
            >
              <div className="rf-switcher-item-body">
                <div className="rf-switcher-item-name">{c.legalName}</div>
                <div className="rf-switcher-item-cui">{c.cui}</div>
              </div>
              {c.id === activeCompanyId && (
                <Icon name="check" size={13} className="rf-switcher-check" />
              )}
            </button>
          ))}
          {filtered.length === 0 && (
            <div className="rf-switcher-empty">
              {companies.length === 0 ? "Nicio companie configurată." : "Nicio companie găsită."}
            </div>
          )}
        </div>

        {/* Footer hint */}
        <div className="rf-switcher-foot">
          <span><kbd>↑↓</kbd> navighează</span>
          <span><kbd>Enter</kbd> selectează</span>
          <span><kbd>Esc</kbd> închide</span>
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
  const location = useLocation();
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

  // ANAF form-version staleness — checked once at launch, graceful on network failure
  const [stalenessDismissed, setStalenessDismissed] = useState(false);
  const { data: stalenessForms = [] } = useQuery({
    queryKey: ["anaf", "formVersions"],
    queryFn: () => api.system.checkFormVersions().catch(() => []),
    staleTime: 60 * 60 * 1000, // 1 hour
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const activeCompanyName = activeCompany?.legalName ?? "—";

  return (
    <OnboardingGate>
      <div className="app">
        <TopBar />
        <Sidebar onOpenCompanySwitcher={() => setSwitcherOpen(true)} />
        <div className="app-main">
          {stalenessForms.length > 0 && !stalenessDismissed && (
            <Banner
              variant="warning"
              actions={
                <button
                  type="button"
                  onClick={() => setStalenessDismissed(true)}
                  style={{
                    background: "none",
                    border: "none",
                    cursor: "pointer",
                    padding: "0 4px",
                    lineHeight: 1,
                    color: "var(--rf-text-dim)",
                  }}
                  aria-label="Închide notificare"
                >
                  ×
                </button>
              }
            >
              Formular ANAF actualizat ({stalenessForms.map((f) => f.form).join(", ")}) — actualizați aplicația pentru validare corectă.
            </Banner>
          )}
          <div className="rf-content">
            <div className="rf-page" key={location.pathname}>{children}</div>
          </div>
          <StatusBar
            activeCompanyName={activeCompanyName}
            activeCompanyId={activeCompanyId ?? undefined}
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
