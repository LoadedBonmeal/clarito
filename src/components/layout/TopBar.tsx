/**
 * TopBar — slim top bar with breadcrumb, search, SPV pill, +Nou menu,
 * notification bell, and profile menu.
 *
 * Wiring:
 *  - Search button → setCommandOpen(true)
 *  - SPV pill "Sincronizează" → api.anaf.syncSpv (harvested from Ribbon.tsx handleSyncSpv)
 *  - "+Nou" menu items → navigate to routes
 *  - Bell → /notifications
 *  - Profile: Ieșire → exit(0), Documentație → openUrl(...), Comută tema → setTheme
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtShortcut } from "@/lib/platform";

// ── Breadcrumb route map ──────────────────────────────────────────────────────

const ROUTE_META: Record<string, string[]> = {
  "/": ["Tablou de bord", "Privire generală"],
  "/invoices": ["e-Factura", "Facturi emise"],
  "/invoices/new": ["e-Factura", "Facturi emise", "Factură nouă"],
  "/received": ["e-Factura", "Facturi primite"],
  "/notifications": ["e-Factura", "Mesaje SPV"],
  "/companies": ["Operativ", "Companii"],
  "/companies/new": ["Operativ", "Companii", "Companie nouă"],
  "/contacts": ["Operativ", "Clienți & Furnizori"],
  "/receipts": ["Operativ", "Chitanțe"],
  "/payments": ["Operativ", "Urmărire Plăți"],
  "/recurring": ["Operativ", "Facturi Recurente"],
  "/products": ["Operativ", "Articole & Stocuri"],
  "/accounts": ["Date", "Plan de conturi"],
  "/vat-rates": ["Date", "Cote TVA"],
  "/reports": ["Raportare", "Rapoarte"],
  "/declarations": ["Raportare", "Declarații ANAF"],
  "/settings": ["Setări"],
};

function routeTrail(pathname: string): string[] {
  // Exact match first
  if (ROUTE_META[pathname]) return ROUTE_META[pathname];
  // Prefix match for dynamic routes (e.g. /invoices/abc123)
  if (pathname.startsWith("/invoices/")) return ["e-Factura", "Facturi emise", "Detaliu factură"];
  if (pathname.startsWith("/companies/")) return ["Operativ", "Companii", "Detaliu companie"];
  if (pathname.startsWith("/contacts/")) return ["Operativ", "Clienți & Furnizori", "Detaliu contact"];
  if (pathname.startsWith("/received/")) return ["e-Factura", "Facturi primite", "Detaliu"];
  return ["RoFactura"];
}

// ── +Nou menu items ───────────────────────────────────────────────────────────

interface NewMenuItem {
  id: string;
  label: string;
  icon: string;
  hint?: string;
  to: string;
  search?: Record<string, string>;
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
  const location = useLocation();
  const queryClient = useQueryClient();

  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const sidebarCollapsed = useAppStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);

  const [newOpen, setNewOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);

  const newRef = useRef<HTMLDivElement>(null);
  const profileRef = useRef<HTMLDivElement>(null);

  // Close dropdowns on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (newRef.current && !newRef.current.contains(e.target as Node)) setNewOpen(false);
      if (profileRef.current && !profileRef.current.contains(e.target as Node)) setProfileOpen(false);
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

  // Breadcrumb
  const crumbs = routeTrail(location.pathname);

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

  const handleExit = async () => {
    const { exit } = await import("@tauri-apps/plugin-process");
    await exit(0);
  };

  const handleDocs = async () => {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl("https://mfinante.gov.ro/ro/web/efactura/informatii-tehnice");
  };

  const handleToggleTheme = () => {
    setTheme(theme === "dark" ? "light" : "dark");
  };

  const anafOk = !activeCompanyId || isAnafAuth;
  const pillClass = `rf-spv-pill ${anafOk ? "rf-spv-pill--ok" : "rf-spv-pill--err"}`;

  return (
    <div className="rf-topbar">
      {/* Sidebar collapse toggle */}
      <button
        type="button"
        className="rf-topbar-icon"
        onClick={toggleSidebar}
        title={sidebarCollapsed ? "Extinde bara laterală" : "Restrânge bara laterală"}
        style={{ flexShrink: 0 }}
      >
        <Icon name={sidebarCollapsed ? "chevRight" : "chevLeft"} size={16} />
      </button>

      {/* Breadcrumb */}
      <div className="rf-breadcrumb">
        <span className="rf-crumb">RoFactura</span>
        {crumbs.map((c, i) => (
          <span key={i} style={{ display: "contents" }}>
            <Icon name="chevRight" size={13} style={{ flexShrink: 0 }} />
            <span className={`rf-crumb${i === crumbs.length - 1 ? " current" : ""}`}>{c}</span>
          </span>
        ))}
      </div>

      <div className="rf-spacer" />

      {/* Global search button */}
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

      {/* Profile menu */}
      <div ref={profileRef} style={{ position: "relative" }}>
        <button
          type="button"
          className="rf-topbar-icon"
          title="Profil și setări"
          onClick={() => setProfileOpen((o) => !o)}
        >
          <Icon name="user" size={18} />
        </button>
        {profileOpen && (
          <div
            className="rf-profile-menu"
            style={{
              position: "absolute",
              top: "calc(100% + 8px)",
              right: 0,
              zIndex: 60,
              width: 220,
              padding: 6,
              background: "var(--rf-content)",
              border: "1px solid var(--rf-border)",
              borderRadius: "var(--rf-radius)",
              boxShadow: "var(--rf-shadow-md)",
            }}
          >
            <button
              type="button"
              onClick={() => { setProfileOpen(false); void navigate({ to: "/settings" }); }}
              style={profileMenuItemStyle}
            >
              <Icon name="settings" size={15} />
              <span>Setări</span>
            </button>
            <button
              type="button"
              onClick={() => { setProfileOpen(false); handleToggleTheme(); }}
              style={profileMenuItemStyle}
            >
              <Icon name="view" size={15} />
              <span>Comută tema</span>
              <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--rf-text-dim)" }}>
                {theme === "dark" ? "Luminoasă" : "Întunecată"}
              </span>
            </button>
            <button
              type="button"
              onClick={() => { setProfileOpen(false); void handleDocs(); }}
              style={profileMenuItemStyle}
            >
              <Icon name="help" size={15} />
              <span>Documentație e-Factura</span>
            </button>
            <div style={{ height: 1, background: "var(--rf-border)", margin: "4px 0" }} />
            <button
              type="button"
              onClick={() => void handleExit()}
              style={{ ...profileMenuItemStyle, color: "var(--rf-error)" }}
            >
              <Icon name="x" size={15} />
              <span>Ieșire</span>
              <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--rf-text-dim)" }}>
                {fmtShortcut("Alt+F4")}
              </span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

const profileMenuItemStyle: React.CSSProperties = {
  display: "flex",
  width: "100%",
  gap: 10,
  alignItems: "center",
  border: "none",
  background: "transparent",
  padding: "9px 11px",
  cursor: "pointer",
  borderRadius: "var(--rf-radius-sm)",
  fontSize: 13.5,
  color: "var(--rf-text)",
  textAlign: "left",
  fontFamily: "inherit",
};
