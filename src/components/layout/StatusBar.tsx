/**
 * StatusBar — chips de info la baza ferestrei.
 *
 * Portat din Claude Design. Folosește .statusbar și .statusbar-chip.
 */

import { useQuery } from "@tanstack/react-query";

import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

interface StatusBarProps {
  activeCompanyName: string;
  activeCompanyId?: string;
  companyCount?: number;
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

  const lastSyncLabel = lastSyncRaw
    ? new Date(parseInt(lastSyncRaw) * 1000).toLocaleTimeString("ro-RO", { hour: "2-digit", minute: "2-digit" })
    : null;

  const version = appInfo?.version ?? "0.1.0";

  return (
    <div className="statusbar">
      <span className="statusbar-chip ok">
        <span className="dot" />
        Conectat
      </span>
      {activeCompanyId && (
        <span className={"statusbar-chip" + (isAnafAuth ? " ok" : " warn")}>
          <span className="dot" />
          {isAnafAuth ? "ANAF Auth ✓" : "ANAF Neautentificat"}
        </span>
      )}
      <span className="statusbar-chip">{companyCount} companii administrate</span>
      <span className="statusbar-chip">
        Activă:{" "}
        <span style={{ fontWeight: 600, color: "var(--text)" }}>
          {activeCompanyName}
        </span>
      </span>
      <span className="statusbar-spacer" />
      {lastSyncLabel && (
        <span className="statusbar-chip">Sync: {lastSyncLabel}</span>
      )}
      <span className="statusbar-chip">UTF-8</span>
      <span className="statusbar-chip">RO_CIUS 1.0.1</span>
      <span className="statusbar-chip">RON · ro-RO</span>
      <span className="statusbar-chip">v{version}</span>
    </div>
  );
}
