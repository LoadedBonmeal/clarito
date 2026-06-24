/**
 * TopBar — verbatim port of the design `.topbar` (clarito-shell.js), wired to
 * real data: brand + collapse, centered search (→ command palette), SPV pill
 * (→ syncSpv), +Nou menu (→ routes), notifications bell. IDs #spvSync/#nouBtn/
 * #bellBtn match clarito-shell.css so the icon hover-animations apply for free.
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Notification } from "@/types";

const NOU_ITEMS = [
  { labelKey: "shell.topbar.newInvoice", icon: "docUp", to: "/invoices/new" },
  { labelKey: "shell.topbar.newReceipt", icon: "receipt", to: "/receipts" },
  { labelKey: "shell.topbar.newContact", icon: "users", to: "/contacts" },
  { labelKey: "shell.topbar.newProduct", icon: "cube", to: "/products" },
];

/** Notification type → an Ic-set icon name for the bell popup (decoupled from the full
 *  `msgKind` in Notifications.tsx — only Ic-set names, no inline paths). */
function notifIcon(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("REJECT") || t.includes("FAIL") || t.includes("ERROR")) return "shield";
  if (t.includes("VALID") || t.includes("ACCEPT") || t.includes("RECIPIS")) return "check";
  if (t.includes("STALE") || t.includes("WARN") || t.includes("EXPIR")) return "clock";
  if (t.includes("SYNC")) return "sync";
  if (t.includes("IMPORT") || t.includes("RECEIV")) return "send";
  return "mail";
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
  const [notifOpen, setNotifOpen] = useState(false);
  const [bellPulse, setBellPulse] = useState(false); // arrival swing + badge pop
  const [syncSettle, setSyncSettle] = useState(false); // overshoot settle after sync ends
  const nouRef = useRef<HTMLDivElement>(null);
  const bellRef = useRef<HTMLDivElement>(null);
  const prevUnread = useRef<number | null>(null);
  const prevSyncing = useRef(false);

  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (nouRef.current && !nouRef.current.contains(e.target as Node)) setNouOpen(false);
      if (bellRef.current && !bellRef.current.contains(e.target as Node)) setNotifOpen(false);
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

  // Recent notifications for the bell popup — fetched lazily when the popup opens (the same
  // query key the /notifications page uses, so it's usually warm from cache).
  const { data: recent = [], isFetching: recentFetching } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn: () => api.notifications.list(false),
    enabled: notifOpen,
  });

  const { mutate: markRead } = useMutation({
    mutationFn: (id: string) => api.notifications.markRead(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all }),
  });

  // Bell swings + badge pops ONCE when the unread count INCREASES (anBellSwing/anBadgePop, wired
  // here per the prototype). Skip the first resolve so existing unread on load doesn't ring.
  useEffect(() => {
    const c = unreadCount ?? 0;
    if (prevUnread.current !== null && c > prevUnread.current) {
      setBellPulse(true);
      const id = setTimeout(() => setBellPulse(false), 600);
      prevUnread.current = c;
      return () => clearTimeout(id);
    }
    prevUnread.current = c;
  }, [unreadCount]);

  // Sync icon overshoot-settle (anSyncSettle) right after a sync finishes — the spin itself is the
  // existing `spin-btn spinning`; this only adds the short settle flourish on the syncing→idle edge.
  useEffect(() => {
    if (prevSyncing.current && !syncing) {
      setSyncSettle(true);
      const id = setTimeout(() => setSyncSettle(false), 400);
      prevSyncing.current = syncing;
      return () => clearTimeout(id);
    }
    prevSyncing.current = syncing;
  }, [syncing]);

  // Open a notification: mark read + close popup + route to the related entity (mirrors the
  // /notifications page row click). Falls back to the full notifications page.
  const openNotif = (n: Notification) => {
    if (!n.isRead) markRead(n.id);
    setNotifOpen(false);
    if (n.data) {
      try {
        const parsed = JSON.parse(n.data) as Record<string, unknown>;
        const entityType = parsed["entityType"];
        const entityId = parsed["entityId"];
        if (typeof entityId === "string" && typeof entityType === "string") {
          if (entityType === "invoice") { void navigate({ to: "/invoices/$id", params: { id: entityId } }); return; }
          if (entityType === "received") { void navigate({ to: "/received/$id", params: { id: entityId } }); return; }
        }
      } catch { /* data is not JSON — fall through */ }
      if (n.data.startsWith("spv_msg_")) { void navigate({ to: "/received" }); return; }
    }
    void navigate({ to: "/notifications" });
  };

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
      {/* Collapse toggle (the brand now lives in the sidebar's company card) */}
      <div className="brand">
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
            data-anim-state={syncSettle ? "settle" : undefined}
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

        <div className="bell-wrap" ref={bellRef}>
          <button
            id="bellBtn"
            className="icon-btn"
            onClick={() => setNotifOpen((o) => !o)}
            aria-label={t("shell.profile.notifications")}
            aria-expanded={notifOpen}
          >
            <Ic name="bell" cls={bellPulse ? "ic bell-swing" : "ic"} />
            {unreadCount != null && unreadCount > 0 && (
              <span className={`bell-dot${bellPulse ? " badge-pop" : ""}`} />
            )}
          </button>
          {notifOpen && (
            <div className="pop show" id="notifPop">
              <div className="notif-head">
                <div className="nh-t">{t("shell.topbar.notifTitle")}</div>
                <a
                  className="nh-a"
                  onClick={() => { setNotifOpen(false); void navigate({ to: "/notifications" }); }}
                >
                  {t("shell.topbar.notifSeeAll")}
                </a>
              </div>
              {recent.length > 0 ? (
                recent.slice(0, 6).map((n) => (
                  <div key={n.id} className="notif-item" onClick={() => openNotif(n)}>
                    <div className="notif-ic"><Ic name={notifIcon(n.notificationType)} /></div>
                    <div className="notif-tx">
                      <div className="n1">{n.isRead ? n.title : <b>{n.title}</b>}</div>
                      <div className="n2">{n.body}</div>
                    </div>
                  </div>
                ))
              ) : recentFetching ? null : (
                <div className="notif-empty">{t("shell.topbar.notifEmpty")}</div>
              )}
            </div>
          )}
        </div>
      </div>
    </header>
  );
}
