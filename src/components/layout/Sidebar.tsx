/**
 * Sidebar — white grouped navigation (rf- prefixed classes).
 *
 * Groups: TABLOU DE BORD / E-FACTURA / OPERATIV / RAPORTARE.
 * Top: logo + company-card → opens CompanySwitcher in AppShell.
 * Footer: Setări link, Ajutor (openUrl), collapse toggle.
 * Preserves badge queries from original Sidebar.tsx.
 */

import { Link, useLocation } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

// ── Nav data ──────────────────────────────────────────────────────────────────

interface NavItem {
  id: string;
  label: string;
  icon: string;
  path: string;
  matchPrefix?: string;
  badgeAccent?: boolean;
  disabled?: boolean;
  badge?: number;
}

interface NavGroup {
  group: string;
  items: NavItem[];
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

interface SidebarProps {
  onOpenCompanySwitcher: () => void;
}

export function Sidebar({ onOpenCompanySwitcher }: SidebarProps) {
  const location = useLocation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const sidebarCollapsed = useAppStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);

  // ── Badge queries (same as original) ──────────────────────────────────────

  const { data: invoicesPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () =>
      api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 1 } }),
    enabled: !!activeCompanyId,
  });

  const { data: receivedPaged } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () =>
      api.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 1 } }),
    enabled: !!activeCompanyId,
  });

  const { data: unreadCount } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const invoicesBadge = invoicesPaged?.total;
  const receivedBadge = receivedPaged?.total;
  const spvBadge = unreadCount ?? undefined;
  const companiesBadge = companies.length > 0 ? companies.length : undefined;
  const contactsBadge = contacts.length > 0 ? contacts.length : undefined;

  // ── Nav groups ─────────────────────────────────────────────────────────────

  const NAV_GROUPS: NavGroup[] = [
    {
      group: "TABLOU DE BORD",
      items: [
        { id: "dashboard", label: "Privire generală", icon: "data", path: "/" },
      ],
    },
    {
      group: "E-FACTURA",
      items: [
        { id: "facturi-emise",   label: "Facturi emise",   icon: "invoice",   path: "/invoices",      matchPrefix: "/invoices",      badge: invoicesBadge },
        { id: "facturi-primite", label: "Facturi primite", icon: "invoiceIn", path: "/received",      matchPrefix: "/received",      badge: receivedBadge },
        { id: "mesaje-spv",      label: "Mesaje SPV",      icon: "anaf",      path: "/notifications", badge: spvBadge, badgeAccent: true },
        { id: "stornate",        label: "Stornate",        icon: "storno",    path: "/invoices" },
      ],
    },
    {
      group: "OPERATIV",
      items: [
        { id: "companii",        label: "Companii",            icon: "buildings", path: "/companies",  matchPrefix: "/companies", badge: companiesBadge },
        { id: "contacte",        label: "Clienți & Furnizori", icon: "users",     path: "/contacts",   badge: contactsBadge },
        { id: "chitante",        label: "Chitanțe",             icon: "receipt",   path: "/receipts" },
        { id: "plati",           label: "Urmărire Plăți",       icon: "bank",      path: "/payments" },
        { id: "recurente",       label: "Facturi Recurente",    icon: "refresh",   path: "/recurring" },
        { id: "stocuri",         label: "Articole & Stocuri",  icon: "stock",     path: "/products" },
        { id: "plan-conturi",    label: "Plan de conturi",      icon: "database",  path: "/accounts" },
        { id: "cote-tva",        label: "Cote TVA",             icon: "tag",       path: "/vat-rates" },
      ],
    },
    {
      group: "RAPORTARE",
      items: [
        { id: "rapoarte",    label: "Rapoarte",         icon: "reports", path: "/reports" },
        { id: "declaratii",  label: "Declarații ANAF",  icon: "anaf",    path: "/declarations" },
      ],
    },
  ];

  // ── Active company display ─────────────────────────────────────────────────

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const companyInitials = (activeCompany?.legalName ?? "RF").slice(0, 2).toUpperCase();

  return (
    <nav className={`rf-sidebar${sidebarCollapsed ? " collapsed" : ""}`}>
      {/* Brand wordmark */}
      <div className="rf-brand-wordmark">
        <span className="rf-logo">
          <Icon name="logo" size={17} stroke={2} />
        </span>
        <span className="rf-name">RoFactura</span>
      </div>

      {/* Company card — opens switcher modal in AppShell */}
      <button
        type="button"
        className="rf-company-card"
        onClick={onOpenCompanySwitcher}
        title="Schimbă compania activă"
      >
        {/* Avatar (shown when collapsed) */}
        <span
          style={{
            display: sidebarCollapsed ? "grid" : "none",
            width: 32, height: 32, borderRadius: 8,
            background: "var(--rf-accent-tint)", color: "var(--rf-accent)",
            placeItems: "center", fontSize: 12, fontWeight: 700, flexShrink: 0,
          }}
        >
          {companyInitials}
        </span>
        {/* Full card (shown when expanded) */}
        {!sidebarCollapsed && (
          <>
            <div className="rf-cc-label">Companie</div>
            <div className="rf-cc-name">
              <span>{activeCompany?.legalName ?? "Nicio companie"}</span>
              <Icon name="chevDown" size={13} style={{ color: "var(--rf-text-muted)", flexShrink: 0 }} />
            </div>
            {activeCompany?.cui && (
              <div className="rf-cc-cui">{activeCompany.cui}</div>
            )}
          </>
        )}
      </button>

      {/* Nav scroll area */}
      <div className="rf-nav-scroll">
        {NAV_GROUPS.map((grp) => (
          <div key={grp.group}>
            <div className="rf-nav-group-label">{grp.group}</div>
            {grp.items.map((item) => {
              const isActive = item.matchPrefix
                ? location.pathname === item.matchPrefix ||
                  location.pathname.startsWith(`${item.matchPrefix}/`)
                : location.pathname === item.path;

              if (item.disabled) {
                return (
                  <div
                    key={item.id}
                    className="rf-nav-item"
                    style={{ opacity: 0.4, cursor: "not-allowed", pointerEvents: "none" }}
                    title="În curând"
                  >
                    <span className="rf-nav-ic"><Icon name={item.icon} size={18} /></span>
                    <span className="rf-nav-label">{item.label}</span>
                  </div>
                );
              }

              return (
                <Link
                  key={item.id}
                  to={item.path as "/"}
                  className={`rf-nav-item${isActive ? " active" : ""}`}
                  title={sidebarCollapsed ? item.label : undefined}
                >
                  <span className="rf-nav-ic"><Icon name={item.icon} size={18} /></span>
                  <span className="rf-nav-label">{item.label}</span>
                  {item.badge != null && (
                    <span className={`rf-nav-badge${item.badgeAccent ? " accent" : ""}`}>
                      {item.badge}
                    </span>
                  )}
                </Link>
              );
            })}
          </div>
        ))}
      </div>

      {/* Footer */}
      <div className="rf-sidebar-foot">
        <Link
          to="/settings"
          className={`rf-nav-item${location.pathname === "/settings" ? " active" : ""}`}
          title={sidebarCollapsed ? "Setări" : undefined}
        >
          <span className="rf-nav-ic"><Icon name="settings" size={18} /></span>
          <span className="rf-nav-label">Setări</span>
        </Link>

        <button
          type="button"
          className="rf-nav-item"
          title="Documentație e-Factura"
          onClick={() => {
            void import("@tauri-apps/plugin-opener").then((m) =>
              m.openUrl("https://mfinante.gov.ro/ro/web/efactura/informatii-tehnice")
            );
          }}
        >
          <span className="rf-nav-ic"><Icon name="help" size={18} /></span>
          <span className="rf-nav-label">Ajutor</span>
        </button>

        <button
          type="button"
          className="rf-nav-item"
          onClick={toggleSidebar}
          title={sidebarCollapsed ? "Extinde" : "Restrânge"}
        >
          <span className="rf-nav-ic">
            <Icon name={sidebarCollapsed ? "chevRight" : "chevLeft"} size={18} />
          </span>
          <span className="rf-nav-label">Restrânge</span>
        </button>
      </div>
    </nav>
  );
}
