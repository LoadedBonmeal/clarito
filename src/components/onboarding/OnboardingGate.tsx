import { type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";

import { OnboardingWizard } from "./OnboardingWizard";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

function LoadingScreen() {
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--bg)",
        zIndex: 9999,
      }}
    >
      <div style={{ fontSize: 12, color: "var(--text-muted)" }}>Se încarcă…</div>
    </div>
  );
}

interface OnboardingGateProps {
  children: ReactNode;
}

export function OnboardingGate({ children }: OnboardingGateProps) {
  const { data: companies = [], isLoading } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  if (isLoading) return <LoadingScreen />;
  if (companies.length === 0) return <OnboardingWizard />;
  return <>{children}</>;
}
