/**
 * D301View — Decont special de TVA (OPANAF 592/2016).
 *
 * GUARDRAILS implementate:
 * 1. Review-before-file gate: "Am verificat declarația" checkbox OBLIGATORIU înainte de export.
 *    DUK pass ≠ corectitudine semantică — contabilul validează manual.
 * 2. Tabel editable tip_operatie: clasificarea auto este SUGESTIE; contabilul o poate schimba
 *    (1=AIC bunuri / 2=transport noi / 3=accizabile / 4=servicii intracomunitare art.307(2)
 *    / 5=alte operațiuni). Notă vizibilă "Clasificare sugerată automat — verificați încadrarea".
 * 3. DUK block + "Exportă oricum" override (ca toate celelalte declarații oficiale).
 * 4. Exportul final folosește rândurile (posibil editate) din tabelul UI.
 *
 * Embedded în Reports page — Claude-Design classes (.scr-card / .scr-table / .banner / .field).
 */

import { useState, useCallback } from "react";
import { useMutation } from "@tanstack/react-query";
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
import type { D301Sectiune, PreflightIssue } from "@/lib/tauri";

interface Props {
  dateFrom: string;
  dateTo:   string;
}

// Tip operație labels — per nomenclator D301 (art. clasificare).
const TIP_LABELS: Record<number, string> = {
  1: "1 — AIC bunuri taxabile (art.268 alin.3 lit.c)",
  2: "2 — AIC mijloace transport noi (art.268 alin.3 lit.b)",
  3: "3 — AIC produse accizabile (art.268 alin.3 lit.d)",
  4: "4 — Servicii intracomunitare art.307(2)",
  5: "5 — Alte operațiuni taxare inversă (art.307(3),(5),(6))",
};

const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';
const IC_INFO =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

const fmtLei = (s: string) => {
  const n = parseFloat(s);
  return isNaN(n) ? s : n.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
};

export function D301View({ dateFrom, dateTo: _dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const openXml = useOpenXml();

  // Period from dateFrom: extract luna + an
  const luna = Number(dateFrom.slice(5, 7));
  const an   = Number(dateFrom.slice(0, 4));

  // Editable sectiuni (possibly modified by accountant from auto-aggregated).
  const [sectiuni, setSectiuni] = useState<D301Sectiune[]>([]);
  const [aggregated, setAggregated] = useState(false);

  // Export state
  const [exporting, setExporting]     = useState(false);
  const [previewing, setPreviewing]   = useState(false);
  const [dRec, setDRec]               = useState(0);
  const [dukBlock, setDukBlock]       = useState<PreflightIssue[] | null>(null);
  const [lastDestPath, setLastDestPath] = useState<string | null>(null);

  // GUARDRAIL: human-review gate — must be ticked before export is allowed.
  const [humanConfirmed, setHumanConfirmed] = useState(false);

  // ── Auto-agregare ────────────────────────────────────────────────────────

  const aggregateMut = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("declarations.notify.selectCompany"));
      return api.d301.aggregateRows(activeCompanyId, luna, an);
    },
    onSuccess: (rows) => {
      setSectiuni(rows);
      setAggregated(true);
      setHumanConfirmed(false); // reset gate whenever data changes
      setDukBlock(null);
      if (rows.length === 0) {
        notify.info(t("declarations.d301.notify.noRows"));
      } else {
        notify.success(t("declarations.d301.notify.aggregated", { count: rows.length }));
      }
    },
    onError: (err) => notify.error(formatError(err, t("declarations.d301.notify.aggregateFailed"))),
  });

  // ── Editare tip_operatie ─────────────────────────────────────────────────

  const updateTipOperatie = useCallback((idx: number, tip: number) => {
    setSectiuni((prev) => {
      const next = [...prev];
      next[idx] = { ...next[idx], tipOperatie: tip };
      return next;
    });
    setHumanConfirmed(false); // force re-confirmation after edit
  }, []);

  const removeRow = useCallback((idx: number) => {
    setSectiuni((prev) => prev.filter((_, i) => i !== idx));
    setHumanConfirmed(false);
  }, []);

  // ── Preview XML ──────────────────────────────────────────────────────────

  const handlePreview = async () => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (sectiuni.length === 0) { notify.info(t("declarations.d301.notify.noRows")); return; }
    setPreviewing(true);
    try {
      const xml = await api.d301.previewXml(activeCompanyId, luna, an, dRec, { sectiuni });
      openXml({ xml, name: `d301-${dateFrom}.xml` });
    } catch (err) {
      notify.error(formatError(err, t("declarations.d301.previewFailed")));
    } finally {
      setPreviewing(false);
    }
  };

  // ── Export oficial (cu gate DUK + gate confirmare umană) ─────────────────

  const handleExportOfficial = async (override = false) => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (sectiuni.length === 0) { notify.info(t("declarations.d301.notify.noRows")); return; }

    // GUARDRAIL: human confirmation required before ANY write.
    if (!humanConfirmed) {
      notify.warn(t("declarations.d301.gate.mustConfirm"));
      return;
    }

    let destPath = lastDestPath;
    if (!destPath || !override) {
      // On first call (or non-override retry) ask for save location.
      const chosen = await saveDialog({
        title:       t("declarations.dialogs.saveD301"),
        defaultPath: `d301-${dateFrom}.xml`,
        filters:     [{ name: "XML", extensions: ["xml"] }],
      });
      if (!chosen) return;
      destPath = chosen;
      setLastDestPath(chosen);
    }

    setExporting(true);
    try {
      const res = await api.d301.exportXmlOfficial(
        activeCompanyId, luna, an, dRec,
        { sectiuni }, destPath, override,
      );
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("declarations.d301.notify.savedDuk",   { path: res.path })
          : t("declarations.d301.notify.savedNoDuk", { path: res.path }),
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d301.notify.exportFailed")));
    } finally {
      setExporting(false);
    }
  };

  // ── Render ───────────────────────────────────────────────────────────────

  const hasRows = sectiuni.length > 0;
  const totalBaza = sectiuni.reduce((s, r) => s + parseFloat(r.baza || "0"), 0);
  const totalTva  = sectiuni.reduce((s, r) => s + parseFloat(r.tva  || "0"), 0);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>

      {/* ── Preflight / DUK block ─────────────────────────────────────────── */}
      {dukBlock && (
        <div>
          <PreflightPanel issues={dukBlock} />
          <button
            className="pill-btn"
            style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.4)" }}
            onClick={() => void handleExportOfficial(true)}
          >
            {t("declarations.common.exportAnyway")}
          </button>
        </div>
      )}

      {/* ── Notă: DUK ≠ corectitudine semantică ─────────────────────────── */}
      <div className="banner warn">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
        <span>
          <b>{t("declarations.d301.gate.semanticWarnTitle")}</b>
          {" — "}
          {t("declarations.d301.gate.semanticWarnBody")}
        </span>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">D301 — {t("declarations.d301.title")}</div>
          <div className="spacer" />
          {/* Rectificativă toggle */}
          <label className="chk-row" style={{ fontSize: 13, userSelect: "none" }}>
            <input
              type="checkbox"
              checked={dRec === 1}
              onChange={(e) => { setDRec(e.target.checked ? 1 : 0); setHumanConfirmed(false); }}
            />
            <span>{t("declarations.common.rectificativa")}</span>
          </label>
          <button
            className="pill-btn spin-btn"
            disabled={aggregateMut.isPending || !activeCompanyId}
            onClick={() => aggregateMut.mutate()}
          >
            <Ic name="sync" />
            {aggregateMut.isPending ? t("declarations.d301.aggregating") : t("declarations.d301.aggregate")}
          </button>
          <button
            className="pill-btn"
            disabled={previewing || !hasRows}
            onClick={() => void handlePreview()}
          >
            <Ic name="eye" />
            {previewing ? t("declarations.d301.previewing") : t("declarations.d301.previewXml")}
          </button>
          <button
            className="btn-dark"
            disabled={exporting || !hasRows || !humanConfirmed}
            onClick={() => void handleExportOfficial(false)}
            title={!humanConfirmed ? t("declarations.d301.gate.mustConfirm") : undefined}
          >
            <Ic name="shield" />
            {exporting ? t("declarations.common.exporting") : t("declarations.common.exportXml")}
          </button>
        </div>

        {/* ── Notă clasificare auto ────────────────────────────────────────── */}
        {aggregated && hasRows && (
          <div style={{ padding: "10px 16px 0" }}>
            <div className="banner">
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_INFO }} />
              <span>{t("declarations.d301.gate.classificationNote")}</span>
            </div>
          </div>
        )}

        {!aggregated ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("declarations.d301.empty")}
          </div>
        ) : sectiuni.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("declarations.d301.emptyAfterAggregate")}
          </div>
        ) : (
          <>
            {/* Tabel editable tip_operatie — GUARDRAIL 2 */}
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 320 }}>{t("declarations.d301.colTipOperatie")}</th>
                  <th>{t("declarations.d301.colNrDoc")}</th>
                  <th>{t("declarations.d301.colDataDoc")}</th>
                  <th>{t("declarations.d301.colTipValuta")}</th>
                  <th className="r">{t("declarations.d301.colBaza")}</th>
                  <th className="r">{t("declarations.d301.colTva")}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {sectiuni.map((s, i) => (
                  <tr key={i}>
                    <td>
                      {/* Editable select — accountant changes classification */}
                      <select
                        className="input"
                        style={{ fontSize: 12, height: 28, width: "100%" }}
                        value={s.tipOperatie}
                        onChange={(e) => updateTipOperatie(i, Number(e.target.value))}
                      >
                        {[1, 2, 3, 4, 5].map((tip) => (
                          <option key={tip} value={tip}>{TIP_LABELS[tip]}</option>
                        ))}
                      </select>
                    </td>
                    <td className="doc">{s.nrDoc}</td>
                    <td className="num">{s.dataDoc}</td>
                    <td><span className="chip sent">{s.tipValuta}</span></td>
                    <td className="r num">{fmtLei(s.baza)}</td>
                    <td className="r num">{fmtLei(s.tva)}</td>
                    <td>
                      <button
                        type="button"
                        className="pill-btn"
                        style={{ height: 26, fontSize: 11.5, padding: "0 8px", color: "var(--red)" }}
                        onClick={() => removeRow(i)}
                        title={t("declarations.d301.removeRow")}
                      >
                        ×
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="tot-foot">
              <span>{t("declarations.d301.totalRows", { count: sectiuni.length })}</span>
              <span>{t("declarations.d301.colBaza")} <b className="num">{fmtLei(totalBaza.toFixed(2))}</b></span>
              <span>{t("declarations.d301.colTva")} <b className="num">{fmtLei(totalTva.toFixed(2))}</b></span>
            </div>
          </>
        )}

        {/* ── GUARDRAIL 1: Human-review gate ───────────────────────────────── */}
        {hasRows && (
          <div style={{ padding: "12px 16px", borderTop: "1px solid var(--line)", marginTop: 4 }}>
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
                <b>{t("declarations.d301.gate.confirmLabel")}</b>
                {" — "}
                {t("declarations.d301.gate.confirmDesc")}
              </span>
            </label>
            {!humanConfirmed && (
              <div style={{ marginTop: 6, fontSize: 12, color: "var(--amber)" }}>
                ⚠ {t("declarations.d301.gate.mustConfirm")}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
