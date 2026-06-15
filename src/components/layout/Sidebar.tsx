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
  id: string;
  sec: string | null;
  items: NavLink[];
}

const stop = (e: React.SyntheticEvent) => e.stopPropagation();

export function Sidebar() {
  const location = useLocation();
  const navigate = useNavigate();
  const { t, i18n } = useTranslation();
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
    { id: "main", sec: null, items: [
      { key: "privire", label: t("shell.nav.overview"), icon: "grid", path: "/" },
      { key: "facturi-emise", label: t("shell.nav.invoicesIssued"), icon: "docUp", path: "/invoices", matchPrefix: "/invoices" },
      { key: "facturi-primite", label: t("shell.nav.invoicesReceived"), icon: "docDown", path: "/received", matchPrefix: "/received" },
      { key: "mesaje-spv", label: t("shell.nav.spvMessages"), icon: "mail", path: "/notifications", badge: unreadCount || undefined },
      { key: "stornate", label: t("shell.nav.credited"), icon: "undo", path: "/stornate" },
    ]},
    { id: "operare", sec: t("shell.nav.groupOperate"), items: [
      { key: "clienti", label: t("shell.nav.contacts"), icon: "users", path: "/contacts" },
      { key: "chitante", label: t("shell.nav.receipts"), icon: "receipt", path: "/receipts" },
      { key: "urmarire-plati", label: t("shell.nav.payments"), icon: "card", path: "/payments" },
      { key: "salarizare", label: t("shell.nav.payroll"), icon: "idcard", path: "/payroll" },
      { key: "articole", label: t("shell.nav.products"), icon: "cube", path: "/products" },
      { key: "companii", label: t("shell.nav.companies"), icon: "building", path: "/companies", matchPrefix: "/companies", more: true },
      { key: "facturi-recurente", label: t("shell.nav.recurring"), icon: "loop", path: "/recurring", more: true },
      { key: "mijloace-fixe", label: t("shell.nav.assets"), icon: "wrench", path: "/assets", more: true },
      { key: "plan-conturi", label: t("shell.nav.accounts"), icon: "book", path: "/accounts", more: true },
      { key: "cote-tva", label: t("shell.nav.vatRates"), icon: "scale", path: "/vat-rates", more: true },
      { key: "dividende", label: t("shell.nav.dividends"), icon: "incasat", path: "/dividends", more: true },
    ]},
    { id: "raportare", sec: t("shell.nav.groupReporting"), items: [
      { key: "rapoarte", label: t("shell.nav.reports"), icon: "chart", path: "/reports" },
      { key: "declaratii", label: t("shell.nav.declarations"), icon: "docText", path: "/declarations" },
      { key: "etransport", label: t("shell.nav.etransport"), icon: "truck", path: "/etransport" },
      { key: "contabilitate", label: t("shell.nav.ledger"), icon: "scale", path: "/ledger" },
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
    if (!local) return t("shell.profile.defaultAccount");
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
            <div className="name">{activeCompany?.legalName ?? t("shell.company.none")}</div>
            <div className="cui num">{activeCompany?.cui ?? ""}</div>
          </div>
          <Ic name="chevUD" cls="ic chev" />
        </button>
        {companyOpen && (
          <div className="pop show" id="companyPop" onMouseDown={stop}>
            <div className="col-title">{t("shell.company.active")}</div>
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
                <div className="col-title">{t("shell.company.switch")}</div>
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
              <Ic name="plus" />{t("shell.company.add")}
            </button>
            <button className="pop-item" onClick={() => { setCompanyOpen(false); void navigate({ to: "/companies" }); }}>
              <Ic name="cog" />{t("shell.company.manage")}
            </button>
          </div>
        )}

        {/* Nav groups */}
        {NAV.map((g) => {
          const prim = g.items.filter((i) => !i.more);
          const extra = g.items.filter((i) => i.more);
          const gid = g.id;
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
                      <span className="nlabel">{open ? t("shell.nav.fewer") : t("shell.nav.more")}</span>
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
            <div className="umail">{license?.email ?? t("shell.profile.defaultAccount")}</div>
          </div>
        </button>
      </div>

      {/* Profile pop (anchored to the aside via #profilePop bottom:70px) */}
      {profileOpen && (
        <div className="pop show" id="profilePop" onMouseDown={stop}>
          <div className="pop-head">
            <div className="u-ava">{initials(license?.email ?? "Clarito", 2)}</div>
            <div><div className="pn">{accountName}</div><div className="pm">{license?.email ?? t("shell.profile.defaultAccount")}</div></div>
          </div>
          <div className="pop-div" />
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/settings" }); }}><Ic name="cog" />{t("shell.profile.settings")}</button>
          <button className="pop-item" onClick={toggleLang}>
            <Ic name="lang" />{t("shell.profile.language")}
            <span className="pill-new" style={{ marginLeft: "auto", background: "var(--fill)", color: "var(--text-2)", border: "1px solid var(--line)" }}>{langTag}</span>
          </button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/notifications" }); }}>
            <Ic name="bell" />{t("shell.profile.notifications")}
            {unreadCount != null && unreadCount > 0 && <span className="pill-new" style={{ marginLeft: "auto", background: "var(--black)" }}>{unreadCount}</span>}
          </button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/documents" }); }}><Ic name="docText" />{t("shell.profile.documents")}</button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/account" }); }}><Ic name="team" />{t("shell.profile.account")}</button>
          <button className="pop-item" onClick={() => { setProfileOpen(false); void navigate({ to: "/help" }); }}><Ic name="help" />{t("shell.profile.help")}</button>
          <div className="pop-div" />
          <button className="pop-item" onClick={() => void handleExit()}><Ic name="exit" />{t("shell.profile.exit")}</button>
        </div>
      )}
    </aside>
  );
}
