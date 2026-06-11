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
import { Trans, useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";

import { OnboardingWizard } from "./OnboardingWizard";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { BrandMark } from "@/components/shared/BrandMark";

// ─── Loading screen ───────────────────────────────────────────────────────────

function LoadingScreen() {
  const { t } = useTranslation();
  return (
    <div className="wiz-wrap">
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, margin: "auto" }}>
        <div className="wiz-spin" />
        <span style={{ fontSize: 13.5, color: "var(--text-2)" }}>{t("onboarding.loading")}</span>
      </div>
    </div>
  );
}

// ─── License expired screen ────────────────────────────────────────────────────

function LicenseExpiredScreen() {
  const { t } = useTranslation();
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
      setActivateError(formatError(err, t("onboarding.errors.activateFailed")));
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
              <h2>{t("onboarding.expired.title")}</h2>
              <p className="lead">
                <Trans i18nKey="onboarding.expired.lead" components={{ b: <strong /> }} />
              </p>
              <div className="anaf-card">
                <div className="anaf-row">
                  <div className="anaf-ic">
                    <svg viewBox="0 0 24 24"><path d="M12 9v3.75m9-.75a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9 3.75h.008v.008H12v-.008Z" /></svg>
                  </div>
                  <div style={{ flex: 1 }}>
                    <div className="at">{t("onboarding.expired.cardTitle")}</div>
                    <div className="as">{t("onboarding.expired.cardSub")}</div>
                  </div>
                  <span className="chip wait">
                    <svg className="sic" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5" /></svg>
                    {t("onboarding.expired.chip")}
                  </span>
                </div>
                <button
                  className="btn btn-dark"
                  type="button"
                  style={{ width: "100%", marginTop: 14 }}
                  onClick={() => { void handleBuy(); }}
                >
                  <svg className="ic" viewBox="0 0 24 24"><path d="M13.5 6H5.25A2.25 2.25 0 0 0 3 8.25v10.5A2.25 2.25 0 0 0 5.25 21h10.5A2.25 2.25 0 0 0 18 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" /></svg>
                  {t("onboarding.expired.buy")}
                </button>
              </div>
              <button
                className="btn btn-link"
                type="button"
                onClick={() => { setShowActivate(true); setActivateError(null); }}
              >
                {t("onboarding.expired.haveKey")}
              </button>
            </div>
          ) : (
            <form
              className="step active"
              style={{ minHeight: 0 }}
              onSubmit={(e) => {
                e.preventDefault();
                setActivateError(null);
                if (!key.trim()) { setActivateError(t("onboarding.errors.enterKey")); return; }
                if (!actEmail.trim()) { setActivateError(t("onboarding.errors.enterPurchaseEmail")); return; }
                activateMutation.mutate();
              }}
            >
              <h2>{t("onboarding.expired.activateTitle")}</h2>
              <p className="lead">{t("onboarding.expired.activateLead")}</p>
              <div className="field">
                <label htmlFor="exp-key">{t("onboarding.license.keyLabel")}</label>
                <input
                  id="exp-key"
                  className="input num"
                  placeholder={t("onboarding.license.keyPlaceholder")}
                  style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}
                  value={key}
                  onChange={(e) => setKey(e.target.value.toUpperCase())}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="hint">{t("onboarding.expired.keyHint")}</span>
              </div>
              <div className="field">
                <label htmlFor="exp-email">{t("onboarding.license.purchaseEmailLabel")}</label>
                <input
                  id="exp-email"
                  className="input"
                  type="email"
                  placeholder={t("onboarding.license.emailPlaceholder")}
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
              {t("onboarding.nav.back")}
            </button>
            <button
              className="btn btn-dark"
              type="button"
              disabled={activateMutation.isPending}
              onClick={() => {
                setActivateError(null);
                if (!key.trim()) { setActivateError(t("onboarding.errors.enterKey")); return; }
                if (!actEmail.trim()) { setActivateError(t("onboarding.errors.enterPurchaseEmail")); return; }
                activateMutation.mutate();
              }}
            >
              {activateMutation.isPending ? t("onboarding.expired.activating") : t("onboarding.expired.activate")}
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
