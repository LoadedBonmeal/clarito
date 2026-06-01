/**
 * Notificări ANAF/SPV — Mesaje SPV — re-skinned to rf kit (Wave 4).
 * Preserves: api.notifications.list(false), unreadCount, tabs all/unread,
 * row click → markRead + navigate (entityType/entityId),
 * Marcează toate citite → api.notifications.markAllRead,
 * delete one → api.notifications.deleteOne,
 * Șterge citite → api.notifications.deleteAllRead (confirm).
 */

import { useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Empty, Segmented,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Notification } from "@/types";

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("ro-RO");
}

function kindIcon(type: string): { icon: string; color: string } {
  if (type === "REJECT") return { icon: "xCircle", color: "error" };
  if (type === "VALID")  return { icon: "checkCircle", color: "success" };
  if (type === "WARN" || type === "EXPIR") return { icon: "alertTriangle", color: "warning" };
  return { icon: "mail", color: "info" };
}

type TabFilter = "all" | "unread";

export function NotificationsPage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [tab, setTab] = useState<TabFilter>("all");

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
    if (tab === "unread") return notifications.filter((n) => !n.isRead);
    return notifications;
  }, [notifications, tab]);

  const tabOptions = [
    { value: "all" as TabFilter, label: `Toate (${notifications.length})` },
    { value: "unread" as TabFilter, label: `Necitite (${unreadCount})` },
  ];

  return (
    <div className="rf-page">
      <PageHeader
        title="Mesaje SPV / Notificări"
        sub={
          unreadCount > 0 ? (
            <Badge variant="info" dot>{unreadCount} necitite</Badge>
          ) : undefined
        }
        actions={
          <>
            <Btn
              variant="secondary"
              size="sm"
              icon="check"
              disabled={unreadCount === 0 || markingAll}
              onClick={() => markAllRead()}
            >
              Marchează toate citite
            </Btn>
            <Btn
              variant="ghost"
              size="sm"
              icon="trash"
              disabled={notifications.filter((n) => n.isRead).length === 0 || deletingAllRead}
              onClick={() => void handleDeleteAllRead()}
            >
              Șterge citite
            </Btn>
            <Btn
              variant="primary"
              size="sm"
              icon="refresh"
              onClick={() => void refetchNotifications()}
            >
              Reîmprospătează
            </Btn>
          </>
        }
      />

      <div className="rf-page-body" style={{ maxWidth: 860, width: "100%" }}>
        <Card>
          {/* Tabs */}
          <div style={{ padding: "10px 16px 0", borderBottom: "1px solid var(--rf-border)" }}>
            <Segmented options={tabOptions} value={tab} onChange={(v) => setTab(v)} />
          </div>

          {/* Content */}
          {isLoading ? (
            <Empty icon="mail" title="Se încarcă…" />
          ) : notifError ? (
            <QueryErrorBanner error={notifErr} label="notificările" onRetry={() => void refetchNotifications()} />
          ) : list.length === 0 ? (
            <Empty icon="mail" title={tab === "unread" ? "Nicio notificare necitită" : "Nicio notificare"}>
              {tab === "all" && "Sistemul va afișa aici mesajele de la ANAF."}
            </Empty>
          ) : (
            <div>
              {list.map((n, i) => {
                const { icon, color } = kindIcon(n.notificationType);
                return (
                  <div
                    key={n.id}
                    onClick={() => handleRowClick(n)}
                    style={{
                      display: "flex",
                      gap: 14,
                      padding: "15px 18px",
                      cursor: "pointer",
                      borderBottom: i < list.length - 1 ? "1px solid var(--rf-border)" : "none",
                      background: !n.isRead ? "var(--rf-accent-bg, var(--rf-success-bg))" : "transparent",
                      transition: "background 0.1s",
                    }}
                    onMouseEnter={(e) => {
                      if (n.isRead) (e.currentTarget as HTMLDivElement).style.background = "var(--rf-neutral-bg)";
                    }}
                    onMouseLeave={(e) => {
                      (e.currentTarget as HTMLDivElement).style.background = !n.isRead ? "var(--rf-accent-bg, var(--rf-success-bg))" : "transparent";
                    }}
                  >
                    {/* Colored icon badge */}
                    <span
                      style={{
                        width: 36,
                        height: 36,
                        borderRadius: "var(--rf-radius-sm)",
                        display: "grid",
                        placeItems: "center",
                        flexShrink: 0,
                        background: `var(--rf-${color}-bg)`,
                        color: `var(--rf-${color})`,
                        border: `1px solid var(--rf-${color}-bd, var(--rf-border))`,
                      }}
                    >
                      <Icon name={icon} size={17} />
                    </span>

                    {/* Content */}
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 2 }}>
                        <span
                          style={{
                            fontSize: 13.5,
                            fontWeight: !n.isRead ? 700 : 500,
                            color: "var(--rf-text)",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {n.title}
                        </span>
                        {!n.isRead && (
                          <span
                            style={{
                              width: 7,
                              height: 7,
                              borderRadius: "50%",
                              background: "var(--rf-accent)",
                              flexShrink: 0,
                            }}
                          />
                        )}
                      </div>
                      {n.body && (
                        <div
                          style={{
                            fontSize: 12,
                            color: "var(--rf-text-muted)",
                            marginBottom: 3,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {n.body}
                        </div>
                      )}
                      <div style={{ fontSize: 11, color: "var(--rf-text-dim)", display: "flex", alignItems: "center", gap: 5 }}>
                        <span
                          style={{
                            fontWeight: 600,
                            color: `var(--rf-${color})`,
                            textTransform: "uppercase",
                            letterSpacing: "0.04em",
                          }}
                        >
                          {n.notificationType}
                        </span>
                        <span style={{ opacity: 0.5 }}>·</span>
                        <span>{fmtTime(n.createdAt)}</span>
                      </div>
                    </div>

                    {/* Actions — chevron always visible, trash on hover */}
                    <div
                      style={{ display: "flex", alignItems: "center", gap: 2, flexShrink: 0 }}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <IconBtn
                        icon="trash"
                        title="Șterge notificare"
                        disabled={deletingOne}
                        onClick={() => deleteOne(n.id)}
                      />
                      <IconBtn
                        icon="chevRight"
                        title="Deschide"
                        onClick={() => handleRowClick(n)}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>Total: <b>{notifications.length}</b> notificări</span>
            <span>Necitite: <b style={{ color: "var(--rf-accent)" }}>{unreadCount}</b></span>
          </div>
        </Card>
      </div>
    </div>
  );
}
