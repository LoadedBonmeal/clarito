/**
 * TopBar — full-width top bar (design clarito-shell.css):
 *   brand (logo + wordmark) + sidebar toggle  ·  centered global search  ·
 *   ANAF·SPV pill  ·  +Nou menu  ·  notification bell.
 * The profile/user menu lives in the Sidebar foot (per the design).
 *
 * Wiring preserved:
 *  - Search → setCommandOpen(true)
 *  - SPV pill "Sincronizează" → api.anaf.syncSpv
 *  - "+Nou" menu items → navigate to routes
 *  - Bell → /notifications
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { BrandMark } from "@/components/shared/BrandMark";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtShortcut } from "@/lib/platform";

// ── +Nou menu items ───────────────────────────────────────────────────────────

interface NewMenuItem {
  id: string;
  label: string;
  icon: string;
  hint?: string;
  to: string;
}

const NEW_MENU_ITEMS: NewMenuItem[] = [
  { id: "new-invoice",  label: "Factură nouă",    icon: "fileOut",  hint: fmtShortcut("Ctrl+N"), to: "/invoices/new" },
  { id: "received",     label: "Factură primită", icon: "fileIn",   to: "/received" },
  { id: "receipts",     label: "Chitanță",        icon: "receipt",  to: "/receipts" },
  { id: "payments",     label: "Plată",           icon: "bank",     to: "/payments" },
  { id: "contacts",     label: "Contact nou",     icon: "users",    to: "/contacts" },
  { id: "products",     label: "Articol nou",     icon: "stock",    to: "/products" },
  { id: "companies-new",label: "Companie nouă",   icon: "buildings",to: "/companies/new" },
];

// ── TopBar component ──────────────────────────────────────────────────────────

export function TopBar() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const sidebarCollapsed = useAppStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);

  const [newOpen, setNewOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const newRef = useRef<HTMLDivElement>(null);

  // Close +Nou dropdown on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (newRef.current && !newRef.current.contains(e.target as Node)) setNewOpen(false);
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  // ANAF test mode
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const anafTestMode = testModeSetting === "1";

  // ANAF auth status (for pill color)
  const { data: isAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  // Notification unread count for bell badge
  const { data: unreadCount } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
    refetchInterval: 60_000,
  });

  // ── Handlers ──────────────────────────────────────────────────────────────

  // Harvested verbatim from Ribbon.tsx handleSyncSpv
  const handleSyncSpv = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (syncing) return;
    setSyncing(true);
    try {
      const newCount = await api.anaf.syncSpv(activeCompanyId, anafTestMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      if (newCount > 0) {
        notify.success(`${newCount} mesaje SPV noi descărcate`);
      } else {
        notify.info("Nicio factură nouă în SPV");
      }
      void navigate({ to: "/received" });
    } catch (e) {
      notify.error(formatError(e, "Sincronizarea SPV a eșuat."));
    } finally {
      setSyncing(false);
    }
  };

  const anafOk = !activeCompanyId || isAnafAuth;
  const pillClass = `rf-spv-pill ${anafOk ? "rf-spv-pill--ok" : "rf-spv-pill--err"}`;

  return (
    <div className="rf-topbar">
      {/* Brand + sidebar toggle (sits above the sidebar column) */}
      <div className="rf-topbar-brand">
        <BrandMark size={26} className="rf-logo-img" />
        <span className="rf-name">Clarito</span>
        <button
          type="button"
          className="rf-topbar-collapse"
          onClick={toggleSidebar}
          title={sidebarCollapsed ? "Extinde bara laterală" : "Restrânge bara laterală"}
        >
          <Icon name={sidebarCollapsed ? "chevRight" : "chevLeft"} size={16} />
        </button>
      </div>

      {/* Centered global search */}
      <div className="rf-searchwrap">
        <button
          type="button"
          className="rf-topbar-cmd"
          onClick={() => setCommandOpen(true)}
          title={fmtShortcut("Ctrl+K")}
        >
          <Icon name="search" size={16} style={{ flexShrink: 0 }} />
          <span>Caută facturi, clienți, articole…</span>
          <span className="mono" style={{ flexShrink: 0, fontSize: 11, border: "1px solid var(--rf-border)", borderRadius: 5, padding: "2px 6px", background: "var(--rf-content)" }}>
            {fmtShortcut("Ctrl+K")}
          </span>
        </button>
      </div>

      {/* Right cluster */}
      <div className="rf-topright">
        {/* ANAF · SPV pill */}
        <div className={pillClass}>
          <span className="rf-spv-status">
            ANAF · SPV: <b>{anafOk ? "Conectat" : "Neautentificat"}</b>
          </span>
          <span className="rf-spv-div" />
          <button
            type="button"
            className={`rf-spv-sync${syncing ? " is-syncing" : ""}`}
            onClick={() => void handleSyncSpv()}
            disabled={syncing}
          >
            <Icon name="refresh" size={15} />
            {syncing ? "Se sincronizează…" : "Sincronizează"}
          </button>
        </div>

        {/* +Nou menu */}
        <div ref={newRef} style={{ position: "relative" }}>
          <button
            type="button"
            className="rf-btn rf-btn--primary rf-btn--sm"
            onClick={() => setNewOpen((o) => !o)}
          >
            <Icon name="plus" size={15} />
            Nou
            <Icon name="chevDown" size={14} style={{ marginLeft: -2, opacity: 0.8 }} />
          </button>
          {newOpen && (
            <div className="rf-new-menu">
              {NEW_MENU_ITEMS.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => {
                    setNewOpen(false);
                    void navigate({ to: item.to as "/" });
                  }}
                >
                  <span className="rf-menu-ic">
                    <Icon name={item.icon} size={16} />
                  </span>
                  <span style={{ flex: 1 }}>{item.label}</span>
                  {item.hint && (
                    <span className="mono" style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>
                      {item.hint}
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Notifications bell */}
        <button
          type="button"
          className="rf-topbar-icon"
          title="Notificări"
          onClick={() => void navigate({ to: "/notifications" })}
        >
          <Icon name="bell" size={18} />
          {unreadCount != null && unreadCount > 0 && (
            <span className="rf-badge-dot" />
          )}
        </button>
      </div>
    </div>
  );
}
