/**
 * Sidebar — listă plată de module cu color-bar pe item-ul activ.
 *
 * Portat din Claude Design (chrome.jsx). Folosește clase din design.css
 * (.sidebar, .sidebar-section, .sidebar-item.active, .bar, .badge, etc.).
 */

import { Link, useLocation } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtShortcut } from "@/lib/platform";

interface NavItem {
  id: string;
  label: string;
  ico: string;
  color: string;
  badge?: number;
  path: string;
  matchPrefix?: string;
  /** Pagina nu este implementată încă — dezactivează click-ul și estompează itemul */
  disabled?: boolean;
}

interface NavSection {
  section: string;
}

type Item = NavItem | NavSection;

export function Sidebar() {
  const location = useLocation();
  const openPalette = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const { data: invoicesPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 1 } }),
    enabled: !!activeCompanyId,
  });

  const { data: receivedPaged } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 1 } }),
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

  const MODULES: Item[] = [
    { section: "Tablou de bord" },
    { id: "dashboard", label: "Privire generală", ico: "data", color: "#2848A1", path: "/" },
    { section: "e-Factura" },
    { id: "facturi-emise",   label: "Facturi emise",   ico: "invoice",   color: "var(--color-facturi)", badge: invoicesBadge, path: "/invoices",    matchPrefix: "/invoices" },
    { id: "facturi-primite", label: "Facturi primite", ico: "invoiceIn", color: "var(--color-primite)", badge: receivedBadge, path: "/received",    matchPrefix: "/received" },
    { id: "spv",             label: "Mesaje SPV",      ico: "anaf",      color: "var(--color-primite)", badge: spvBadge,      path: "/notifications" },
    // "Stornate" navigates to /invoices — the correct route for storned invoices.
    // The Invoices page manages tab state internally; clicking here lands on the
    // full invoice list from which the Stornate tab is one click away.
    { id: "stornate",        label: "Stornate",        ico: "storno",    color: "var(--color-rapoarte)", path: "/invoices" },
    { section: "Operativ" },
    { id: "companii",  label: "Companii",           ico: "buildings", color: "var(--color-companii)", badge: companiesBadge, path: "/companies", matchPrefix: "/companies" },
    { id: "contacte",  label: "Contacte",           ico: "users",     color: "var(--color-contacte)", badge: contactsBadge,  path: "/contacts" },
    { id: "plati",     label: "Urmărire Plăți",     ico: "receipt",   color: "var(--color-banca)",    path: "/payments" },
    { id: "recurente", label: "Facturi Recurente",  ico: "refresh",   color: "var(--color-facturi)",  path: "/recurring" },
    // Stocuri and Bancă have no page — hidden until implemented.
    // { id: "stocuri",   label: "Articole & Stocuri", ico: "stock",  color: "var(--color-stocuri)",  path: "/contacts",  disabled: true },
    // { id: "banca",     label: "Bancă & Casă",       ico: "bank",   color: "var(--color-banca)",    path: "/contacts",  disabled: true },
    { section: "Raportare" },
    { id: "rapoarte",   label: "Rapoarte",          ico: "reports", color: "var(--color-rapoarte)", path: "/reports" },
    // Audit has no dedicated page yet — shown disabled (planned feature).
    { id: "declaratii", label: "Declarații ANAF",   ico: "anaf",    color: "var(--color-rapoarte)", path: "/declarations" },
    { id: "audit",      label: "Jurnal modificări", ico: "history", color: "#8A857A",               path: "/settings",  disabled: true },
  ];

  const activeCompanyName =
    companies.find((c) => c.id === activeCompanyId)?.legalName ?? "RoFactura";

  return (
    <div className="sidebar">
      {MODULES.map((m, i) => {
        if ("section" in m) {
          return (
            <div key={"s" + i} className="sidebar-section">
              {m.section}
            </div>
          );
        }

        if (m.disabled) {
          return (
            <div
              key={m.id}
              className="sidebar-item"
              style={{
                ["--module-color" as string]: m.color,
                opacity: 0.4,
                cursor: "not-allowed",
                pointerEvents: "none",
              }}
              title="În curând"
            >
              <span className="bar" />
              <span className="ico">
                <Icon name={m.ico} size={15} />
              </span>
              <span>{m.label}</span>
            </div>
          );
        }

        const isActive = m.matchPrefix
          ? location.pathname === m.matchPrefix ||
            location.pathname.startsWith(`${m.matchPrefix}/`)
          : location.pathname === m.path;

        return (
          <Link
            key={m.id}
            to={m.path}
            className={"sidebar-item" + (isActive ? " active" : "")}
            style={{ ["--module-color" as string]: m.color }}
          >
            <span className="bar" />
            <span className="ico">
              <Icon name={m.ico} size={15} />
            </span>
            <span>{m.label}</span>
            {m.badge != null && <span className="badge">{m.badge}</span>}
          </Link>
        );
      })}
      <button
        type="button"
        className="sidebar-footer"
        onClick={() => openPalette(true)}
      >
        <Icon name="user" size={14} />
        <span style={{ flex: 1, textAlign: "left", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {activeCompanyName}
        </span>
        <span className="kbd">{fmtShortcut("Ctrl+K")}</span>
      </button>
    </div>
  );
}
