/**
 * D700View — Declarație de înregistrare/mențiuni/radiere (OPANAF 15/2026, ed. 0126).
 *
 * GUARDRAILS implementate:
 * 1. Review-before-file gate: "Am verificat declarația" checkbox OBLIGATORIU înainte de export.
 *    DUK pass ≠ corectitudine semantică — contabilul validează manual.
 * 2. D700 este strict USER-DRIVEN: formularul nu derivă automat mențiunile din date contabile.
 *    Notă prominentă: "Completați mențiunile manual — D700 NU se generează automat din date."
 * 3. DUK block + "Exportă oricum" override.
 *
 * Embedded în Reports page — Claude-Design classes (.scr-card / .scr-table / .banner / .field).
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
import type { D700Input, TvaMentiune, RegimFiscalMentiune, PreflightIssue } from "@/lib/tauri";

const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

const TVA_MENTIUNI: TvaMentiune[] = [
  "inregistrare",
  "anulare",
  "trecerea_la_lunar",
  "trecerea_la_trimestrial",
  "tva_la_incasare_inregistrare",
  "tva_la_incasare_anulare",
];

const REGIM_MENTIUNI: RegimFiscalMentiune[] = [
  "trecerea_la_micro",
  "trecerea_la_profit",
  "modificare_frecventa_profit_lunar",
  "modificare_frecventa_profit_trimestrial",
];

export function D700View({ dateFrom: _dateFrom }: { dateFrom: string; dateTo: string }) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const openXml = useOpenXml();

  // Fetch active company for pre-filling sect A.
  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn:  () => api.companies.get(activeCompanyId!),
    enabled:  !!activeCompanyId,
  });

  // ── Form state: Secțiunea A (identificare) ──────────────────────────────
  const [reprNume,     setReprNume]     = useState("");
  const [reprPrenume,  setReprPrenume]  = useState("");
  const [reprFunctie,  setReprFunctie]  = useState("Administrator");
  const [judetCod,     setJudetCod]     = useState("");
  const [formaJuridica, setFormaJuridica] = useState("SRL");

  // ── Form state: Secțiunea B (vector fiscal) — USER-DRIVEN ───────────────
  const [tvaMentiune,      setTvaMentiune]      = useState<TvaMentiune | "">("");
  const [tvaData,          setTvaData]          = useState("");
  const [regimFiscal,      setRegimFiscal]      = useState<RegimFiscalMentiune | "">("");
  const [regimFiscalData,  setRegimFiscalData]  = useState("");

  // ── Form state: Secțiunea C (sedii) ─────────────────────────────────────
  // (simplified: domiciliu fiscal nou)
  const [domiciliuFiscalNou, setDomiciliuFiscalNou] = useState("");

  // ── Form state: Secțiunea D (radiere) ───────────────────────────────────
  const [motivRadiere,  setMotivRadiere]  = useState("");
  const [dataRadiere,   setDataRadiere]   = useState("");

  // ── Declarant ────────────────────────────────────────────────────────────
  const [dRec, setDRec] = useState(0);

  // ── Export state ─────────────────────────────────────────────────────────
  const [exporting,  setExporting]  = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [dukBlock,   setDukBlock]   = useState<PreflightIssue[] | null>(null);
  const [lastInput,  setLastInput]  = useState<D700Input | null>(null);

  // GUARDRAIL: human-review gate.
  const [humanConfirmed, setHumanConfirmed] = useState(false);

  // ── Build D700Input from form ────────────────────────────────────────────

  const buildInput = (): D700Input | null => {
    if (!company) return null;
    const cif = company.cui.replace(/\D/g, "");
    return {
      dRec,
      sectA: {
        cui:         cif,
        den:         company.legalName,
        adresa:      company.address + ", " + company.city + ", " + company.county,
        judetCod:    judetCod || "B",
        formaJuridica: formaJuridica || "SRL",
        reprNume:    reprNume,
        reprPrenume: reprPrenume,
        reprFunctie: reprFunctie || "Administrator",
        telefon:     company.phone ?? undefined,
        email:       company.email ?? undefined,
      },
      sectB: (tvaMentiune || regimFiscal) ? {
        tvaMentiune:       tvaMentiune     || undefined,
        tvaData:           tvaData         || undefined,
        regimFiscal:       regimFiscal     || undefined,
        regimFiscalData:   regimFiscalData || undefined,
      } : undefined,
      sectC: domiciliuFiscalNou ? { domiciliuFiscalNou } : undefined,
      sectD: motivRadiere ? { motiv: motivRadiere, dataRadiere: dataRadiere || undefined } : undefined,
    };
  };

  // ── Preview ──────────────────────────────────────────────────────────────

  const handlePreview = async () => {
    const input = buildInput();
    if (!input) { notify.warn(t("declarations.notify.selectCompany")); return; }
    setPreviewing(true);
    try {
      const xml = await api.d700.previewXml(input);
      openXml({ xml, name: "d700.xml" });
    } catch (err) {
      notify.error(formatError(err, t("declarations.d700.previewFailed")));
    } finally {
      setPreviewing(false);
    }
  };

  // ── Export oficial ───────────────────────────────────────────────────────

  const handleExportOfficial = async (override = false) => {
    const input = buildInput();
    if (!input) { notify.warn(t("declarations.notify.selectCompany")); return; }

    // GUARDRAIL: human confirmation.
    if (!humanConfirmed) {
      notify.warn(t("declarations.d700.gate.mustConfirm"));
      return;
    }

    setLastInput(input);
    const destPath = await saveDialog({
      title:       t("declarations.dialogs.saveD700"),
      defaultPath: "d700.xml",
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!destPath) return;

    setExporting(true);
    try {
      const res = await api.d700.exportXmlOfficial(input, destPath, override);
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("declarations.d700.notify.savedDuk",   { path: res.path })
          : t("declarations.d700.notify.savedNoDuk", { path: res.path }),
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d700.notify.exportFailed")));
    } finally {
      setExporting(false);
    }
  };

  // ── Render ───────────────────────────────────────────────────────────────

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

      {/* ── GUARDRAIL 2: Notă prominentă — D700 este USER-DRIVEN ──────── */}
      <div className="banner warn">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
        <span>
          <b>{t("declarations.d700.gate.userDrivenTitle")}</b>
          {" — "}
          {t("declarations.d700.gate.userDrivenBody")}
        </span>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">D700 — {t("declarations.d700.title")}</div>
          <div className="spacer" />
          <label className="chk-row" style={{ fontSize: 13, userSelect: "none" }}>
            <input
              type="checkbox"
              checked={dRec === 1}
              onChange={(e) => { setDRec(e.target.checked ? 1 : 0); setHumanConfirmed(false); }}
            />
            <span>{t("declarations.common.rectificativa")}</span>
          </label>
          <button
            className="pill-btn"
            disabled={previewing || !company}
            onClick={() => void handlePreview()}
          >
            <Ic name="eye" />
            {previewing ? t("declarations.d700.previewing") : t("declarations.d700.previewXml")}
          </button>
          <button
            className="btn-dark"
            disabled={exporting || !company || !humanConfirmed}
            onClick={() => void handleExportOfficial(false)}
            title={!humanConfirmed ? t("declarations.d700.gate.mustConfirm") : undefined}
          >
            <Ic name="shield" />
            {exporting ? t("declarations.common.exporting") : t("declarations.common.exportXml")}
          </button>
        </div>

        <div style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 20 }}>

          {/* ── Secțiunea A — Identificare (pre-filled from company) ──── */}
          <div>
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-2)", marginBottom: 10, textTransform: "uppercase", letterSpacing: ".05em" }}>
              {t("declarations.d700.sectATitle")}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 12 }}>
              <div className="field">
                <label>{t("declarations.d700.reprNume")}</label>
                <input className="input" value={reprNume} onChange={(e) => { setReprNume(e.target.value); setHumanConfirmed(false); }} />
              </div>
              <div className="field">
                <label>{t("declarations.d700.reprPrenume")}</label>
                <input className="input" value={reprPrenume} onChange={(e) => { setReprPrenume(e.target.value); setHumanConfirmed(false); }} />
              </div>
              <div className="field">
                <label>{t("declarations.d700.reprFunctie")}</label>
                <input className="input" value={reprFunctie} onChange={(e) => { setReprFunctie(e.target.value); setHumanConfirmed(false); }} />
              </div>
              <div className="field">
                <label>{t("declarations.d700.judetCod")}</label>
                <input className="input" value={judetCod} placeholder="B" maxLength={2} onChange={(e) => { setJudetCod(e.target.value.toUpperCase()); setHumanConfirmed(false); }} />
              </div>
              <div className="field">
                <label>{t("declarations.d700.formaJuridica")}</label>
                <input className="input" value={formaJuridica} placeholder="SRL" onChange={(e) => { setFormaJuridica(e.target.value); setHumanConfirmed(false); }} />
              </div>
            </div>
          </div>

          {/* ── Secțiunea B — Vector fiscal (USER INPUT ONLY) ─────────── */}
          <div>
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-2)", marginBottom: 10, textTransform: "uppercase", letterSpacing: ".05em" }}>
              {t("declarations.d700.sectBTitle")}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 12 }}>
              <div className="field" style={{ gridColumn: "1 / 3" }}>
                <label>{t("declarations.d700.tvaMentiune")}</label>
                <select
                  className="input"
                  value={tvaMentiune}
                  onChange={(e) => { setTvaMentiune(e.target.value as TvaMentiune | ""); setHumanConfirmed(false); }}
                >
                  <option value="">— {t("declarations.d700.none")} —</option>
                  {TVA_MENTIUNI.map((m) => (
                    <option key={m} value={m}>{t(`declarations.d700.tvaOptions.${m}`)}</option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>{t("declarations.d700.tvaData")}</label>
                <input className="input" type="date" value={tvaData} onChange={(e) => { setTvaData(e.target.value); setHumanConfirmed(false); }} />
              </div>
              <div className="field" style={{ gridColumn: "1 / 3" }}>
                <label>{t("declarations.d700.regimFiscal")}</label>
                <select
                  className="input"
                  value={regimFiscal}
                  onChange={(e) => { setRegimFiscal(e.target.value as RegimFiscalMentiune | ""); setHumanConfirmed(false); }}
                >
                  <option value="">— {t("declarations.d700.none")} —</option>
                  {REGIM_MENTIUNI.map((m) => (
                    <option key={m} value={m}>{t(`declarations.d700.regimOptions.${m}`)}</option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>{t("declarations.d700.regimFiscalData")}</label>
                <input className="input" type="date" value={regimFiscalData} onChange={(e) => { setRegimFiscalData(e.target.value); setHumanConfirmed(false); }} />
              </div>
            </div>
          </div>

          {/* ── Secțiunea C — Domiciliu fiscal ──────────────────────── */}
          <div>
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-2)", marginBottom: 10, textTransform: "uppercase", letterSpacing: ".05em" }}>
              {t("declarations.d700.sectCTitle")}
            </div>
            <div className="field">
              <label>{t("declarations.d700.domiciliuFiscalNou")}</label>
              <input
                className="input"
                value={domiciliuFiscalNou}
                placeholder={t("declarations.d700.domiciliuPlaceholder")}
                onChange={(e) => { setDomiciliuFiscalNou(e.target.value); setHumanConfirmed(false); }}
              />
            </div>
          </div>

          {/* ── Secțiunea D — Radiere ───────────────────────────────── */}
          <div>
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-2)", marginBottom: 10, textTransform: "uppercase", letterSpacing: ".05em" }}>
              {t("declarations.d700.sectDTitle")}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr", gap: 12 }}>
              <div className="field">
                <label>{t("declarations.d700.motivRadiere")}</label>
                <input
                  className="input"
                  value={motivRadiere}
                  placeholder={t("declarations.d700.motivPlaceholder")}
                  onChange={(e) => { setMotivRadiere(e.target.value); setHumanConfirmed(false); }}
                />
              </div>
              <div className="field">
                <label>{t("declarations.d700.dataRadiere")}</label>
                <input className="input" type="date" value={dataRadiere} onChange={(e) => { setDataRadiere(e.target.value); setHumanConfirmed(false); }} />
              </div>
            </div>
          </div>

          {/* ── GUARDRAIL 1: Human-review gate ──────────────────────── */}
          <div style={{ paddingTop: 8, borderTop: "1px solid var(--line)" }}>
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
                <b>{t("declarations.d700.gate.confirmLabel")}</b>
                {" — "}
                {t("declarations.d700.gate.confirmDesc")}
              </span>
            </label>
            {!humanConfirmed && (
              <div style={{ marginTop: 6, fontSize: 12, color: "var(--amber)" }}>
                ⚠ {t("declarations.d700.gate.mustConfirm")}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
