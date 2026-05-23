/**
 * Notificări ANAF/SPV — listează toate notificările din backend,
 * cu posibilitate de filtrare Toate/Necitite și marcare ca citite.
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("ro-RO");
}

function typeColor(type: string): string {
  if (type === "REJECT") return "#DC2626";
  if (type === "VALID") return "#16A34A";
  if (type === "WARN" || type === "EXPIR") return "#D97706";
  return "var(--accent)";
}

type TabFilter = "all" | "unread";

export function NotificationsPage() {
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<TabFilter>("all");

  const { data: notifications = [], isLoading } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn: () => api.notifications.list(false),
  });

  const { data: unreadCount = 0 } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
  });

  const { mutate: markRead } = useMutation({
    mutationFn: (id: string) => api.notifications.markRead(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    },
  });

  const { mutate: markAllRead } = useMutation({
    mutationFn: () => api.notifications.markAllRead(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
    },
  });

  const list = useMemo(() => {
    if (tab === "unread") return notifications.filter((n) => !n.isRead);
    return notifications;
  }, [notifications, tab]);

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          Notificări ANAF
        </span>
        {unreadCount > 0 && (
          <span className="badge" style={{ marginLeft: 8 }}>
            {unreadCount} necitite
          </span>
        )}
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn"
            disabled={unreadCount === 0}
            onClick={() => markAllRead()}
          >
            <Icon name="check" size={12} /> Marchează toate ca citite
          </button>
        </span>
      </div>

      {/* Tabs */}
      <div className="views-bar">
        <span
          className={"view-tab " + (tab === "all" ? "active" : "")}
          onClick={() => setTab("all")}
        >
          Toate <span className="count">{notifications.length}</span>
        </span>
        <span
          className={"view-tab " + (tab === "unread" ? "active" : "")}
          onClick={() => setTab("unread")}
        >
          Necitite{" "}
          <span className="count" style={{ color: "var(--accent)" }}>
            {unreadCount}
          </span>
        </span>
      </div>

      <div className="content-body">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : list.length === 0 ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-muted)",
            }}
          >
            {tab === "unread"
              ? "Nicio notificare necitită."
              : "Nicio notificare. Sistemul va afișa aici mesajele de la ANAF."}
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 160 }}>Timp</th>
                <th style={{ width: 100 }}>Tip</th>
                <th style={{ width: 220 }}>Titlu</th>
                <th>Mesaj</th>
                <th style={{ width: 90 }}>Status</th>
              </tr>
            </thead>
            <tbody>
              {list.map((n) => (
                <tr
                  key={n.id}
                  style={{
                    cursor: "pointer",
                    background: !n.isRead ? "var(--accent-soft)" : undefined,
                  }}
                  onClick={() => {
                    if (!n.isRead) markRead(n.id);
                  }}
                >
                  <td className="mono muted" style={{ whiteSpace: "nowrap" }}>
                    {fmtTime(n.createdAt)}
                  </td>
                  <td>
                    <span
                      style={{
                        fontSize: 11,
                        fontWeight: 600,
                        color: typeColor(n.notificationType),
                        textTransform: "uppercase",
                      }}
                    >
                      {n.notificationType}
                    </span>
                  </td>
                  <td>
                    <b style={{ fontSize: 12 }}>{n.title}</b>
                  </td>
                  <td className="muted" style={{ fontSize: 11 }}>
                    {n.body}
                  </td>
                  <td>
                    {n.isRead ? (
                      <span className="muted" style={{ fontSize: 11 }}>
                        Citit
                      </span>
                    ) : (
                      <span
                        style={{
                          fontSize: 11,
                          fontWeight: 600,
                          color: "var(--accent)",
                        }}
                      >
                        Necitit
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div
        style={{
          padding: "6px 14px",
          borderTop: "1px solid var(--border)",
          background: "var(--bg)",
          display: "flex",
          gap: 16,
          fontSize: 11,
          color: "var(--text-muted)",
        }}
      >
        <span>
          Total: <b style={{ color: "var(--text)" }}>{notifications.length}</b> notificări
        </span>
        <span>
          Necitite:{" "}
          <b style={{ color: "var(--accent)" }}>{unreadCount}</b>
        </span>
      </div>
    </div>
  );
}
