/**
 * Companii — verbatim port of the design "Companii.html":
 *   .page-head (title + "N companii administrate · planul X permite Y" sub +
 *   btn-dark "Adaugă companie") → .scr-card (.scr-toolbar .tt "Toate companiile" ·
 *   tabs Toate/Cu SPV/Fără SPV · .scr-search) → .scr-table (CUI · Denumire cu
 *   .cli-ava + chip regim fiscal Micro 1%/Profit 16% · Localitate · Județ · SPV ·
 *   Serie · Reg. Com. · Activă) → .sec-h "Monitorizare plafoane" → .plafon-grid
 *   (plafon micro 100.000 EUR · plafon TVA 395.000 lei · plafon TVA la încasare
 *   5.000.000 lei) → .banner warn/danger.
 *
 * ALL wiring preserved: api.companies.list(), api.license.get() (limită plan),
 * tabs Toate/Cu SPV/Fără SPV, search, row click → /companies/$id,
 * set active company (useAppStore), delete confirm → api.companies.delete,
 * "Adaugă companie" → /companies/new, edit → /companies/$id/edit.
 * Plafon cards wired to api.companies.taxRegimeStatus (micro + TVA la încasare)
 * și api.companies.vatRegistrationStatus (art. 310) — same queries as Dashboard.
 */

import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { AppErrorPayload, Company } from "@/types";

const TIER_LIMITS: Record<string, number> = {
  TRIAL: 3,
  SOLO: 1,
  ACCOUNTANT: 15,
  FIRM: Infinity,
};

type SpvFilter = "all" | "yes" | "no";

// Prototype icons not in Ic.tsx — inlined verbatim (rule 2).
const SVG_CHECK_CIRCLE = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const SVG_WARN_TRIANGLE = '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const SVG_TRASH = '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

const fmtLei = (n: number) => Math.round(n).toLocaleString("ro-RO");
const fmtPct = (n: number) =>
  n.toLocaleString("ro-RO", { minimumFractionDigits: 0, maximumFractionDigits: 1 });

// level ("ok" | "approaching" | "exceeded" | "na") → design chip + meter class.
function levelChip(level: string): { cls: string; bar: string; labelKey: string; svg: string | null; icon: string | null } {
  switch (level) {
    case "ok":          return { cls: "paid", bar: "ok",   labelKey: "companies.plafon.level.ok",          svg: SVG_CHECK_CIRCLE,  icon: null };
    case "approaching": return { cls: "wait", bar: "warn", labelKey: "companies.plafon.level.approaching", svg: SVG_WARN_TRIANGLE, icon: null };
    case "exceeded":    return { cls: "late", bar: "bad",  labelKey: "companies.plafon.level.exceeded",    svg: SVG_WARN_TRIANGLE, icon: null };
    default:            return { cls: "sent", bar: "",     labelKey: "companies.plafon.level.na",          svg: null,              icon: "dot" };
  }
}

// ── PlafonCard — verbatim .plafon DOM from the prototype ─────────────────────

interface PlafonCardProps {
  title: React.ReactNode;
  ps: React.ReactNode;
  level: string;
  /** Cumulated turnover (RON) — null renders "—" (not applicable). */
  value: number | null;
  plafon: number;
  pct: number | null;
  foot: string;
}

function PlafonCard({ title, ps, level, value, plafon, pct, foot }: PlafonCardProps) {
  const { t } = useTranslation();
  const chip = levelChip(level);
  const width = pct === null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className="plafon">
      <div className="ph">
        <div>
          <div className="pt">{title}</div>
          <div className="ps">{ps}</div>
        </div>
        <span className={`chip ${chip.cls}`}>
          {chip.svg ? (
            <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: chip.svg }} />
          ) : (
            <Ic name={chip.icon ?? "dot"} cls="sic" />
          )}
          {t(chip.labelKey)}
        </span>
      </div>
      <div className="pv num">
        {value === null ? "—" : fmtLei(value)} <span className="of">{t("companies.plafon.of", { amount: fmtLei(plafon) })}</span>
      </div>
      <div className="meter">
        <span className={chip.bar || undefined} style={{ width: `${width}%` }} />
      </div>
      <div className="pf">
        <span className="num">{pct === null ? "n/a" : `${fmtPct(pct)}%`}</span>
        <span>{foot}</span>
      </div>
    </div>
  );
}

// ── CompaniesPage ─────────────────────────────────────────────────────────────

export function CompaniesPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [query, setQuery] = useState("");
  const [filterSPV, setFilterSPV] = useState<SpvFilter>("all");

  const {
    data: companies = [],
    isLoading,
    isError: companiesError,
    error: companiesErr,
    refetch: refetchCompanies,
  } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: license } = useQuery({
    queryKey: queryKeys.license,
    queryFn: () => api.license.get(),
  });

  // Plafon monitors — same wiring as Dashboard. Conversia plafonului micro se face la
  // cursul BNR de la închiderea exercițiului precedent (31.12 anul anterior), NU la cursul zilei.
  // Pentru 2026 cursul oficial 31.12.2025 = 5,0985 RON/EUR (folosit și ca fallback offline).
  const currentYear = new Date().getFullYear();
  const OFFICIAL_EOY_EUR: Record<number, number> = { 2026: 5.0985 };
  const { data: regimeStatus } = useQuery({
    queryKey: ["taxRegimeStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: async () => {
      let eur = OFFICIAL_EOY_EUR[currentYear] ?? 5.0;
      try {
        eur = await api.bnr.fetchRate("EUR", `${currentYear - 1}-12-31`);
      } catch {
        /* offline — rămâne constanta oficială de închidere de an */
      }
      const status = await api.companies.taxRegimeStatus(activeCompanyId!, currentYear, eur);
      return { ...status, eurRate: eur };
    },
  });

  // Plafonul de scutire TVA (art. 310, Legea 141/2025): 395.000 lei — doar neplătitori de TVA.
  const { data: vatReg } = useQuery({
    queryKey: ["vatRegistrationStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: () => api.companies.vatRegistrationStatus(activeCompanyId!, currentYear),
  });

  const tierLimit = license ? (TIER_LIMITS[license.tier] ?? Infinity) : Infinity;
  const atLimit = companies.length >= tierLimit;

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return companies
      .filter(
        (c) =>
          !q ||
          c.legalName.toLowerCase().includes(q) ||
          c.cui.toLowerCase().includes(q) ||
          c.city.toLowerCase().includes(q),
      )
      .filter((c) =>
        filterSPV === "all" ? true : filterSPV === "yes" ? c.spvEnabled : !c.spvEnabled,
      );
  }, [companies, query, filterSPV]);

  const withSpv = companies.filter((c) => c.spvEnabled).length;

  const handleDelete = async (c: Company) => {
    const ok = await confirm(
      t("companies.confirm.delete", { name: c.legalName }),
      { title: t("companies.confirm.deleteTitle"), kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.companies.delete(c.id);
      if (activeCompanyId === c.id) setActiveCompanyId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
    } catch (err) {
      const payload = err as AppErrorPayload;
      notify.error(formatError(payload, t("companies.notify.deleteError")));
    }
  };

  const tabs: Array<{ value: SpvFilter; label: string; count: number }> = [
    { value: "all", label: t("companies.tabs.all"),        count: companies.length },
    { value: "yes", label: t("companies.tabs.withSpv"),    count: withSpv },
    { value: "no",  label: t("companies.tabs.withoutSpv"), count: companies.length - withSpv },
  ];

  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  // Plafon card derived values (RON, numeric).
  const microYtd     = regimeStatus ? parseDec(regimeStatus.ytdTurnoverRon) : null;
  const microCeiling = regimeStatus ? parseDec(regimeStatus.ceilingRon) : null;
  const cashPlafon   = regimeStatus ? parseDec(regimeStatus.cashVatPlafonRon) : null;
  const cashPct =
    regimeStatus && regimeStatus.cashVatLevel !== "na" && cashPlafon
      ? (parseDec(regimeStatus.ytdTurnoverRon) / cashPlafon) * 100
      : null;
  const vatYtd    = vatReg ? parseDec(vatReg.ytdTurnoverRon) : null;
  const vatPlafon = vatReg ? parseDec(vatReg.plafonRon) : 395_000;

  const microLevel = regimeStatus?.level ?? "na";
  const showMicroBanner = microLevel === "approaching" || microLevel === "exceeded";

  const tierName = license ? t(`companies.tiers.${license.tier}`, { defaultValue: license.tier }) : "";
  const tierLimitLabel = tierLimit === Infinity ? t("companies.head.unlimited") : tierLimit;

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("companies.title")}</h1>
          <p className="sub">
            {t("companies.head.managed", { count: companies.length })}
            {license ? ` · ${t("companies.head.planAllows", { plan: tierName, limit: tierLimitLabel })}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="btn-dark"
            style={atLimit ? { opacity: 0.5, cursor: "not-allowed" } : undefined}
            title={atLimit ? t("companies.head.limitTitle") : undefined}
            onClick={() => {
              if (atLimit) {
                notify.info(t("companies.notify.limitInfo"));
                return;
              }
              void navigate({ to: "/companies/new" });
            }}
          >
            <Ic name="plus" />{t("companies.head.add")}
          </button>
        </div>
      </div>

      {/* license limit reached — real feature kept (prototype lacks it) */}
      {license && atLimit && tierLimit < Infinity && (
        <div className="banner warn">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN_TRIANGLE }} />
          <span>
            <b>{t("companies.banner.limitBold", { plan: tierName })}</b>{" "}
            {t("companies.banner.limitCount", { n: companies.length, limit: tierLimit })}{" "}
            {t("companies.banner.limitContact")} <b>support@efactura.ro</b>.
          </span>
        </div>
      )}

      <div className="scr-card" style={{ marginBottom: 18 }}>
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tt">{t("companies.toolbar.title")}</div>
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${filterSPV === t.value ? " active" : ""}`}
                onClick={() => setFilterSPV(t.value)}
              >
                {t.label}<span className="cnt">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search" style={{ width: 190 }}>
            <Ic name="lens" />
            <input
              type="text"
              placeholder={t("companies.toolbar.searchPlaceholder")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          {/* refresh — real feature kept (prototype lacks it) */}
          <button className="sq-btn spin-btn" title={t("companies.toolbar.refresh")} onClick={() => void refetchCompanies()}>
            <Ic name="sync" />
          </button>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("companies.loading")}</div>
        ) : companiesError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={companiesErr} label={t("companies.errorLabel")} onRetry={() => void refetchCompanies()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {companies.length === 0
              ? t("companies.empty.none")
              : t("companies.empty.filtered")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("companies.table.cui")}</th>
                <th>{t("companies.table.name")}</th>
                <th>{t("companies.table.city")}</th>
                <th>{t("companies.table.county")}</th>
                <th style={{ textAlign: "center" }}>{t("companies.table.spv")}</th>
                <th>{t("companies.table.series")}</th>
                <th>{t("companies.table.regCom")}</th>
                <th style={{ textAlign: "center" }}>{t("companies.table.active")}</th>
                <th className="r" style={{ width: 96 }}></th>
              </tr>
            </thead>
            <tbody>
              {list.map((c) => {
                const isActive = activeCompanyId === c.id;
                return (
                  <tr
                    key={c.id}
                    className="clickable"
                    style={isActive ? { background: "var(--bg-table-header)" } : undefined}
                    onClick={() => void navigate({ to: "/companies/$id", params: { id: c.id } })}
                  >
                    <td><span className="doc">{c.cui}</span></td>
                    <td>
                      <div className="cli">
                        <span
                          className="cli-ava"
                          style={isActive ? { background: "var(--black)", color: "var(--on-accent)", border: 0 } : undefined}
                        >
                          {(c.legalName[0] ?? "—").toUpperCase()}
                        </span>
                        {isActive ? <b>{c.legalName}</b> : c.legalName}
                        {c.tradeName && (
                          <span className="muted" style={{ marginLeft: 6, fontSize: 11.5 }}>({c.tradeName})</span>
                        )}
                        <span className="chip sent" style={{ marginLeft: 6 }}>
                          {c.taxRegime === "profit" ? t("companies.regime.profit") : t("companies.regime.micro")}
                        </span>
                      </div>
                    </td>
                    <td>{c.city}</td>
                    <td>{c.county}</td>
                    <td style={{ textAlign: "center" }}>
                      {c.spvEnabled ? <span className="pos">✓</span> : <span className="muted">—</span>}
                    </td>
                    <td><span className="doc">{c.invoiceSeries}</span></td>
                    <td><span className="doc">{c.registryNumber ?? "—"}</span></td>
                    <td style={{ textAlign: "center" }}>
                      {isActive ? (
                        <span className="chip paid">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_CHECK_CIRCLE }} />
                          {t("companies.status.active")}
                        </span>
                      ) : (
                        <span className="chip sent">{t("companies.status.inactive")}</span>
                      )}
                    </td>
                    <td onClick={(e) => e.stopPropagation()}>
                      {/* row actions — real features kept (prototype lacks them) */}
                      <div className="row-acts">
                        {!isActive && (
                          <button className="mini-btn" title={t("companies.actions.setActive")} onClick={() => setActiveCompanyId(c.id)}>
                            <Ic name="check" />
                          </button>
                        )}
                        <button
                          className="mini-btn"
                          title={t("companies.actions.edit")}
                          onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: c.id } })}
                        >
                          <Ic name="pen" />
                        </button>
                        <button className="mini-btn" title={t("companies.actions.delete")} onClick={() => void handleDelete(c)}>
                          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* plafon monitors — only with an active company (real monitors, like Dashboard) */}
      {activeCompany && (
        <>
          <div className="sec-h" style={{ marginTop: 0 }}>
            {t("companies.plafon.heading")} — {activeCompany.legalName}{" "}
            {microYtd !== null && (
              <span className="muted" style={{ fontWeight: 400, fontSize: 12.5 }}>
                {t("companies.plafon.ytdRevenue", { year: currentYear, amount: fmtLei(microYtd) })}
              </span>
            )}
          </div>

          <div className="plafon-grid">
            <PlafonCard
              title={t("companies.plafon.micro.title")}
              ps={
                <>
                  {t("companies.plafon.micro.ps", {
                    year: currentYear - 1,
                    rate: (regimeStatus?.eurRate ?? OFFICIAL_EOY_EUR[currentYear] ?? 5.0).toLocaleString("ro-RO", { maximumFractionDigits: 4 }),
                  })}{" "}
                  <b className="num">{microCeiling !== null ? `${fmtLei(microCeiling)} lei` : "—"}</b>
                </>
              }
              level={microLevel}
              value={microLevel === "na" ? null : microYtd}
              plafon={microCeiling ?? 0}
              pct={microLevel === "na" ? null : (regimeStatus?.pct ?? null)}
              foot={
                microLevel === "na"
                  ? (regimeStatus?.note ?? t("companies.plafon.micro.footNa"))
                  : t("companies.plafon.micro.foot")
              }
            />

            <PlafonCard
              title={t("companies.plafon.vat.title")}
              ps={
                <>
                  {t("companies.plafon.vat.psPrefix")} <b className="num">{fmtLei(vatPlafon)} lei</b>{" "}
                  {t("companies.plafon.vat.psSuffix")}
                </>
              }
              level={vatReg ? (vatReg.applicable ? vatReg.level : "na") : "na"}
              value={vatReg?.applicable ? vatYtd : null}
              plafon={vatPlafon}
              pct={vatReg?.applicable ? vatReg.pct : null}
              foot={
                vatReg?.applicable
                  ? vatReg.level === "exceeded"
                    ? t("companies.plafon.vat.footExceeded")
                    : t("companies.plafon.vat.footApproaching")
                  : t("companies.plafon.vat.footNa")
              }
            />

            <PlafonCard
              title={t("companies.plafon.cash.title")}
              ps={
                <>
                  {t("companies.plafon.cash.psPrefix")} <b className="num">{cashPlafon !== null ? fmtLei(cashPlafon) : "5.000.000"} lei</b>{" "}
                  {t("companies.plafon.cash.psSuffix")}
                </>
              }
              level={regimeStatus?.cashVatLevel ?? "na"}
              value={regimeStatus && regimeStatus.cashVatLevel !== "na" ? parseDec(regimeStatus.ytdTurnoverRon) : null}
              plafon={cashPlafon ?? 5_000_000}
              pct={cashPct}
              foot={
                regimeStatus?.cashVatLevel === "na"
                  ? (regimeStatus?.cashVatNote ?? t("companies.plafon.cash.footNa"))
                  : regimeStatus?.cashVatLevel === "exceeded"
                    ? (regimeStatus?.cashVatNote ?? t("companies.plafon.cash.footExceeded"))
                    : t("companies.plafon.cash.footOk")
              }
            />
          </div>

          {showMicroBanner && regimeStatus && microCeiling !== null && (
            <div className={`banner ${microLevel === "exceeded" ? "danger" : "warn"}`} style={{ marginTop: 16 }}>
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN_TRIANGLE }} />
              <span>
                <b>
                  {microLevel === "exceeded"
                    ? t("companies.plafon.banner.exceeded", { pct: fmtPct(regimeStatus.pct) })
                    : t("companies.plafon.banner.approaching", { pct: fmtPct(regimeStatus.pct) })}
                </b>{" "}
                {t("companies.plafon.banner.body1", { amount: fmtLei(microCeiling) })}{" "}
                <b>{t("companies.plafon.banner.bodyBold")}</b> {t("companies.plafon.banner.body2")}
              </span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
