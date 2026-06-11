/**
 * TopBar — verbatim port of the design `.topbar` (clarito-shell.js), wired to
 * real data: brand + collapse, centered search (→ command palette), SPV pill
 * (→ syncSpv), +Nou menu (→ routes), notifications bell. IDs #spvSync/#nouBtn/
 * #bellBtn match clarito-shell.css so the icon hover-animations apply for free.
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

const NOU_ITEMS = [
  { labelKey: "shell.topbar.newInvoice", icon: "docUp", to: "/invoices/new" },
  { labelKey: "shell.topbar.newReceipt", icon: "receipt", to: "/receipts" },
  { labelKey: "shell.topbar.newContact", icon: "users", to: "/contacts" },
  { labelKey: "shell.topbar.newProduct", icon: "cube", to: "/products" },
];

/** The design brand mark — white "C" glyph on the near-black `.mark` square. */
function MarkGlyph() {
  return (
    <svg viewBox="0 0 32 32" fill="none" style={{ width: 16, height: 16, display: "block" }}>
      <path d="M23 9.4A9 9 0 1 0 23 22.6" stroke="var(--rf-text-on-accent)" strokeWidth="2.7" strokeLinecap="round" />
      <circle cx="16" cy="16" r="2.9" fill="var(--rf-text-on-accent)" />
    </svg>
  );
}

export function TopBar() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);

  const [nouOpen, setNouOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const nouRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (nouRef.current && !nouRef.current.contains(e.target as Node)) setNouOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);

  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const anafTestMode = testModeSetting === "1";

  const { data: isAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  const { data: unreadCount } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
    refetchInterval: 60_000,
  });

  const handleSyncSpv = async () => {
    if (!activeCompanyId) { notify.warn(t("shell.notify.selectCompany")); return; }
    if (syncing) return;
    setSyncing(true);
    try {
      const n = await api.anaf.syncSpv(activeCompanyId, anafTestMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      notify[n > 0 ? "success" : "info"](n > 0 ? t("shell.notify.spvNew", { count: n }) : t("shell.notify.noNewSpv"));
      void navigate({ to: "/received" });
    } catch (e) {
      notify.error(formatError(e, t("shell.notify.spvSyncFailed")));
    } finally {
      setSyncing(false);
    }
  };

  const connected = !activeCompanyId || isAnafAuth;

  return (
    <header className="topbar">
      {/* Brand + collapse */}
      <div className="brand">
        <div className="b-logo">
          <div className="mark"><MarkGlyph /></div>
          <span className="wordmark">Clarito</span>
        </div>
        <button className="collapse-btn" onClick={toggleSidebar} aria-label={t("shell.topbar.collapseMenu")}>
          <Ic name="collapse" />
        </button>
      </div>

      {/* Centered search → command palette */}
      <div className="searchwrap">
        <div className="search" onClick={() => setCommandOpen(true)} role="button" tabIndex={0}>
          <Ic name="lens" />
          <input type="text" placeholder={t("shell.topbar.searchPlaceholder")} readOnly />
          <span className="kbd">⌘ K</span>
        </div>
      </div>

      {/* Right cluster */}
      <div className="topright">
        <div className="spv-pill">
          {connected && <span className="spv-dot" />}
          <Ic name="shield" cls={connected ? "spv-ic" : "spv-ic spv-ic--err"} />
          SPV: {connected ? t("shell.topbar.spvConnected") : t("shell.topbar.spvNotAuth")}
          <span className="spv-div" />
          <button
            id="spvSync"
            className={`spv-sync spin-btn${syncing ? " spinning" : ""}`}
            onClick={() => void handleSyncSpv()}
            disabled={syncing}
            aria-label={t("shell.topbar.syncSpv")}
          >
            <Ic name="sync" />
          </button>
        </div>

        <div className="nou-wrap" ref={nouRef}>
          <button id="nouBtn" className={`btn-dark${nouOpen ? " anim-open" : ""}`} onClick={() => setNouOpen((o) => !o)}>
            <Ic name="plus" />
            {t("shell.topbar.new")}
            <Ic name="chevD" cls="ic chev-sm" />
          </button>
          {nouOpen && (
            <div className="pop show" id="nouPop">
              <div className="col-title">{t("shell.topbar.create")}</div>
              {NOU_ITEMS.map((it) => (
                <button
                  key={it.to}
                  className="pop-item"
                  onClick={() => { setNouOpen(false); void navigate({ to: it.to as "/" }); }}
                >
                  <Ic name={it.icon} />
                  {t(it.labelKey)}
                </button>
              ))}
              <div className="pop-div" />
              <button
                className="pop-item"
                onClick={() => { setNouOpen(false); void navigate({ to: "/received" }); }}
              >
                <Ic name="docDown" />
                {t("shell.topbar.importSpv")}
              </button>
            </div>
          )}
        </div>

        <div className="bell-wrap">
          <button id="bellBtn" className="icon-btn" onClick={() => void navigate({ to: "/notifications" })} aria-label={t("shell.profile.notifications")}>
            <Ic name="bell" />
            {unreadCount != null && unreadCount > 0 && <span className="bell-dot" />}
          </button>
        </div>
      </div>
    </header>
  );
}
