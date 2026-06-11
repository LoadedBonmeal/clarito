/**
 * OnboardingGate — Instalare design (same .wiz-wrap/.wiz card as the wizard).
 *
 * Gate logic: 100% preserved.
 *  - Loading → LoadingScreen (centered spinner on the wizard backdrop)
 *  - No companies → OnboardingWizard (first run)
 *  - Companies + invalid license → LicenseExpiredScreen (.wiz card)
 *  - Companies + valid license → render children
 *
 * LicenseExpiredScreen: purchase url → openUrl, activate form → api.license.activate.
 */

import { useState, type ReactNode } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";

import { OnboardingWizard } from "./OnboardingWizard";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { BrandMark } from "@/components/shared/BrandMark";

// ─── Loading screen ───────────────────────────────────────────────────────────

function LoadingScreen() {
  return (
    <div className="wiz-wrap">
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, margin: "auto" }}>
        <div className="wiz-spin" />
        <span style={{ fontSize: 13.5, color: "var(--text-2)" }}>Se încarcă…</span>
      </div>
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

  const handleBuy = async () => {
    try {
      const purchase = await api.settings.get("purchase_url");
      await openUrl(purchase || "https://lucaris.ro/rofactura#pret");
    } catch {
      window.open("https://lucaris.ro/rofactura#pret", "_blank");
    }
  };

  return (
    <div className="wiz-wrap">
      <div className="wiz">
        <div className="wiz-top">
          <div className="brand">
            <BrandMark size={34} />
            <span className="word">Clarito</span>
          </div>
        </div>

        <div className="wiz-body" style={{ minHeight: 0, paddingTop: 24 }}>
          {!showActivate ? (
            <div className="step active" style={{ minHeight: 0 }}>
              <h2>Licența a expirat</h2>
              <p className="lead">
                Perioada de probă de <strong>14 zile</strong> s-a încheiat sau licența nu mai este
                validă pe această mașină. Datele tale sunt păstrate local și nu vor fi șterse.
              </p>
              <div className="anaf-card">
                <div className="anaf-row">
                  <div className="anaf-ic">
                    <svg viewBox="0 0 24 24"><path d="M12 9v3.75m9-.75a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9 3.75h.008v.008H12v-.008Z" /></svg>
                  </div>
                  <div style={{ flex: 1 }}>
                    <div className="at">Licență Clarito</div>
                    <div className="as">Reactivează pentru a continua să emiți facturi</div>
                  </div>
                  <span className="chip wait">
                    <svg className="sic" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5" /></svg>
                    Expirată
                  </span>
                </div>
                <button
                  className="btn btn-dark"
                  type="button"
                  style={{ width: "100%", marginTop: 14 }}
                  onClick={() => { void handleBuy(); }}
                >
                  <svg className="ic" viewBox="0 0 24 24"><path d="M13.5 6H5.25A2.25 2.25 0 0 0 3 8.25v10.5A2.25 2.25 0 0 0 5.25 21h10.5A2.25 2.25 0 0 0 18 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" /></svg>
                  Cumpără licența
                </button>
              </div>
              <button
                className="btn btn-link"
                type="button"
                onClick={() => { setShowActivate(true); setActivateError(null); }}
              >
                Am deja o licență — introdu cheia →
              </button>
            </div>
          ) : (
            <form
              className="step active"
              style={{ minHeight: 0 }}
              onSubmit={(e) => {
                e.preventDefault();
                setActivateError(null);
                if (!key.trim()) { setActivateError("Introduceți cheia de licență."); return; }
                if (!actEmail.trim()) { setActivateError("Introduceți emailul de achiziție."); return; }
                activateMutation.mutate();
              }}
            >
              <h2>Activare licență</h2>
              <p className="lead">Introdu cheia primită prin email după achiziție.</p>
              <div className="field">
                <label htmlFor="exp-key">Cheie licență</label>
                <input
                  id="exp-key"
                  className="input num"
                  placeholder="XXXX-XXXX-XXXX-XXXX"
                  style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}
                  value={key}
                  onChange={(e) => setKey(e.target.value.toUpperCase())}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="hint">Format XXXX-XXXX-XXXX-XXXX.</span>
              </div>
              <div className="field">
                <label htmlFor="exp-email">Email achiziție</label>
                <input
                  id="exp-email"
                  className="input"
                  type="email"
                  placeholder="office@firma.ro"
                  value={actEmail}
                  onChange={(e) => setActEmail(e.target.value)}
                />
              </div>
              {activateError && <p className="werr">{activateError}</p>}
            </form>
          )}
        </div>

        {showActivate && (
          <div className="wiz-foot">
            <button
              className="btn btn-ghost"
              type="button"
              disabled={activateMutation.isPending}
              onClick={() => { setShowActivate(false); setActivateError(null); }}
            >
              <svg className="ic" viewBox="0 0 24 24" style={{ transform: "scaleX(-1)" }}><path d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" /></svg>
              Înapoi
            </button>
            <button
              className="btn btn-dark"
              type="button"
              disabled={activateMutation.isPending}
              onClick={() => {
                setActivateError(null);
                if (!key.trim()) { setActivateError("Introduceți cheia de licență."); return; }
                if (!actEmail.trim()) { setActivateError("Introduceți emailul de achiziție."); return; }
                activateMutation.mutate();
              }}
            >
              {activateMutation.isPending ? "Se activează…" : "Activează licența"}
              <svg className="ic arrow" viewBox="0 0 24 24"><path d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" /></svg>
            </button>
          </div>
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
