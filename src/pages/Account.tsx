/**
 * Cont & Licență — page in the visual language of the design "Echipa.html"
 * (.page-head + .scr-card members-table aesthetics + info .banner), adapted per
 * the product decision: the app is single-user with NO team backend, so this is
 * the REAL account/license page:
 *   .page-head "Cont & Licență" (sub = emailul licenței) →
 *   .scr-card "Licența ta" (.kv: plan chip · cheie mascată · expiră dd lll yyyy ·
 *   zile rămase trial · status chip Activă/Expirată) →
 *   .scr-card "Companii permise" (limita planului vs companii reale + .meter +
 *   .scr-table denumire/CUI/activă — chips ca în tabelul de membri din Echipa) →
 *   "Activează o licență" (.fgrid cheie+email, vizibil când trial/expirat) →
 *   .set-row versiune aplicație.
 *
 * ALL wiring real: api.license.get, api.license.checkLicenseValidity,
 * api.license.activate, api.companies.list, api.system.appInfo.
 */

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { LicenseTier } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
/** dd lll yyyy from a unix-seconds timestamp (e.g. `03 iun 2026`). */
const fmtRoUnix = (ts: number): string => {
  const d = new Date(ts * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}`;
};

const TIER_LIMITS: Record<LicenseTier, number> = {
  TRIAL: 3,
  SOLO: 1,
  ACCOUNTANT: 15,
  FIRM: Infinity,
};

// Prototype icons not in Ic.tsx — inlined verbatim (rule 2).
const SVG_CHECK_CIRCLE = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const SVG_WARN_TRIANGLE = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const SVG_INFO_CIRCLE = '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

/** Mask the license key, keeping only the last group visible (XXXX-…-A1B2). */
function maskKey(key: string | null): string {
  if (!key) return "—";
  const tail = key.slice(-4);
  const masked = key.slice(0, -4).replace(/[^-]/g, "•");
  return masked + tail;
}

// ── AccountPage ───────────────────────────────────────────────────────────────

export function AccountPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const {
    data: license,
    isLoading: licenseLoading,
    isError: licenseError,
    error: licenseErr,
    refetch: refetchLicense,
  } = useQuery({
    queryKey: queryKeys.license,
    queryFn: () => api.license.get(),
  });

  const { data: isValid } = useQuery({
    queryKey: queryKeys.licenseValidity,
    queryFn: () => api.license.checkLicenseValidity(),
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: appInfo } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
  });

  // Activation form (.fgrid key + email → api.license.activate)
  const [keyInput, setKeyInput] = useState("");
  const [emailInput, setEmailInput] = useState("");
  const [activateError, setActivateError] = useState<string | null>(null);

  const activateMutation = useMutation({
    mutationFn: () => api.license.activate(keyInput.trim(), emailInput.trim()),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.license });
      void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
      setKeyInput("");
      setEmailInput("");
      setActivateError(null);
      notify.success(t("account.notify.activated"));
    },
    onError: (e) => setActivateError(formatError(e, t("account.notify.activateError"))),
  });

  const tier = license?.tier ?? null;
  const tierName = tier ? t(`account.tier.${tier}`) : null;
  const tierLimit = tier ? (TIER_LIMITS[tier] ?? Infinity) : null;

  // Status — checkLicenseValidity is authoritative; fallback to isExpired while loading.
  const active = isValid ?? (license ? !license.isExpired : false);

  const daysLeft = license
    ? Math.max(0, Math.floor((license.expiresAt - Date.now() / 1000) / 86400))
    : null;

  // Companii permise — real usage vs plan limit.
  const used = companies.length;
  const limit = tierLimit ?? Infinity;
  const usagePct = limit === Infinity ? 0 : Math.min(100, (used / limit) * 100);
  const meterCls = limit === Infinity ? "ok" : used >= limit ? "bad" : usagePct >= 80 ? "warn" : "ok";

  // Activation form visible when there is no license, it expired, or it's a trial.
  const showActivate = !license || license.isExpired || license.tier === "TRIAL";

  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("account.title")}</h1>
          <p className="sub">
            {licenseLoading
              ? t("account.sub.loading")
              : license
                ? `${license.email ?? t("account.sub.noEmail")} · ${t("account.sub.planAllows", {
                    tier: tierName,
                    limit:
                      limit === Infinity
                        ? t("account.limit.unlimited")
                        : t("account.limit.companies", { count: limit }),
                  })}`
                : t("account.sub.noLicense")}
          </p>
        </div>
      </div>

      {licenseError && (
        <QueryErrorBanner error={licenseErr} label={t("account.errorLabel")} onRetry={() => void refetchLicense()} />
      )}

      {/* licența ta */}
      <div className="scr-card" style={{ marginBottom: 14 }}>
        <div className="scr-toolbar">
          <div className="tt">{t("account.license.title")}</div>
          <div className="spacer" />
          {license && (
            active ? (
              <span className="chip paid">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHECK_CIRCLE }} />
                {t("account.license.active")}
              </span>
            ) : (
              <span className="chip late"><Ic name="xMark" cls="sic" />{t("account.license.expired")}</span>
            )
          )}
        </div>
        <div className="card-pad">
          {licenseLoading ? (
            <div style={{ fontSize: 13, color: "var(--text-2)" }}>{t("account.license.loading")}</div>
          ) : !license ? (
            <div style={{ padding: "28px 0", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("account.license.none")}
            </div>
          ) : (
            <dl className="kv" style={{ margin: 0 }}>
              <dt>{t("account.license.plan")}</dt>
              <dd>
                <span className="chip sent">
                  <Ic name="idcard" cls="sic" />
                  {tierName}
                </span>
              </dd>
              <dt>{t("account.license.key")}</dt>
              <dd><span className="doc num">{maskKey(license.licenseKey)}</span></dd>
              <dt>{t("account.license.email")}</dt>
              <dd>{license.email ?? "—"}</dd>
              <dt>{license.isExpired ? t("account.license.expiredAt") : t("account.license.expiresAt")}</dt>
              <dd>
                <span className="num">{fmtRoUnix(license.expiresAt)}</span>
                {!license.isExpired && daysLeft !== null && license.tier === "TRIAL" && (
                  <span className="muted" style={{ marginLeft: 8 }}>
                    · {t("account.license.trialDaysLeft", { count: license.trialDaysRemaining ?? daysLeft })}
                  </span>
                )}
              </dd>
              <dt>{t("account.license.device")}</dt>
              <dd className="muted">{t("account.license.boundToDevice")}</dd>
            </dl>
          )}
        </div>
      </div>

      {/* companii permise */}
      <div className="scr-card" style={{ marginBottom: 14 }}>
        <div className="scr-toolbar">
          <div className="tt">{t("account.companies.title")}</div>
          <div className="spacer" />
          <span className="muted num" style={{ fontSize: 12.5 }}>
            {used} / {limit === Infinity ? t("account.companies.unlimited") : limit}
          </span>
        </div>
        <div className="card-pad" style={{ paddingTop: 12, paddingBottom: 12 }}>
          <div className="meter">
            <span className={meterCls} style={{ width: `${limit === Infinity ? (used > 0 ? 8 : 0) : usagePct}%` }} />
          </div>
          <div style={{ display: "flex", justifyContent: "space-between", marginTop: 7, fontSize: 11.5, color: "var(--dim)" }}>
            <span className="num">
              {t("account.companies.managed", { count: used })}
            </span>
            <span>
              {limit === Infinity
                ? t("account.companies.planUnlimited")
                : t("account.sub.planAllows", {
                    tier: tierName ?? "—",
                    limit: t("account.limit.companies", { count: limit }),
                  })}
            </span>
          </div>
        </div>
        {companies.length === 0 ? (
          <div style={{ padding: "28px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)", borderTop: "1px solid var(--line)" }}>
            {t("account.companies.empty")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("account.companies.table.name")}</th>
                <th>{t("account.companies.table.cui")}</th>
                <th style={{ textAlign: "center" }}>{t("account.companies.table.active")}</th>
              </tr>
            </thead>
            <tbody>
              {companies.map((c) => {
                const isActive = activeCompanyId === c.id;
                return (
                  <tr key={c.id}>
                    <td>
                      <div className="cli">
                        <span
                          className="cli-ava"
                          style={isActive ? { background: "var(--black)", color: "var(--on-accent)", border: 0 } : undefined}
                        >
                          {(c.legalName[0] ?? "—").toUpperCase()}
                        </span>
                        {isActive ? <b>{c.legalName}</b> : c.legalName}
                      </div>
                    </td>
                    <td><span className="doc">{c.cui}</span></td>
                    <td style={{ textAlign: "center" }}>
                      {isActive ? (
                        <span className="chip paid">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHECK_CIRCLE }} />
                          {t("account.companies.chipActive")}
                        </span>
                      ) : (
                        <span className="chip sent">{t("account.companies.chipInactive")}</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* activează o licență — vizibil când trial / expirat / fără licență */}
      {showActivate && (
        <div className="scr-card" style={{ marginBottom: 14 }}>
          <div className="scr-toolbar"><div className="tt">{t("account.activate.title")}</div></div>
          <div className="card-pad">
            <div className="fgrid">
              <div className="field">
                <label>{t("account.activate.keyLabel")} <span className="req">*</span></label>
                <input
                  className="input num"
                  placeholder="XXXX-XXXX-XXXX-XXXX"
                  value={keyInput}
                  onChange={(e) => setKeyInput(e.target.value.toUpperCase())}
                  style={{ textTransform: "uppercase" }}
                  autoComplete="off"
                  spellCheck={false}
                />
              </div>
              <div className="field">
                <label>{t("account.activate.emailLabel")} <span className="req">*</span></label>
                <input
                  className="input"
                  type="email"
                  placeholder={t("account.activate.emailPlaceholder")}
                  value={emailInput}
                  onChange={(e) => setEmailInput(e.target.value)}
                />
              </div>
            </div>
            {activateError && (
              <div className="banner danger" style={{ marginTop: 12, marginBottom: 0 }}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN_TRIANGLE }} />
                <span>{activateError}</span>
              </div>
            )}
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 13 }}>
              <button
                className="btn-dark"
                style={{ height: 34 }}
                disabled={activateMutation.isPending}
                onClick={() => {
                  setActivateError(null);
                  if (!keyInput.trim()) { setActivateError(t("account.activate.keyRequired")); return; }
                  if (!emailInput.trim()) { setActivateError(t("account.activate.emailRequired")); return; }
                  activateMutation.mutate();
                }}
              >
                <Ic name="check" />
                {activateMutation.isPending ? t("account.activate.activating") : t("account.activate.activateBtn")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* planuri — banner informativ (stil Echipa.html) */}
      <div className="banner">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO_CIRCLE }} />
        <span>
          {t("account.plansBanner.intro")} <b>{t("account.tier.TRIAL")}</b> {t("account.plansBanner.trialLimit")} · <b>{t("account.tier.SOLO")}</b> {t("account.plansBanner.soloLimit")} ·{" "}
          <b>{t("account.tier.ACCOUNTANT")}</b> {t("account.plansBanner.accountantLimit")} · <b>{t("account.tier.FIRM")}</b> {t("account.plansBanner.firmLimit")}. {t("account.plansBanner.upgrade")}{" "}
          <b>support@efactura.ro</b>.
        </span>
      </div>

      {/* versiune aplicație */}
      <div className="scr-card">
        <div className="set-row">
          <div>
            <div className="s1">{t("account.version.title")}</div>
            <div className="s2 num">
              {appInfo ? `${appInfo.name} v${appInfo.version}` : t("account.version.loading")}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
