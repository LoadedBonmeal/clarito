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

const TIER_NAMES: Record<string, string> = {
  TRIAL: "Trial",
  SOLO: "Solo",
  ACCOUNTANT: "Contabil",
  FIRM: "Firmă",
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
function levelChip(level: string): { cls: string; bar: string; label: string; svg: string | null; icon: string | null } {
  switch (level) {
    case "ok":          return { cls: "paid", bar: "ok",   label: "În limită",   svg: SVG_CHECK_CIRCLE,  icon: null };
    case "approaching": return { cls: "wait", bar: "warn", label: "Se apropie",  svg: SVG_WARN_TRIANGLE, icon: null };
    case "exceeded":    return { cls: "late", bar: "bad",  label: "Depășit",     svg: SVG_WARN_TRIANGLE, icon: null };
    default:            return { cls: "sent", bar: "",     label: "Nu se aplică", svg: null,             icon: "dot" };
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
          {chip.label}
        </span>
      </div>
      <div className="pv num">
        {value === null ? "—" : fmtLei(value)} <span className="of">/ {fmtLei(plafon)} lei</span>
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
      `Ștergeți compania "${c.legalName}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.companies.delete(c.id);
      if (activeCompanyId === c.id) setActiveCompanyId(null);
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
    } catch (err) {
      const payload = err as AppErrorPayload;
      notify.error(formatError(payload, "Eroare la ștergerea companiei."));
    }
  };

  const tabs: Array<{ value: SpvFilter; label: string; count: number }> = [
    { value: "all", label: "Toate",    count: companies.length },
    { value: "yes", label: "Cu SPV",   count: withSpv },
    { value: "no",  label: "Fără SPV", count: companies.length - withSpv },
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

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Companii</h1>
          <p className="sub">
            {companies.length === 1 ? "1 companie administrată" : `${companies.length} companii administrate`}
            {license
              ? ` · planul ${TIER_NAMES[license.tier] ?? license.tier} permite ${tierLimit === Infinity ? "nelimitat" : tierLimit}`
              : ""}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="btn-dark"
            style={atLimit ? { opacity: 0.5, cursor: "not-allowed" } : undefined}
            title={atLimit ? "Limita planului dumneavoastră este atinsă" : undefined}
            onClick={() => {
              if (atLimit) {
                notify.info("Limita planului este atinsă. Contactați-ne pentru upgrade la support@efactura.ro");
                return;
              }
              void navigate({ to: "/companies/new" });
            }}
          >
            <Ic name="plus" />Adaugă companie
          </button>
        </div>
      </div>

      {/* license limit reached — real feature kept (prototype lacks it) */}
      {license && atLimit && tierLimit < Infinity && (
        <div className="banner warn">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN_TRIANGLE }} />
          <span>
            <b>Limita planului {TIER_NAMES[license.tier] ?? license.tier} este atinsă</b> ({companies.length}/{tierLimit} companii).
            Contactați-ne pentru upgrade la <b>support@efactura.ro</b>.
          </span>
        </div>
      )}

      <div className="scr-card" style={{ marginBottom: 18 }}>
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tt">Toate companiile</div>
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
              placeholder="Caută companie…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          {/* refresh — real feature kept (prototype lacks it) */}
          <button className="sq-btn spin-btn" title="Reîncarcă" onClick={() => void refetchCompanies()}>
            <Ic name="sync" />
          </button>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : companiesError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={companiesErr} label="companiile" onRetry={() => void refetchCompanies()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {companies.length === 0
              ? "Nicio companie. Adăugați prima companie cu butonul „Adaugă companie”."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>CUI</th>
                <th>Denumire</th>
                <th>Localitate</th>
                <th>Județ</th>
                <th style={{ textAlign: "center" }}>SPV</th>
                <th>Serie</th>
                <th>Reg. Com.</th>
                <th style={{ textAlign: "center" }}>Activă</th>
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
                    style={isActive ? { background: "#FCFCFD" } : undefined}
                    onClick={() => void navigate({ to: "/companies/$id", params: { id: c.id } })}
                  >
                    <td><span className="doc">{c.cui}</span></td>
                    <td>
                      <div className="cli">
                        <span
                          className="cli-ava"
                          style={isActive ? { background: "var(--black)", color: "#fff", border: 0 } : undefined}
                        >
                          {(c.legalName[0] ?? "—").toUpperCase()}
                        </span>
                        {isActive ? <b>{c.legalName}</b> : c.legalName}
                        {c.tradeName && (
                          <span className="muted" style={{ marginLeft: 6, fontSize: 11.5 }}>({c.tradeName})</span>
                        )}
                        <span className="chip sent" style={{ marginLeft: 6 }}>
                          {c.taxRegime === "profit" ? "Profit · 16%" : "Micro · 1%"}
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
                          Activă
                        </span>
                      ) : (
                        <span className="chip sent">Inactivă</span>
                      )}
                    </td>
                    <td onClick={(e) => e.stopPropagation()}>
                      {/* row actions — real features kept (prototype lacks them) */}
                      <div className="row-acts">
                        {!isActive && (
                          <button className="mini-btn" title="Setează ca activă" onClick={() => setActiveCompanyId(c.id)}>
                            <Ic name="check" />
                          </button>
                        )}
                        <button
                          className="mini-btn"
                          title="Editează"
                          onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: c.id } })}
                        >
                          <Ic name="pen" />
                        </button>
                        <button className="mini-btn" title="Șterge" onClick={() => void handleDelete(c)}>
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
            Monitorizare plafoane — {activeCompany.legalName}{" "}
            {microYtd !== null && (
              <span className="muted" style={{ fontWeight: 400, fontSize: 12.5 }}>
                · venituri cumulate {currentYear}: {fmtLei(microYtd)} lei
              </span>
            )}
          </div>

          <div className="plafon-grid">
            <PlafonCard
              title={<>Plafon micro&shy;întreprindere</>}
              ps={
                <>
                  100.000 EUR (OUG 89/2025) la curs BNR 31.12.{currentYear - 1} ={" "}
                  {(regimeStatus?.eurRate ?? OFFICIAL_EOY_EUR[currentYear] ?? 5.0).toLocaleString("ro-RO", { maximumFractionDigits: 4 })} →{" "}
                  <b className="num">{microCeiling !== null ? `${fmtLei(microCeiling)} lei` : "—"}</b>
                </>
              }
              level={microLevel}
              value={microLevel === "na" ? null : microYtd}
              plafon={microCeiling ?? 0}
              pct={microLevel === "na" ? null : (regimeStatus?.pct ?? null)}
              foot={
                microLevel === "na"
                  ? (regimeStatus?.note ?? "compania este pe impozit pe profit")
                  : "la depășire → impozit pe profit 16% din trimestrul următor"
              }
            />

            <PlafonCard
              title="Plafon înregistrare TVA"
              ps={
                <>
                  art. 310 / Legea 141/2025 · <b className="num">{fmtLei(vatPlafon)} lei</b> — relevant doar
                  pentru neplătitori de TVA
                </>
              }
              level={vatReg ? (vatReg.applicable ? vatReg.level : "na") : "na"}
              value={vatReg?.applicable ? vatYtd : null}
              plafon={vatPlafon}
              pct={vatReg?.applicable ? vatReg.pct : null}
              foot={
                vatReg?.applicable
                  ? vatReg.level === "exceeded"
                    ? "înregistrarea în scopuri de TVA este obligatorie"
                    : "la depășire → înregistrare obligatorie în scopuri de TVA"
                  : "compania este deja înregistrată în scopuri de TVA"
              }
            />

            <PlafonCard
              title="Plafon TVA la încasare"
              ps={
                <>
                  OUG 8/2026 · <b className="num">{cashPlafon !== null ? fmtLei(cashPlafon) : "5.000.000"} lei</b> cifră
                  de afaceri — doar pentru companii pe TVA la încasare
                </>
              }
              level={regimeStatus?.cashVatLevel ?? "na"}
              value={regimeStatus && regimeStatus.cashVatLevel !== "na" ? parseDec(regimeStatus.ytdTurnoverRon) : null}
              plafon={cashPlafon ?? 5_000_000}
              pct={cashPct}
              foot={
                regimeStatus?.cashVatLevel === "na"
                  ? (regimeStatus?.cashVatNote ?? "compania nu aplică TVA la încasare")
                  : regimeStatus?.cashVatLevel === "exceeded"
                    ? (regimeStatus?.cashVatNote ?? "plafonul TVA la încasare a fost depășit")
                    : "sub plafonul TVA la încasare"
              }
            />
          </div>

          {showMicroBanner && regimeStatus && microCeiling !== null && (
            <div className={`banner ${microLevel === "exceeded" ? "danger" : "warn"}`} style={{ marginTop: 16 }}>
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN_TRIANGLE }} />
              <span>
                <b>
                  {microLevel === "exceeded"
                    ? `Plafonul micro a fost depășit (${fmtPct(regimeStatus.pct)}%).`
                    : `Plafonul micro se apropie (${fmtPct(regimeStatus.pct)}%).`}
                </b>{" "}
                La depășirea plafonului de {fmtLei(microCeiling)} lei compania trece la{" "}
                <b>impozit pe profit 16%</b> începând cu trimestrul depășirii (OUG 89/2025). Monitorizarea
                apare și pe Privire generală.
              </span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
