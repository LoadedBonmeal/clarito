/**
 * StatusBar — chips informative la baza ferestrei.
 *
 * Design actualizat: pulse-dot ANAF live, sincronizare, mesaje SPV,
 * companie activă cu swatch color.
 */

import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

interface StatusBarProps {
  activeCompanyName: string;
  activeCompanyId?: string;
  companyCount?: number;
}

const DOT_COLORS = [
  "#2848A1", "#7C3AED", "#0891B2", "#D97706", "#16A34A",
  "#0369A1", "#E11D48", "#65A30D", "#525252", "#B45309",
];
function companyColor(cui: string | undefined): string {
  if (!cui) return "#9CA3AF";
  let h = 0;
  for (let i = 0; i < cui.length; i++) h = (h * 31 + cui.charCodeAt(i)) >>> 0;
  return DOT_COLORS[h % DOT_COLORS.length];
}

export function StatusBar({ activeCompanyName, activeCompanyId, companyCount = 0 }: StatusBarProps) {
  const { data: appInfo } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
    staleTime: Infinity,
  });

  const { data: isAnafAuth } = useQuery({
    queryKey: ["anaf", "auth", activeCompanyId ?? ""],
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  const { data: lastSyncRaw } = useQuery({
    queryKey: ["settings", "last_sync_at"],
    queryFn: () => api.settings.get("last_sync_at"),
    refetchInterval: 60_000,
  });

  const { data: unreadCount } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
    refetchInterval: 60_000,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId);
  const activeCui = activeCompany?.cui;
  const dotColor = companyColor(activeCui);

  const lastSyncLabel = lastSyncRaw
    ? new Date(parseInt(lastSyncRaw) * 1000).toLocaleTimeString("ro-RO", {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : null;

  const version = appInfo?.version ?? "0.1.0";
  const anafOk = !activeCompanyId || isAnafAuth;

  return (
    <div className="statusbar">
      {/* ANAF status — pulse-dot verde/roșu */}
      <span className="statusbar-chip">
        {anafOk ? (
          <span className="pulse-dot" />
        ) : (
          <span className="dot-err" />
        )}
        <span>
          <b>ANAF · SPV</b>{" "}
          <span className="label-dim">
            {anafOk ? "conectat" : "neautentificat"}
          </span>
        </span>
      </span>

      {/* Ultima sincronizare */}
      {lastSyncLabel && (
        <span className="statusbar-chip">
          <Icon name="refresh" size={12} />
          <span className="label-dim">Ultima sincronizare</span>
          <b>{lastSyncLabel}</b>
        </span>
      )}

      {/* Mesaje SPV noi */}
      {unreadCount != null && unreadCount > 0 && (
        <span className="statusbar-chip">
          <Icon name="anaf" size={12} />
          <span className="label-dim">Mesaje SPV</span>
          <b>{unreadCount} noi</b>
        </span>
      )}

      {/* Companie activă cu swatch color */}
      {activeCompanyId && (
        <span className="statusbar-chip">
          <span
            style={{
              width: 8,
              height: 8,
              background: dotColor,
              display: "inline-block",
              borderRadius: 2,
              flexShrink: 0,
            }}
          />
          <span className="label-dim">Companie activă</span>
          <b>{activeCompanyName}</b>
          {activeCui && (
            <span className="mono label-dim" style={{ fontSize: 10.5 }}>
              · {activeCui}
            </span>
          )}
        </span>
      )}

      <span className="statusbar-spacer" />

      {/* Info dreapta */}
      <span className="statusbar-chip">
        <span className="label-dim">{companyCount} companii administrate</span>
      </span>
      <span className="statusbar-chip">
        <span className="label-dim">RO_CIUS 1.0.1 · RON · ro-RO</span>
      </span>
      <span className="statusbar-chip">
        <span className="label-dim">v{version}</span>
      </span>
    </div>
  );
}
