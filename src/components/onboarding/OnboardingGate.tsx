import { useState, type ReactNode } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

import { OnboardingWizard } from "./OnboardingWizard";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AppErrorPayload } from "@/types";

function isAppErrorPayload(e: unknown): e is AppErrorPayload {
  return typeof e === "object" && e !== null && ("message" in e || "code" in e);
}

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

/** Shown when the trial has expired or the license record was tampered with. */
function LicenseExpiredScreen() {
  const queryClient = useQueryClient();
  const [showActivate, setShowActivate] = useState(false);
  const [key, setKey] = useState("");
  const [actEmail, setActEmail] = useState("");
  const [activateError, setActivateError] = useState<string | null>(null);

  const activateMutation = useMutation({
    mutationFn: () => api.license.activate(key.trim(), actEmail.trim()),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
    },
    onError: (err) => {
      const message = isAppErrorPayload(err) ? err.message : String(err);
      setActivateError(message || "Licența nu a putut fi activată.");
    },
  });

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 9999,
      }}
    >
      <div
        style={{
          width: 440,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.12)",
          padding: "32px 36px 28px",
          textAlign: "center",
        }}
      >
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: 52,
            height: 52,
            background: "#FEE2E2",
            color: "#DC2626",
            fontSize: 24,
            fontWeight: 700,
            marginBottom: 20,
          }}
        >
          !
        </div>

        {!showActivate ? (
          <>
            <h2
              style={{
                fontSize: 17,
                fontWeight: 700,
                margin: "0 0 10px",
                fontFamily: "var(--font-ui)",
              }}
            >
              Licența a expirat
            </h2>
            <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 8px", lineHeight: 1.7 }}>
              Perioada de probă de <strong>14 zile</strong> s-a încheiat sau
              licența nu mai este validă pe această mașină.
            </p>
            <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 24px", lineHeight: 1.7 }}>
              Datele dvs. sunt păstrate local și nu vor fi șterse.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <button
                type="button"
                className="btn primary"
                style={{ width: "100%", justifyContent: "center", height: 36, fontSize: 12 }}
                onClick={() =>
                  window.open("https://lucaris.ro/rofactura#pret", "_blank")
                }
              >
                Cumpărați licența →
              </button>
              <button
                type="button"
                className="btn"
                style={{ width: "100%", justifyContent: "center", height: 30, fontSize: 11 }}
                onClick={() => setShowActivate(true)}
              >
                Am deja o licență — Activează
              </button>
            </div>
          </>
        ) : (
          <>
            <h2
              style={{
                fontSize: 15,
                fontWeight: 700,
                margin: "0 0 20px",
                fontFamily: "var(--font-ui)",
                textAlign: "left",
              }}
            >
              Activare licență
            </h2>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                setActivateError(null);
                if (!key.trim()) { setActivateError("Introduceți cheia de licență."); return; }
                if (!actEmail.trim()) { setActivateError("Introduceți emailul de achiziție."); return; }
                activateMutation.mutate();
              }}
              style={{ display: "flex", flexDirection: "column", gap: 10, textAlign: "left" }}
            >
              <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
                <label style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}>
                  Cheie licență *
                </label>
                <input
                  className="field"
                  placeholder="XXXX-XXXX-XXXX-XXXX"
                  style={{ fontFamily: "var(--font-mono)", textTransform: "uppercase" }}
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                />
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
                <label style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}>
                  Email achiziție *
                </label>
                <input
                  className="field"
                  type="email"
                  placeholder="office@firma.ro"
                  value={actEmail}
                  onChange={(e) => setActEmail(e.target.value)}
                />
              </div>
              {activateError && (
                <div style={{ padding: "7px 10px", background: "#FEE2E2", border: "1px solid #FECACA", fontSize: 11, color: "#991B1B" }}>
                  {activateError}
                </div>
              )}
              <button
                type="submit"
                disabled={activateMutation.isPending}
                className="btn primary"
                style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
              >
                {activateMutation.isPending ? "Se activează…" : "Activează →"}
              </button>
              <button
                type="button"
                className="btn"
                style={{ width: "100%", justifyContent: "center", height: 28, fontSize: 11 }}
                onClick={() => { setShowActivate(false); setActivateError(null); }}
              >
                ← Înapoi
              </button>
            </form>
          </>
        )}
      </div>
    </div>
  );
}

interface OnboardingGateProps {
  children: ReactNode;
}

export function OnboardingGate({ children }: OnboardingGateProps) {
  const { data: companies = [], isLoading: companiesLoading } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  // Re-check validity every 5 minutes to catch expiry mid-session
  const { data: isLicenseValid, isLoading: licenseLoading } = useQuery({
    queryKey: queryKeys.licenseValidity,
    queryFn: () => api.license.checkLicenseValidity(),
    refetchInterval: 5 * 60 * 1000,
    staleTime: 60_000,
  });

  if (companiesLoading || licenseLoading) return <LoadingScreen />;

  // No companies = first run → show wizard (handles trial start + license activation)
  if (companies.length === 0) return <OnboardingWizard />;

  // Companies exist but license invalid = trial expired or tampered
  if (isLicenseValid === false) return <LicenseExpiredScreen />;

  return <>{children}</>;
}
