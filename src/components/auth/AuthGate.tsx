/**
 * AuthGate — wraps the entire app and enforces authentication.
 *
 * On startup it calls `auth_status` (PUBLIC command) to determine which
 * screen to show:
 *   1. needsSetup=true  → <SetupAdminScreen>  (first-run, no users)
 *   2. !authenticated   → <LoginScreen>
 *   3. authenticated    → children (the real app)
 *
 * Session state is held in a Zustand slice (authStore) so any component
 * can read `currentUser` and `role` without prop-drilling.
 */

import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";

import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { useAuthStore } from "@/lib/auth-store";
import { SetupAdminScreen } from "./SetupAdminScreen";
import { LoginScreen } from "./LoginScreen";

interface AuthGateProps {
  children: ReactNode;
}

export function AuthGate({ children }: AuthGateProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { setCurrentUser, clearCurrentUser } = useAuthStore();

  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.auth.status,
    queryFn: () => api.auth.status(),
    // Poll auth status every 5 minutes to detect server-side session changes.
    staleTime: 5 * 60 * 1000,
    // Don't retry on error — we'll show a loading state.
    retry: false,
  });

  // Keep Zustand auth store in sync with the query result.
  useEffect(() => {
    if (data?.authenticated && data.currentUser) {
      setCurrentUser(data.currentUser);
    } else {
      clearCurrentUser();
    }
  }, [data, setCurrentUser, clearCurrentUser]);

  if (isLoading) {
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--rf-bg)",
        }}
      >
        <span style={{ color: "var(--rf-text-dim)", fontSize: 14 }}>
          {t("shared.common.loading")}
        </span>
      </div>
    );
  }

  if (error) {
    // Auth status check itself failed — show a minimal error.
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--rf-bg)",
          color: "var(--rf-text-dim)",
          fontFamily: "system-ui, sans-serif",
          fontSize: 14,
          padding: 24,
          textAlign: "center",
        }}
      >
        {t("shared.common.authLoadError")}
      </div>
    );
  }

  if (!data) return null;

  // Case 1: first launch — no users yet.
  if (data.needsSetup) {
    return (
      <SetupAdminScreen
        onSuccess={() => {
          void queryClient.invalidateQueries({ queryKey: queryKeys.auth.status });
        }}
      />
    );
  }

  // Case 2: users exist but not logged in.
  if (!data.authenticated) {
    return (
      <LoginScreen
        onSuccess={() => {
          void queryClient.invalidateQueries({ queryKey: queryKeys.auth.status });
        }}
      />
    );
  }

  // Case 3: authenticated — render the full app.
  return <>{children}</>;
}
