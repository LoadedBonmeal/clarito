/**
 * OnboardingGate — Wave 6 re-skin.
 *
 * Gate logic: 100% preserved.
 *  - Loading → LoadingScreen (rf spinner)
 *  - No companies → OnboardingWizard (first run)
 *  - Companies + invalid license → LicenseExpiredScreen (rf card)
 *  - Companies + valid license → render children
 *
 * LicenseExpiredScreen: rf card, purchase url → openUrl, activate form → api.license.activate.
 */

import { useState, type ReactNode } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";

import { OnboardingWizard } from "./OnboardingWizard";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { Icon } from "@/components/shared/Icon";
import { Btn, Field, Input, Banner } from "@/components/rf";

// ─── Loading screen ───────────────────────────────────────────────────────────

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
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14 }}>
        {/* Animated spinner using rf accent */}
        <div
          style={{
            width: 40,
            height: 40,
            borderRadius: "50%",
            border: "3px solid var(--rf-border)",
            borderTopColor: "var(--rf-accent)",
            animation: "spin 0.8s linear infinite",
          }}
        />
        <span className="rf-text-muted" style={{ fontSize: 13.5 }}>Se încarcă…</span>
      </div>

      <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
    </div>
  );
}

// ─── License expired screen ────────────────────────────────────────────────────

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
      setActivateError(formatError(err, "Licența nu a putut fi activată."));
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
          width: 460,
          background: "var(--rf-content)",
          border: "1px solid var(--rf-border)",
          borderRadius: "var(--rf-radius)",
          boxShadow: "var(--rf-shadow-md)",
          padding: "40px 40px 32px",
          textAlign: "center",
        }}
      >
        {/* Icon */}
        <div
          style={{
            width: 60,
            height: 60,
            borderRadius: "50%",
            background: "var(--rf-error-bg)",
            color: "var(--rf-error)",
            display: "grid",
            placeItems: "center",
            margin: "0 auto 20px",
          }}
        >
          <Icon name="alert" size={28} />
        </div>

        {!showActivate ? (
          <>
            <h2
              style={{
                fontSize: 20,
                fontWeight: 700,
                margin: "0 0 10px",
                letterSpacing: "-0.01em",
              }}
            >
              Licența a expirat
            </h2>
            <p className="rf-text-muted" style={{ fontSize: 13.5, margin: "0 0 8px", lineHeight: 1.7 }}>
              Perioada de probă de <strong>14 zile</strong> s-a încheiat sau
              licența nu mai este validă pe această mașină.
            </p>
            <p className="rf-text-muted" style={{ fontSize: 13.5, margin: "0 0 28px", lineHeight: 1.7 }}>
              Datele dvs. sunt păstrate local și nu vor fi șterse.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <Btn
                variant="primary"
                block
                onClick={async () => {
                  try {
                    const purchase = await api.settings.get("purchase_url");
                    await openUrl(purchase || "https://lucaris.ro/rofactura#pret");
                  } catch {
                    window.open("https://lucaris.ro/rofactura#pret", "_blank");
                  }
                }}
              >
                Cumpărați licența →
              </Btn>
              <Btn
                variant="secondary"
                block
                onClick={() => { setShowActivate(true); setActivateError(null); }}
              >
                Am deja o licență — Introduceți cheia →
              </Btn>
            </div>
          </>
        ) : (
          <>
            <h2
              style={{
                fontSize: 18,
                fontWeight: 700,
                margin: "0 0 20px",
                textAlign: "left",
                letterSpacing: "-0.01em",
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
              style={{ display: "flex", flexDirection: "column", gap: 12, textAlign: "left" }}
            >
              <Field label="Cheie licență" required>
                <Input
                  className="rf-mono"
                  placeholder="XXXX-XXXX-XXXX-XXXX"
                  style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}
                  value={key}
                  onChange={(e) => setKey(e.target.value.toUpperCase())}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span style={{ fontSize: 11.5, color: "var(--rf-text-muted)", lineHeight: 1.5 }}>
                  Introduceți cheia primită prin email după achiziție (format XXXX-XXXX-XXXX-XXXX).
                </span>
              </Field>
              <Field label="Email achiziție" required>
                <Input
                  type="email"
                  placeholder="office@firma.ro"
                  value={actEmail}
                  onChange={(e) => setActEmail(e.target.value)}
                />
              </Field>
              {activateError && <Banner variant="error">{activateError}</Banner>}
              <Btn
                type="submit"
                variant="primary"
                disabled={activateMutation.isPending}
                block
              >
                {activateMutation.isPending ? "Se activează…" : "Activează →"}
              </Btn>
              <Btn
                type="button"
                variant="ghost"
                block
                onClick={() => { setShowActivate(false); setActivateError(null); }}
              >
                ← Înapoi
              </Btn>
            </form>
          </>
        )}
      </div>
    </div>
  );
}

// ─── Gate ─────────────────────────────────────────────────────────────────────

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
