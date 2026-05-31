/**
 * StatusBar — chips informative la baza ferestrei.
 *
 * Design actualizat: pulse-dot ANAF live, sincronizare, mesaje SPV,
 * companie activă cu swatch color, chip licență permanent.
 */

import { useQuery } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";

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
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  const { data: lastSyncRaw } = useQuery({
    queryKey: queryKeys.settings.get("last_sync_at"),
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

  const { data: license } = useQuery({
    queryKey: queryKeys.license,
    queryFn: () => api.license.get(),
    staleTime: 60_000,
  });

  const { data: purchaseUrl } = useQuery({
    queryKey: queryKeys.settings.get("purchase_url"),
    queryFn: () => api.settings.get("purchase_url"),
    staleTime: Infinity,
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
      {/* ANAF status */}
      <span className="statusbar-chip">
        <span className={anafOk ? "anaf-dot" : "anaf-dot err"} />
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

      {/* Chip licență */}
      {license != null && (() => {
        const isTrial = license.tier === "TRIAL";
        const expired = license.isExpired || (license.trialDaysRemaining != null && license.trialDaysRemaining <= 0);
        const warn = isTrial && !expired && license.trialDaysRemaining != null && license.trialDaysRemaining <= 5;

        let label: string;
        if (isTrial) {
          if (expired) {
            label = "Probă expirată";
          } else {
            label = `Probă · ${license.trialDaysRemaining} zile`;
          }
        } else {
          label = `Licență ${license.tier.charAt(0) + license.tier.slice(1).toLowerCase()}`;
        }

        const handleClick = isTrial
          ? async () => {
              try {
                const url = purchaseUrl || "https://lucaris.ro/rofactura#pret";
                await openPath(url);
              } catch {
                window.open("https://lucaris.ro/rofactura#pret", "_blank");
              }
            }
          : undefined;

        return (
          <span
            className="statusbar-chip"
            onClick={handleClick ?? undefined}
            style={{
              cursor: isTrial ? "pointer" : "default",
              color: (expired || warn) ? "#D97706" : undefined,
              fontWeight: expired ? 700 : undefined,
            }}
            title={isTrial ? "Cumpărați licența" : undefined}
          >
            <Icon name="info" size={12} />
            <span>{label}</span>
          </span>
        );
      })()}

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
