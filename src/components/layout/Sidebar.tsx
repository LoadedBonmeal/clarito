/**
 * Sidebar — verbatim port of the design `.sidebar` (clarito-shell.js), wired to
 * real data: company card + switcher pop, grouped nav (with the "Mai multe"
 * drawer), and the user/account card + profile pop. The pops are direct children
 * of <aside class="sidebar"> so the design's absolute anchoring (#companyPop
 * top:54px, #profilePop bottom:70px) lands correctly.
 */

import { useEffect, useState } from "react";
import { Link, useLocation, useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

interface NavLink {
  key: string;
  label: string;
  icon: string;
  path: string;
  matchPrefix?: string;
  badge?: number;
  more?: boolean;
}
interface NavGroup {
  sec: string | null;
  items: NavLink[];
}

const stop = (e: React.SyntheticEvent) => e.stopPropagation();

export function Sidebar() {
  const location = useLocation();
  const navigate = useNavigate();
  const { i18n } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

  const [companyOpen, setCompanyOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState<Record<string, boolean>>({});

  // Any outside mousedown closes the pops (triggers/pops call stopPropagation).
  useEffect(() => {
    const h = () => { setCompanyOpen(false); setProfileOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);

  // ── Real data ──────────────────────────────────────────────────────────────
  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const { data: unreadCount } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
  });
  const { data: license } = useQuery({
    queryKey: ["license", "current"],
    queryFn: () => api.license.get(),
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];
  const initials = (s: string | undefined, n = 2) =>
    (s ?? "AC").replace(/[^A-Za-zĂÂÎȘȚ ]/g, "").split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, n).toUpperCase() || "AC";

  // ── Nav model (design NAV → real routes) ───────────────────────────────────
  const NAV: NavGroup[] = [
    { sec: null, items: [
      { key: "privire", label: "Privire generală", icon: "grid", path: "/" },
      { key: "facturi-emise", label: "Facturi emise", icon: "docUp", path: "/invoices", matchPrefix: "/invoices" },
      { key: "facturi-primite", label: "Facturi primite", icon: "docDown", path: "/received", matchPrefix: "/received" },
      { key: "mesaje-spv", label: "Mesaje SPV", icon: "mail", path: "/notifications", badge: unreadCount || undefined },
      { key: "stornate", label: "Stornate", icon: "undo", path: "/stornate" },
    ]},
    { sec: "Operare", items: [
      { key: "clienti", label: "Clienți & Furnizori", icon: "users", path: "/contacts" },
      { key: "chitante", label: "Chitanțe", icon: "receipt", path: "/receipts" },
      { key: "urmarire-plati", label: "Urmărire plăți", icon: "card", path: "/payments" },
      { key: "salarizare", label: "Salarizare", icon: "idcard", path: "/payroll" },
      { key: "articole", label: "Articole & stocuri", icon: "cube", path: "/products" },
      { key: "companii", label: "Companii", icon: "building", path: "/companies", matchPrefix: "/companies", more: true },
      { key: "facturi-recurente", label: "Facturi recurente", icon: "loop", path: "/recurring", more: true },
      { key: "mijloace-fixe", label: "Mijloace fixe", icon: "wrench", path: "/assets", more: true },
      { key: "plan-conturi", label: "Plan de conturi", icon: "book", path: "/accounts", more: true },
      { key: "cote-tva", label: "Cote TVA", icon: "scale", path: "/vat-rates", more: true },
    ]},
    { sec: "Raportare", items: [
      { key: "rapoarte", label: "Rapoarte", icon: "chart", path: "/reports" },
      { key: "declaratii", label: "Declarații ANAF", icon: "docText", path: "/declarations" },
      { key: "etransport", label: "e-Transport", icon: "truck", path: "/etransport" },
      { key: "contabilitate", label: "Jurnal contabil", icon: "scale", path: "/ledger" },
    ]},
  ];

  const isActive = (it: NavLink) =>
    it.matchPrefix
      ? location.pathname === it.matchPrefix || location.pathname.startsWith(`${it.matchPrefix}/`)
      : location.pathname === it.path;

  const navItem = (it: NavLink) => (
    <Link key={it.key} to={it.path as "/"} className={`nav-item${isActive(it) ? " active" : ""}`}>
      <Ic name={it.icon} />
      <span className="nlabel">{it.label}</span>
      {it.badge != null && <span className="badge num">{it.badge}</span>}
    </Link>
  );

  const toggleLang = () => { void i18n.changeLanguage(i18n.language?.startsWith("en") ? "ro" : "en"); };
  // Display name = the license email's local part, capitalized ("andrei@…" → "Andrei").
  const accountName = (() => {
    const local = (license?.email ?? "").split("@")[0];
    if (!local) return "Cont Clarito";
    return local.split(/[._-]+/).filter(Boolean).map((w) => w[0].toUpperCase() + w.slice(1)).join(" ");
  })();
  const handleExit = async () => { (await import("@tauri-apps/plugin-process")).exit(0); };
  const langTag = i18n.language?.startsWith("en") ? "EN" : "RO";

  return (
    <aside className="sidebar">
      <div className="side-scroll">
        {/* Company card */}
        <button
          className={`company${companyOpen ? " open" : ""}`}
          onMouseDown={stop}
          onClick={() => { setCompanyOpen((o) => !o); setProfileOpen(false); }}
        >
          <div className="co-ava round">{initials(activeCompany?.legalName)}</div>
          <div className="meta">
            <div className="name">{activeCompany?.legalName ?? "Nicio companie"}</div>
            <div className="cui num">{activeCompany?.cui ?? ""}</div>
          </div>
          <Ic name="chevUD" cls="ic chev" />
        </button>
        {companyOpen && (
          <div className="pop show" id="companyPop" onMouseDown={stop}>
            <div className="col-title">Companie activă</div>
            {activeCompany && (
              <div className="co-row sel">
                <div className="co-ava">{initials(activeCompany.legalName, 1)}</div>
                <div className="co-meta">
                  <div className="co-name">{activeCompany.legalName}</div>
                  <div className="co-cui">{activeCompany.cui}</div>
                </div>
                <Ic name="check" cls="co-check" />
              </div>
            )}
            {companies.filter((c) => c.id !== activeCompany?.id).length > 0 && (
              <>
                <div className="pop-div" />
                <div className="col-title">Schimbă compania</div>
                {companies.filter((c) => c.id !== activeCompany?.id).map((c) => (
                  <button key={c.id} className="co-row" onClick={() => { setActiveCompanyId(c.id); setCompanyOpen(false); }}>
                    <div className="co-ava alt">{initials(c.legalName, 1)}</div>
                    <div className="co-meta">
                      <div className="co-name">{c.legalName}</div>
                      <div className="co-cui">{c.cui}</div>
                    </div>
                    <Ic name="check" cls="co-check" />
                  </button>
                ))}
              </>
            )}
            <div className="pop-div" />
            <button className="pop-item" onClick={() => { setCompanyOpen(false); void navigate({ to: "/companies/new" }); }}>
              <Ic name="plus" />Adaugă companie
            </button>
            <button className="pop-item" onClick={() => { setCompanyOpen(false); void navigate({ to: "/companies" }); }}>
              <Ic name="cog" />Gestionează companiile
            </button>
          </div>
        )}

        {/* Nav groups */}
        {NAV.map((g) => {
          const prim = g.items.filter((i) => !i.more);
          const extra = g.items.filter((i) => i.more);
          const gid = g.sec ?? "x";
          const open = moreOpen[gid] ?? extra.some(isActive);
          return (
            <div key={gid}>
              {g.sec && <div className="sec">{g.sec}</div>}
              <div className="nav-group">
                {prim.map(navItem)}
                {extra.length > 0 && (
                  <>
                    <div className={`nav-extra${open ? " open" : ""}`}>
                      <div className="nav-extra-inner">{extra.map(navItem)}</div>
                    </div>
                    <button className={`nav-more${open ? " open" : ""}`} onClick={() => setMoreOpen((s) => ({ ...s, [gid]: !open }))}>
                      <Ic name="chevD" />
                      <span className="nlabel">{open ? "Mai puține" : "Mai multe"}</span>
                      <span className="badge num">{extra.length}</span>
                    </button>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* Footer: user card */}
      <div className="side-foot">
        <button
          className={`user-card${profileOpen ? " open" : ""}`}
          onMouseDown={stop}
          onClick={() => { setProfileOpen((o) => !o); setCompanyOpen(false); }}
        >
          <div className="u-ava">{initials(license?.email ?? "Clarito", 2)}</div>
          <div className="meta">
            <div className="uname">{accountName}</div>
            <div className="umail">{license?.email ?? "Cont Clarito"}</div>
          </div>
        </button>
      </div>

      {/* Profile pop (anchored to the aside via #profilePop bottom:70px) */}
      {profileOpen && (
        <div className="pop show" id="profilePop" onMouseDown={stop}>
          <div className="pop-head">
            <div className="u-ava">{initials(license?.email ?? "Clarito", 2)}</div>
            <div><div className="pn">{accountName}</div><div className="pm">{license?.email ?? "Cont Clarito"}</div></div>
          </div>
          <div className="pop-div" />
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/settings" }); }}><Ic name="cog" />Setări</button>
          <button className="pop-item" onClick={toggleLang}>
            <Ic name="lang" />Limbă
            <span className="pill-new" style={{ marginLeft: "auto", background: "var(--fill)", color: "var(--text-2)", border: "1px solid var(--line)" }}>{langTag}</span>
          </button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/notifications" }); }}>
            <Ic name="bell" />Notificări
            {unreadCount != null && unreadCount > 0 && <span className="pill-new" style={{ marginLeft: "auto", background: "var(--black)" }}>{unreadCount}</span>}
          </button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/documents" }); }}><Ic name="docText" />Documente</button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/account" }); }}><Ic name="team" />Cont & Licență</button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/help" }); }}><Ic name="help" />Ajutor</button>
          <div className="pop-div" />
          <button className="pop-item" onClick={() => void handleExit()}><Ic name="exit" />Ieșire</button>
        </div>
      )}
    </aside>
  );
}
