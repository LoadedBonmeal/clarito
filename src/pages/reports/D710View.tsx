/**
 * D710View — Declarație rectificativă obligații D100 (OPANAF 587/2016 + 779/2024).
 *
 * GUARDRAILS implementate:
 * 1. Review-before-file gate: "Am verificat declarația" checkbox OBLIGATORIU.
 *    DUK pass ≠ corectitudine semantică — contabilul validează manual.
 * 2. D100 tie: dacă D100 a fost calculat pentru aceeași perioadă, suma inițială
 *    (suma_dat_i) este pre-completată din D100 și afișată read-only; accountantul
 *    trebuie să introducă suma corectată (suma_dat_c) manual.
 * 3. Diff vizibil: "inițial → corectat" per obligație, cu semnal de creștere/scădere.
 * 4. Suma corectată (suma_dat_c) este OBLIGATORIE (UI blochează exportul dacă lipsește).
 * 5. Avertisment dacă se rectifică o obligație fără bază D100 pentru perioada respectivă.
 * 6. DUK block + "Exportă oricum" override.
 * 7. Notă: sumele sunt CORECTE (totaluri), nu diferențe.
 *
 * Embedded în Reports page — Claude-Design classes (.scr-card / .scr-table / .banner / .field).
 */

import { useState, useMemo } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { useOpenXml } from "@/hooks/use-open-xml";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";
import type { D710Input, D710Obligation, PreflightIssue } from "@/lib/tauri";
import type { D100Result } from "@/types";

interface Props {
  dateFrom: string;
  dateTo:   string;
}

const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

/** Obligation row state managed in UI */
interface ObligRow {
  codOblig: string;
  codBugetar: string;
  scadenta: string;
  nrEvid: string;
  denOblig: string;
  sumaDatI: string;   // (I) initial — pre-filled from D100 if available; otherwise user enters
  sumaDatC: string;   // (C) correct — ALWAYS user input, never auto-computed
  d100Basis: boolean; // whether this row has a D100 basis for this period
}

const EMPTY_ROW: ObligRow = {
  codOblig: "", codBugetar: "", scadenta: "", nrEvid: "0",
  denOblig: "", sumaDatI: "", sumaDatC: "", d100Basis: false,
};

function rowToObligation(r: ObligRow): D710Obligation {
  return {
    codOblig:  Number(r.codOblig) || 0,
    codBugetar: r.codBugetar,
    scadenta:  r.scadenta,
    nrEvid:    Number(r.nrEvid) || 0,
    denOblig:  r.denOblig,
    sumaDatI:  r.sumaDatI ? r.sumaDatI : null,
    sumaDatC:  r.sumaDatC ? r.sumaDatC : null,
  };
}

/** Format lei for display */
const fmtLei = (s: string) => {
  const n = parseFloat(s);
  return isNaN(n) ? "—" : n.toLocaleString("ro-RO", { maximumFractionDigits: 0 });
};

/** Diff indicator: shows arrow + delta. */
function DiffCell({ initial, corrected }: { initial: string; corrected: string }) {
  const i = parseFloat(initial);
  const c = parseFloat(corrected);
  if (!corrected) return <span style={{ color: "var(--text-2)" }}>—</span>;
  if (isNaN(i) || isNaN(c)) return <span style={{ color: "var(--text-2)" }}>—</span>;
  const delta = c - i;
  const color = delta < 0 ? "var(--green)" : delta > 0 ? "var(--red)" : "var(--text-2)";
  const sign  = delta > 0 ? "+" : "";
  return (
    <span style={{ color, fontFamily: "var(--mono)", fontSize: 12 }}>
      {sign}{delta.toLocaleString("ro-RO", { maximumFractionDigits: 0 })} lei
    </span>
  );
}

export function D710View({ dateFrom, dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const openXml = useOpenXml();

  const luna    = Number(dateFrom.slice(5, 7));
  const an      = Number(dateFrom.slice(0, 4));
  const quarter = Math.ceil(luna / 3);

  // ── Company for pre-filling header ──────────────────────────────────────
  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn:  () => api.companies.get(activeCompanyId!),
    enabled:  !!activeCompanyId,
  });

  // ── D100 for current period (to pre-fill suma inițială) ─────────────────
  const d100Calc = useMutation({
    mutationFn: (): Promise<D100Result> => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      return api.declarations.computeD100(activeCompanyId, dateFrom, dateTo, quarter, an, "0");
    },
    onSuccess: (r) => {
      if (!r.applicable) return;
      // Pre-fill the matching obligation row with D100's computed amount.
      setRows((prev) => {
        const next = [...prev];
        // Find an existing row for this cod_oblig, or create one.
        const existingIdx = next.findIndex((row) => row.codOblig === String(r.codOblig));
        const prefilledRow: ObligRow = {
          codOblig:  String(r.codOblig),
          codBugetar: "",   // user must fill
          scadenta:  r.scadenta ?? "",
          nrEvid:    "0",
          denOblig:  r.label ?? "",
          sumaDatI:  String(Math.round(parseFloat(r.sumaDatorata))),
          sumaDatC:  "", // ALWAYS user input — never auto-computed
          d100Basis: true,
        };
        if (existingIdx >= 0) {
          next[existingIdx] = { ...next[existingIdx], sumaDatI: prefilledRow.sumaDatI, d100Basis: true, denOblig: prefilledRow.denOblig, scadenta: prefilledRow.scadenta };
        } else {
          next.push(prefilledRow);
        }
        return next;
      });
      setHumanConfirmed(false);
      notify.success(t("declarations.d710.notify.d100Prefilled", { cod: r.codOblig }));
    },
    onError: (err) => notify.warn(formatError(err, t("declarations.d710.notify.d100Failed"))),
  });

  // ── Obligation rows ──────────────────────────────────────────────────────
  const [rows, setRows] = useState<ObligRow[]>([{ ...EMPTY_ROW }]);

  const updateRow = (idx: number, field: keyof ObligRow, value: string | boolean) => {
    setRows((prev) => {
      const next = [...prev];
      next[idx] = { ...next[idx], [field]: value };
      return next;
    });
    setHumanConfirmed(false);
  };

  const addRow    = () => { setRows((prev) => [...prev, { ...EMPTY_ROW }]); setHumanConfirmed(false); };
  const removeRow = (idx: number) => { setRows((prev) => prev.filter((_, i) => i !== idx)); setHumanConfirmed(false); };

  // ── Header fields ────────────────────────────────────────────────────────
  const [numeDeclar,    setNumeDeclar]    = useState("");
  const [prenumeDeclar, setPreumeDeclar]  = useState("");
  const [functieDeclar, setFunctieDeclar] = useState("Administrator");
  const [dAnulare,      setDAnulare]      = useState(0);
  const [rectificativa, setRectificativa] = useState(true);

  // ── Export state ─────────────────────────────────────────────────────────
  const [exporting,  setExporting]  = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [dukBlock,   setDukBlock]   = useState<PreflightIssue[] | null>(null);
  const [lastInput,  setLastInput]  = useState<D710Input | null>(null);

  // GUARDRAIL: human-review gate.
  const [humanConfirmed, setHumanConfirmed] = useState(false);

  // ── Validation: require sumaDatC for all rows ───────────────────────────
  const missingCorrected = useMemo(
    () => rows.filter((r) => r.codOblig && !r.sumaDatC).length,
    [rows],
  );

  const missingNoBasis = useMemo(
    () => rows.filter((r) => r.codOblig && !r.d100Basis).length,
    [rows],
  );

  // ── Build D710Input ──────────────────────────────────────────────────────

  const buildInput = (): D710Input | null => {
    if (!company) return null;
    const cif = company.cui.replace(/\D/g, "");
    const validRows = rows.filter((r) => r.codOblig && r.codBugetar && r.scadenta);
    if (validRows.length === 0) return null;
    return {
      header: {
        cui:           cif,
        den:           company.legalName,
        adresa:        company.address + ", " + company.city + ", " + company.county,
        luna,
        an,
        dAnulare,
        rectificativa,
        numeDeclar:    numeDeclar  || company.legalName,
        prenumeDeclar: prenumeDeclar || "-",
        functieDeclar: functieDeclar || "Administrator",
      },
      obligations: validRows.map(rowToObligation),
    };
  };

  // ── Preview ──────────────────────────────────────────────────────────────

  const handlePreview = async () => {
    const input = buildInput();
    if (!input) {
      notify.warn(!company ? t("declarations.notify.selectCompany") : t("declarations.d710.notify.noObligations"));
      return;
    }
    setPreviewing(true);
    try {
      const xml = await api.d710.previewXml(input);
      openXml({ xml, name: `d710-${dateFrom}.xml` });
    } catch (err) {
      notify.error(formatError(err, t("declarations.d710.previewFailed")));
    } finally {
      setPreviewing(false);
    }
  };

  // ── Export oficial ───────────────────────────────────────────────────────

  const handleExportOfficial = async (override = false) => {
    const input = buildInput();
    if (!input) {
      notify.warn(!company ? t("declarations.notify.selectCompany") : t("declarations.d710.notify.noObligations"));
      return;
    }
    // GUARDRAIL: human confirmation.
    if (!humanConfirmed) {
      notify.warn(t("declarations.d710.gate.mustConfirm"));
      return;
    }
    // GUARDRAIL: require sumaDatC.
    if (missingCorrected > 0 && !override) {
      notify.error(t("declarations.d710.gate.missingCorrected", { count: missingCorrected }));
      return;
    }

    setLastInput(input);
    const destPath = await saveDialog({
      title:       t("declarations.dialogs.saveD710"),
      defaultPath: `d710-${dateFrom}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!destPath) return;

    setExporting(true);
    try {
      const res = await api.d710.exportXmlOfficial(input, destPath, override);
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("declarations.d710.notify.savedDuk",   { path: res.path })
          : t("declarations.d710.notify.savedNoDuk", { path: res.path }),
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d710.notify.exportFailed")));
    } finally {
      setExporting(false);
    }
  };

  // ── Render ───────────────────────────────────────────────────────────────

  const hasValidRows = rows.some((r) => r.codOblig && r.codBugetar && r.scadenta);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>

      {/* ── DUK block panel ────────────────────────────────────────────── */}
      {dukBlock && (
        <div>
          <PreflightPanel issues={dukBlock} />
          <button
            className="pill-btn"
            style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.4)" }}
            onClick={() => lastInput && void handleExportOfficial(true)}
          >
            {t("declarations.common.exportAnyway")}
          </button>
        </div>
      )}

      {/* ── Notă semantică ───────────────────────────────────────────── */}
      <div className="banner warn">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
        <span>
          <b>{t("declarations.d710.gate.semanticWarnTitle")}</b>
          {" — "}
          {t("declarations.d710.gate.semanticWarnBody")}
        </span>
      </div>

      {/* ── Avertisment obligații fără bază D100 ──────────────────── */}
      {missingNoBasis > 0 && (
        <div className="banner warn">
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
          <span>{t("declarations.d710.gate.noBasis", { count: missingNoBasis })}</span>
        </div>
      )}

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">D710 — {t("declarations.d710.title")}</div>
          <div className="spacer" />
          {/* Pre-fill from D100 */}
          <button
            className="pill-btn spin-btn"
            disabled={d100Calc.isPending || !activeCompanyId}
            onClick={() => d100Calc.mutate()}
            title={t("declarations.d710.prefillFromD100Title")}
          >
            <Ic name="sync" />
            {d100Calc.isPending ? t("declarations.d710.prefilling") : t("declarations.d710.prefillFromD100")}
          </button>
          <label className="chk-row" style={{ fontSize: 13, userSelect: "none" }}>
            <input
              type="checkbox"
              checked={rectificativa}
              onChange={(e) => { setRectificativa(e.target.checked); setHumanConfirmed(false); }}
            />
            <span>{t("declarations.common.rectificativa")}</span>
          </label>
          <button
            className="pill-btn"
            disabled={previewing || !hasValidRows}
            onClick={() => void handlePreview()}
          >
            <Ic name="eye" />
            {previewing ? t("declarations.d710.previewing") : t("declarations.d710.previewXml")}
          </button>
          <button
            className="btn-dark"
            disabled={exporting || !hasValidRows || !humanConfirmed || missingCorrected > 0}
            onClick={() => void handleExportOfficial(false)}
            title={!humanConfirmed ? t("declarations.d710.gate.mustConfirm") : missingCorrected > 0 ? t("declarations.d710.gate.missingCorrectedTitle") : undefined}
          >
            <Ic name="shield" />
            {exporting ? t("declarations.common.exporting") : t("declarations.common.exportXml")}
          </button>
        </div>

        {/* ── GUARDRAIL 4: Notă sume totale vs diferențe ────────────── */}
        <div style={{ padding: "10px 16px 0" }}>
          <div className="banner">
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
            <span>{t("declarations.d710.gate.totalNotDiff")}</span>
          </div>
        </div>

        {/* ── Antet declarant ─────────────────────────────────────────── */}
        <div style={{ padding: "14px 16px 0", display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 12 }}>
          <div className="field">
            <label>{t("declarations.d710.numeDeclar")}</label>
            <input className="input" value={numeDeclar} onChange={(e) => { setNumeDeclar(e.target.value); setHumanConfirmed(false); }} />
          </div>
          <div className="field">
            <label>{t("declarations.d710.prenumeDeclar")}</label>
            <input className="input" value={prenumeDeclar} onChange={(e) => { setPreumeDeclar(e.target.value); setHumanConfirmed(false); }} />
          </div>
          <div className="field">
            <label>{t("declarations.d710.functieDeclar")}</label>
            <input className="input" value={functieDeclar} onChange={(e) => { setFunctieDeclar(e.target.value); setHumanConfirmed(false); }} />
          </div>
          <div className="field">
            <label>{t("declarations.d710.dAnulare")}</label>
            <select className="input" value={dAnulare} onChange={(e) => { setDAnulare(Number(e.target.value)); setHumanConfirmed(false); }}>
              <option value={0}>{t("declarations.d710.dAnulareNo")}</option>
              <option value={1}>{t("declarations.d710.dAnulareYes")}</option>
            </select>
          </div>
        </div>

        {/* ── Tabel obligații cu diff ──────────────────────────────────── */}
        <div style={{ padding: "14px 16px 0" }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-2)", marginBottom: 8, textTransform: "uppercase", letterSpacing: ".05em" }}>
            {t("declarations.d710.obligationsTitle")}
          </div>
        </div>
        <table className="scr-table">
          <thead>
            <tr>
              <th style={{ width: 80 }}>{t("declarations.d710.colCodOblig")}</th>
              <th>{t("declarations.d710.colDenOblig")}</th>
              <th style={{ width: 90 }}>{t("declarations.d710.colCodBugetar")}</th>
              <th style={{ width: 100 }}>{t("declarations.d710.colScadenta")}</th>
              <th className="r" style={{ width: 120 }}>{t("declarations.d710.colSumaDatI")}</th>
              <th className="r" style={{ width: 120 }}>
                {t("declarations.d710.colSumaDatC")}
                {" "}<span style={{ color: "var(--red)", fontWeight: 700 }}>*</span>
              </th>
              <th className="r" style={{ width: 120 }}>{t("declarations.d710.colDiff")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i}>
                <td>
                  <input
                    className="input"
                    style={{ width: "100%", fontSize: 12 }}
                    value={row.codOblig}
                    placeholder="2"
                    onChange={(e) => updateRow(i, "codOblig", e.target.value)}
                  />
                </td>
                <td>
                  <input
                    className="input"
                    style={{ width: "100%", fontSize: 12 }}
                    value={row.denOblig}
                    placeholder={t("declarations.d710.denObligPlaceholder")}
                    onChange={(e) => updateRow(i, "denOblig", e.target.value)}
                  />
                </td>
                <td>
                  <input
                    className="input"
                    style={{ width: "100%", fontSize: 12 }}
                    value={row.codBugetar}
                    placeholder="0205"
                    onChange={(e) => updateRow(i, "codBugetar", e.target.value)}
                  />
                </td>
                <td>
                  <input
                    className="input"
                    style={{ width: "100%", fontSize: 12 }}
                    value={row.scadenta}
                    placeholder="ZZ.LL.AAAA"
                    onChange={(e) => updateRow(i, "scadenta", e.target.value)}
                  />
                </td>
                <td>
                  {/* (I) Initial — pre-filled from D100 if available, otherwise editable */}
                  {row.d100Basis ? (
                    <div style={{ textAlign: "right", fontFamily: "var(--mono)", fontSize: 12, padding: "0 4px", color: "var(--text-2)", position: "relative" }}>
                      {fmtLei(row.sumaDatI)}
                      <span title={t("declarations.d710.d100Sourced")} style={{ position: "absolute", right: -4, top: -2, fontSize: 10, color: "var(--green)" }}>D100</span>
                    </div>
                  ) : (
                    <input
                      className="input"
                      style={{ width: "100%", fontSize: 12, textAlign: "right" }}
                      value={row.sumaDatI}
                      placeholder="0"
                      inputMode="numeric"
                      onChange={(e) => updateRow(i, "sumaDatI", e.target.value)}
                    />
                  )}
                </td>
                <td>
                  {/* (C) Corrected — ALWAYS user input, REQUIRED */}
                  <input
                    className="input"
                    style={{
                      width: "100%", fontSize: 12, textAlign: "right",
                      borderColor: row.codOblig && !row.sumaDatC ? "var(--red)" : undefined,
                    }}
                    value={row.sumaDatC}
                    placeholder={t("declarations.d710.sumaCCorectataRequired")}
                    inputMode="numeric"
                    onChange={(e) => updateRow(i, "sumaDatC", e.target.value)}
                  />
                </td>
                <td style={{ textAlign: "right" }}>
                  <DiffCell initial={row.sumaDatI} corrected={row.sumaDatC} />
                </td>
                <td>
                  {rows.length > 1 && (
                    <button
                      type="button"
                      className="pill-btn"
                      style={{ height: 26, fontSize: 11.5, padding: "0 8px", color: "var(--red)" }}
                      onClick={() => removeRow(i)}
                    >
                      ×
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ padding: "8px 16px" }}>
          <button className="pill-btn" style={{ fontSize: 12 }} onClick={addRow}>
            + {t("declarations.d710.addRow")}
          </button>
        </div>

        {/* ── GUARDRAIL: missingCorrected warning ──────────────────── */}
        {missingCorrected > 0 && (
          <div style={{ padding: "0 16px 8px" }}>
            <div className="banner warn">
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
              <span>{t("declarations.d710.gate.missingCorrected", { count: missingCorrected })}</span>
            </div>
          </div>
        )}

        {/* ── GUARDRAIL 1: Human-review gate ──────────────────────── */}
        <div style={{ padding: "12px 16px", borderTop: "1px solid var(--line)" }}>
          <label
            className="chk-row"
            style={{
              fontSize: 13, userSelect: "none", cursor: "pointer",
              color: humanConfirmed ? "var(--green)" : "var(--text)",
            }}
          >
            <input
              type="checkbox"
              checked={humanConfirmed}
              onChange={(e) => setHumanConfirmed(e.target.checked)}
            />
            <span>
              <b>{t("declarations.d710.gate.confirmLabel")}</b>
              {" — "}
              {t("declarations.d710.gate.confirmDesc")}
            </span>
          </label>
          {!humanConfirmed && (
            <div style={{ marginTop: 6, fontSize: 12, color: "var(--amber)" }}>
              ⚠ {t("declarations.d710.gate.mustConfirm")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
