/**
 * Mesaje SPV / Notificări — verbatim port of the design "Mesaje SPV.html":
 *   .page-head (title + necitite sub + pill-btn "Marchează toate citite" ·
 *   pill-btn "Șterge citite" · btn-dark spin-btn "Reîmprospătează")
 *   .banner.warn (retenție ANAF 60 zile) → .scr-card → .scr-toolbar
 *   (.tabs Toate/Necitite · .spacer · .scr-search) → .msg-list
 *   (.msg unread · .cli-ava · .msg-from/.msg-dot/.msg-time · .msg-sub ·
 *   .msg-tag chip) → .pager (.pg-btns).
 *
 * ALL wiring preserved: api.notifications.list(false), unreadCount,
 * tabs all/unread, row click → markRead + navigate (entityType/entityId,
 * spv_msg_* → /received), markAllRead, deleteOne (kept as .mini-btn —
 * prototype lacks it), deleteAllRead (confirm).
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Notification } from "@/types";

// ── Icons missing from the Ic map — inlined verbatim from the prototype ──────

const P_CIRCLE_CHECK = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const P_TRASH = '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';
const P_WARN_TRI = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const P_CHEV_L = '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>';

function InlineIc({ path, cls = "ic" }: { path: string; cls?: string }) {
  return <svg className={cls} viewBox="0 0 24 24" aria-hidden="true" dangerouslySetInnerHTML={{ __html: path }} />;
}

// ── Date helpers (dd lll yyyy + relative msg time) ────────────────────────────

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];

const fmtRoDateLocal = (d: Date) =>
  `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}`;

const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** Relative time like the prototype: "acum 2 ore" / "ieri, 18:30" / "08 iun 2026". */
function fmtMsgTime(unix: number): string {
  const d = new Date(unix * 1000);
  const now = new Date();
  const diff = Math.floor(Date.now() / 1000) - unix;
  if (diff < 3600 && sameDay(d, now)) {
    const m = Math.max(1, Math.floor(diff / 60));
    return `acum ${m} ${m === 1 ? "minut" : "minute"}`;
  }
  if (sameDay(d, now)) {
    const h = Math.floor(diff / 3600);
    return `acum ${h} ${h === 1 ? "oră" : "ore"}`;
  }
  const yest = new Date(now);
  yest.setDate(now.getDate() - 1);
  if (sameDay(d, yest)) {
    return `ieri, ${d.toLocaleTimeString("ro-RO", { hour: "2-digit", minute: "2-digit" })}`;
  }
  return fmtRoDateLocal(d);
}

// ── Notification type → sender + avatar + chip (design .chip variants) ────────

interface MsgKind {
  from: string;
  ava: string;
  cls: string;
  label: string;
  /** Ic name when available; otherwise raw inline path. */
  icon?: string;
  path?: string;
}

function msgKind(type: string): MsgKind {
  const t = type.toUpperCase();
  if (t.includes("REJECT") || t.includes("FAIL") || t.includes("ERROR"))
    return { from: "ANAF · e-Factura", ava: "AN", cls: "late", label: "Eroare", path: P_WARN_TRI };
  if (t.includes("VALID") || t.includes("ACCEPT") || t.includes("RECIPIS"))
    return { from: "ANAF · e-Factura", ava: "AN", cls: "paid", label: "Recipisă", path: P_CIRCLE_CHECK };
  if (t.includes("STALE") || t.includes("WARN") || t.includes("EXPIR"))
    return { from: "SPV e-Factura", ava: "SP", cls: "wait", label: "Reminder", icon: "clock" };
  if (t.includes("SYNC"))
    return { from: "SPV e-Factura", ava: "SP", cls: "sent", label: "Sistem", icon: "sync" };
  if (t.includes("IMPORT") || t.includes("RECEIV"))
    return { from: "SPV e-Factura", ava: "SP", cls: "sent", label: "Notificare", icon: "send" };
  return { from: "ANAF · e-Factura", ava: "AN", cls: "sent", label: "Notificare", icon: "send" };
}

/** Page size for the .pager (client-side pagination over the full list). */
const PAGE_SIZE = 20;
/** ANAF SPV message retention window (days). */
const RETENTION_DAYS = 60;

type TabFilter = "all" | "unread";

export function NotificationsPage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [tab, setTab] = useState<TabFilter>("all");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);

  const {
    data: notifications = [],
    isLoading,
    isError: notifError,
    error: notifErr,
    refetch: refetchNotifications,
  } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn: () => api.notifications.list(false),
  });

  const { data: unreadCount = 0 } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
  });

  // "ultima sincronizare azi, 09:14" — same source as the StatusBar.
  const { data: lastSyncRaw } = useQuery({
    queryKey: queryKeys.settings.get("last_sync_at"),
    queryFn: () => api.settings.get("last_sync_at"),
  });

  const { mutate: markRead } = useMutation({
    mutationFn: (id: string) => api.notifications.markRead(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    },
  });

  const { mutate: markAllRead, isPending: markingAll } = useMutation({
    mutationFn: () => api.notifications.markAllRead(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      notify.success("Toate notificările marcate ca citite.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut marca ca citite.")),
  });

  const { mutate: deleteOne, isPending: deletingOne } = useMutation({
    mutationFn: (id: string) => api.notifications.deleteOne(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge notificarea.")),
  });

  const { mutate: deleteAllRead, isPending: deletingAllRead } = useMutation({
    mutationFn: () => api.notifications.deleteAllRead(),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      notify.success("Notificările citite au fost șterse.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-au putut șterge notificările.")),
  });

  /** Confirm before bulk-deleting all read notifications (irreversible). */
  async function handleDeleteAllRead() {
    const readCount = notifications.filter((n) => n.isRead).length;
    const ok = await confirm(
      `Ștergeți ${readCount} notificări citite? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteAllRead();
  }

  /** Mark as read then navigate to related entity if possible. */
  function handleRowClick(n: Notification) {
    if (!n.isRead) markRead(n.id);

    // Try structured JSON payload: {"entityType": "invoice"|"received", "entityId": "..."}
    if (n.data) {
      try {
        const parsed = JSON.parse(n.data) as Record<string, unknown>;
        const entityType = parsed["entityType"];
        const entityId = parsed["entityId"];
        if (typeof entityId === "string" && typeof entityType === "string") {
          if (entityType === "invoice") {
            void navigate({ to: "/invoices/$id", params: { id: entityId } });
            return;
          }
          if (entityType === "received") {
            void navigate({ to: "/received/$id", params: { id: entityId } });
            return;
          }
        }
      } catch {
        // data is not JSON — fall through
      }
      // Plain key "spv_msg_*" → received invoices list
      if (n.data.startsWith("spv_msg_")) {
        void navigate({ to: "/received" });
        return;
      }
    }
  }

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return notifications
      .filter((n) => (tab === "unread" ? !n.isRead : true))
      .filter((n) => {
        if (!q) return true;
        return (
          n.title.toLowerCase().includes(q) ||
          n.body.toLowerCase().includes(q) ||
          n.notificationType.toLowerCase().includes(q)
        );
      });
  }, [notifications, tab, query]);

  // Reset to page 1 whenever the filters change the result set.
  useEffect(() => {
    setPage(1);
  }, [tab, query]);

  const pageCount = Math.max(1, Math.ceil(list.length / PAGE_SIZE));
  const curPage = Math.min(page, pageCount);
  const visible = list.slice((curPage - 1) * PAGE_SIZE, curPage * PAGE_SIZE);
  const fromIdx = list.length === 0 ? 0 : (curPage - 1) * PAGE_SIZE + 1;
  const toIdx = Math.min(curPage * PAGE_SIZE, list.length);

  // Numbered page buttons — window of max 5 around the current page.
  const pageNums = useMemo(() => {
    const start = Math.max(1, Math.min(curPage - 2, pageCount - 4));
    const end = Math.min(pageCount, start + 4);
    const nums: number[] = [];
    for (let p = start; p <= end; p++) nums.push(p);
    return nums;
  }, [curPage, pageCount]);

  // "cele mai vechi expiră în N zile" — oldest message + 60-day ANAF retention.
  const expireDays = useMemo(() => {
    if (notifications.length === 0) return null;
    const oldest = notifications.reduce((min, n) => Math.min(min, n.createdAt), Infinity);
    const days = Math.ceil((oldest + RETENTION_DAYS * 86400 - Date.now() / 1000) / 86400);
    return days > 0 ? days : null;
  }, [notifications]);

  const lastSyncLabel = useMemo(() => {
    if (!lastSyncRaw) return null;
    const ts = parseInt(lastSyncRaw, 10);
    if (!Number.isFinite(ts) || ts <= 0) return null;
    const d = new Date(ts * 1000);
    const now = new Date();
    const hm = d.toLocaleTimeString("ro-RO", { hour: "2-digit", minute: "2-digit" });
    if (sameDay(d, now)) return `azi, ${hm}`;
    const yest = new Date(now);
    yest.setDate(now.getDate() - 1);
    if (sameDay(d, yest)) return `ieri, ${hm}`;
    return fmtRoDateLocal(d);
  }, [lastSyncRaw]);

  const readCount = notifications.filter((n) => n.isRead).length;

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Mesaje SPV / Notificări</h1>
          <p className="sub">
            {unreadCount} necitite · recipise, notificări, somații și decizii din SPV
            {lastSyncLabel ? ` · ultima sincronizare ${lastSyncLabel}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="pill-btn"
            disabled={unreadCount === 0 || markingAll}
            style={unreadCount === 0 || markingAll ? { opacity: 0.5, cursor: "default" } : undefined}
            onClick={() => markAllRead()}
          >
            <InlineIc path={P_CIRCLE_CHECK} />Marchează toate citite
          </button>
          <button
            className="pill-btn"
            disabled={readCount === 0 || deletingAllRead}
            style={readCount === 0 || deletingAllRead ? { opacity: 0.5, cursor: "default" } : undefined}
            onClick={() => void handleDeleteAllRead()}
          >
            <InlineIc path={P_TRASH} />Șterge citite
          </button>
          <button className="btn-dark spin-btn" onClick={() => void refetchNotifications()}>
            <Ic name="sync" />Reîmprospătează
          </button>
        </div>
      </div>

      {/* 60-day retention banner */}
      <div className="banner warn">
        <Ic name="clock" />
        <span>
          <b>ANAF păstrează mesajele doar 60 de zile</b> — sincronizați regulat pentru a nu pierde
          recipise și notificări. Clarito arhivează automat tot ce descarcă.
        </span>
      </div>

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            <div className={`tab${tab === "all" ? " active" : ""}`} onClick={() => setTab("all")}>
              Toate<span className="cnt">{notifications.length}</span>
            </div>
            <div className={`tab${tab === "unread" ? " active" : ""}`} onClick={() => setTab("unread")}>
              Necitite<span className="cnt">{unreadCount}</span>
            </div>
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Caută mesaje…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* message list */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : notifError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={notifErr} label="notificările" onRetry={() => void refetchNotifications()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {notifications.length === 0
              ? "Nicio notificare. Sistemul va afișa aici mesajele de la ANAF."
              : query.trim()
                ? "Niciun mesaj pentru căutarea aplicată."
                : "Nicio notificare necitită."}
          </div>
        ) : (
          <div className="msg-list">
            {visible.map((n) => {
              const k = msgKind(n.notificationType);
              return (
                <div
                  key={n.id}
                  className={`msg${!n.isRead ? " unread" : ""}`}
                  onClick={() => handleRowClick(n)}
                >
                  <span className="cli-ava">{k.ava}</span>
                  <div className="msg-main">
                    <div className="msg-top">
                      <span className="msg-from">{k.from}</span>
                      {!n.isRead && <span className="msg-dot" />}
                      <span className="msg-time num">{fmtMsgTime(n.createdAt)}</span>
                    </div>
                    <div className="msg-sub">
                      {n.title}
                      {n.body ? ` — ${n.body}` : ""}
                    </div>
                  </div>
                  <div
                    className="msg-tag"
                    style={{ display: "flex", alignItems: "center", gap: 4 }}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <span className={`chip ${k.cls}`}>
                      {k.icon ? <Ic name={k.icon} cls="sic" /> : <InlineIc path={k.path!} cls="sic" />}
                      {k.label}
                    </span>
                    {/* real feature kept — prototype lacks per-row delete */}
                    <button
                      className="mini-btn"
                      title="Șterge notificare"
                      disabled={deletingOne}
                      onClick={() => deleteOne(n.id)}
                    >
                      <InlineIc path={P_TRASH} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* pager */}
        <div className="pager">
          <span>
            Afișezi <b>{fromIdx === toIdx ? toIdx : `${fromIdx}–${toIdx}`}</b> din <b>{list.length}</b> mesaje
            {expireDays !== null && (
              <>
                {" "}· cele mai vechi expiră în <b className="num">{expireDays} {expireDays === 1 ? "zi" : "zile"}</b>
              </>
            )}
          </span>
          <div className="pg-btns">
            <button
              className="pg-btn"
              disabled={curPage <= 1}
              onClick={() => setPage(curPage - 1)}
            >
              <InlineIc path={P_CHEV_L} />
            </button>
            {pageNums.map((p) => (
              <button
                key={p}
                className={`pg-btn${p === curPage ? " cur" : ""}`}
                onClick={() => setPage(p)}
              >
                {p}
              </button>
            ))}
            <button
              className="pg-btn"
              disabled={curPage >= pageCount}
              onClick={() => setPage(curPage + 1)}
            >
              <Ic name="chevR" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
