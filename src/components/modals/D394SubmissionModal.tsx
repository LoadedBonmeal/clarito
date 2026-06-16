/**
 * D394SubmissionModal — colectează câmpurile D394Submission necesare pentru exportul
 * oficial ANAF (schema v5). Câmpurile stabile (telefon, reprezentant) sunt
 * inițializate cu valori sensibile.
 *
 * Design re-skin: .modal-back/.modal + .fgrid/.field + .cbx check-rows
 * (pattern din src/pages/Receipts.tsx). Toată logica de validare/submit păstrată.
 */

import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import type { Company, D394CashRow, D394Submission } from "@/types";

// ── Constants ─────────────────────────────────────────────────────────────────

const TIP_D394_KEYS = [
  { value: "L", labelKey: "shared.declCommon.periodL" },
  { value: "T", labelKey: "shared.declCommon.periodT" },
  { value: "S", labelKey: "shared.declCommon.periodS" },
  { value: "A", labelKey: "shared.declCommon.periodA" },
];

/** Cotele TVA active 2026 pentru cartuș G/I (9% = tranzitoriu locuințe). */
const COTE_2026 = [21, 11, 9] as const;

/** Cele 14 sume (lei întregi) introduse manual pe cotă. */
const CASH_FIELDS = [
  "bazaI1", "tvaI1", "bazaI2", "tvaI2",
  "bazaFsl", "tvaFsl", "bazaFslCod", "tvaFslCod",
  "bazaFsa", "tvaFsa", "bazaFsai", "tvaFsai", "bazaBfai", "tvaBfai",
] as const;
type CashField = (typeof CASH_FIELDS)[number];
type CashRowStr = Record<CashField, string>;
const emptyCashRow = (): CashRowStr =>
  Object.fromEntries(CASH_FIELDS.map((f) => [f, ""])) as CashRowStr;

// ── Props ─────────────────────────────────────────────────────────────────────

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  company: Company;
  onSubmit: (submission: D394Submission) => void;
  /**
   * Optional — when provided, a „Vizualizează / Editează XML" button appears in
   * the footer that builds the OFFICIAL D394 XML from the current submission and
   * opens it in the in-app viewer/editor (no write, no DUK). The modal stays open.
   */
  onPreview?: (submission: D394Submission) => void | Promise<void>;
  previewing?: boolean;
}

// ── CheckRow — design .cbx + label, full-row clickable ────────────────────────

function CheckRow({
  checked,
  onChange,
  children,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 9,
        fontSize: 13,
        color: "var(--text)",
        cursor: "pointer",
        background: "transparent",
        border: 0,
        padding: 0,
        fontFamily: "inherit",
        textAlign: "left",
      }}
    >
      <span className={`cbx${checked ? " on" : ""}`} />
      {children}
    </button>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export function D394SubmissionModal({ open, onOpenChange, company, onSubmit, onPreview, previewing }: Props) {
  const { t } = useTranslation();
  const [tipD394,          setTipD394]          = useState("L");
  const [sistemTva,        setSistemTva]        = useState(false);
  const [opEfectuate,      setOpEfectuate]      = useState(false);
  const [caen,             setCaen]             = useState("6201");
  const [telefon,          setTelefon]          = useState(company.phone ?? "0000000");
  // Reprezentant
  const [denR,             setDenR]             = useState(company.legalName ?? "");
  const [functieReprez,    setFunctieReprez]    = useState("DIRECTOR");
  const [adresaR,          setAdresaR]          = useState(company.address ?? "");
  // Întocmit
  const [tipIntocmit,      setTipIntocmit]      = useState(0);
  const [denIntocmit,      setDenIntocmit]      = useState(company.legalName ?? "");
  const [cifIntocmit,      setCifIntocmit]      = useState("0");
  const [calitateIntocmit, setCalitateIntocmit] = useState("Reprezentant");
  // Alte flag-uri
  const [optiune,          setOptiune]          = useState(false);
  const [prsAfiliat,       setPrsAfiliat]       = useState(false);
  const [solicit,          setSolicit]          = useState(false);
  // Cartuș G (încasări AMEF) + facturi simplificate — totaluri manuale pe cotă (opțional).
  const [showCash,         setShowCash]         = useState(false);
  const [nrBfI1,           setNrBfI1]           = useState("0");
  const [cash,             setCash]             = useState<Record<number, CashRowStr>>(
    () => Object.fromEntries(COTE_2026.map((c) => [c, emptyCashRow()])) as Record<number, CashRowStr>,
  );
  const setCashField = (cota: number, f: CashField, v: string) =>
    setCash((prev) => ({ ...prev, [cota]: { ...prev[cota], [f]: v.replace(/\D/g, "") } }));
  const cashNum = (cota: number, f: CashField) => Number(cash[cota][f]) || 0;
  // Sumele-total Î1/Î2 se calculează din rânduri (regula DUK) — afișate pentru reconciliere.
  const incasariI1 = COTE_2026.reduce((s, c) => s + cashNum(c, "bazaI1") + cashNum(c, "tvaI1"), 0);
  const incasariI2 = COTE_2026.reduce((s, c) => s + cashNum(c, "bazaI2") + cashNum(c, "tvaI2"), 0);

  const { closing, close } = useAnimatedClose(useCallback(() => onOpenChange(false), [onOpenChange]));

  // Esc closes the modal.
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [open, close]);

  // ── Validation ───────────────────────────────────────────────────────────────

  const caenValid = /^\d{4}$/.test(caen.trim());
  const canSubmit = caenValid && denR.trim() !== "" && denIntocmit.trim() !== "";

  // ── Submit ───────────────────────────────────────────────────────────────────

  const buildSubmission = (): D394Submission => ({
    tipD394,
    sistemTva,
    opEfectuate,
    caen:             caen.trim(),
    telefon:          telefon.trim(),
    denR:             denR.trim(),
    functieReprez:    functieReprez.trim(),
    adresaR:          adresaR.trim(),
    tipIntocmit,
    denIntocmit:      denIntocmit.trim(),
    cifIntocmit:      Number(cifIntocmit) || 0,
    calitateIntocmit: tipIntocmit === 0 ? calitateIntocmit.trim() || null : null,
    optiune,
    prsAfiliat,
    solicit,
    nrBfI1: Number(nrBfI1) || 0,
    // Un rând per cotă cu cel puțin o sumă > 0; cotele goale sunt omise.
    cashRows: COTE_2026.map((cota): D394CashRow => ({
      cota,
      bazaI1: cashNum(cota, "bazaI1"), tvaI1: cashNum(cota, "tvaI1"),
      bazaI2: cashNum(cota, "bazaI2"), tvaI2: cashNum(cota, "tvaI2"),
      bazaFsl: cashNum(cota, "bazaFsl"), tvaFsl: cashNum(cota, "tvaFsl"),
      bazaFslCod: cashNum(cota, "bazaFslCod"), tvaFslCod: cashNum(cota, "tvaFslCod"),
      bazaFsa: cashNum(cota, "bazaFsa"), tvaFsa: cashNum(cota, "tvaFsa"),
      bazaFsai: cashNum(cota, "bazaFsai"), tvaFsai: cashNum(cota, "tvaFsai"),
      bazaBfai: cashNum(cota, "bazaBfai"), tvaBfai: cashNum(cota, "tvaBfai"),
    })).filter((r) => CASH_FIELDS.some((f) => (r[f] ?? 0) > 0)),
  });

  const handleSubmit = () => {
    if (!canSubmit) return;
    onSubmit(buildSubmission());
    close();
  };

  // Build the official XML from the current submission and open it in the in-app
  // viewer/editor — the modal stays open so the user can tweak + re-preview.
  const handlePreview = () => {
    if (!canSubmit || !onPreview) return;
    void onPreview(buildSubmission());
  };

  // A compact per-cotă table of whole-lei number inputs (cartuș G/I).
  const cashTable = (title: string, cols: { label: string; f: CashField }[]) => (
    <div className="span2" style={{ marginTop: 2 }}>
      <div style={{ fontSize: 11.5, color: "var(--text-2)", margin: "2px 0 4px" }}>{title}</div>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
        <thead>
          <tr style={{ color: "var(--text-2)" }}>
            <th style={{ textAlign: "left", fontWeight: 500, padding: "2px 4px" }}>Cotă</th>
            {cols.map((c) => (
              <th key={c.f} style={{ textAlign: "right", fontWeight: 500, padding: "2px 4px" }}>{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {COTE_2026.map((cota) => (
            <tr key={cota}>
              <td style={{ padding: "2px 4px", whiteSpace: "nowrap" }}>{cota}%</td>
              {cols.map((c) => (
                <td key={c.f} style={{ padding: "2px 4px", textAlign: "right" }}>
                  <input
                    className="input num"
                    style={{ width: 76, padding: "4px 6px", fontSize: 12 }}
                    value={cash[cota][c.f]}
                    onChange={(e) => setCashField(cota, c.f, e.target.value)}
                    placeholder="0"
                    inputMode="numeric"
                  />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );

  if (!open) return null;

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div className="modal" style={{ width: 640 }}>
        <div className="modal-head">
          <div>
            <div className="mt">{t("shared.d394.title")}</div>
            <div className="ms">{t("shared.d394.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={() => close()}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            {/* Tip declarație + CAEN */}
            <div className="field">
              <label>{t("shared.d394.tip")}</label>
              <select
                id="d394-tip"
                className="select"
                value={tipD394}
                onChange={(e) => setTipD394(e.target.value)}
              >
                {TIP_D394_KEYS.map((o) => (
                  <option key={o.value} value={o.value}>{t(o.labelKey)}</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>{t("shared.declCommon.caenLabel")} <span className="req">*</span></label>
              <input
                id="d394-caen"
                className={`input num${caen.length > 0 && !caenValid ? " invalid" : ""}`}
                value={caen}
                onChange={(e) => setCaen(e.target.value)}
                placeholder="6201"
                maxLength={4}
              />
              {caen.length > 0 && !caenValid && (
                <span className="err">
                  <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M6 18 18 6M6 6l12 12"/>' }} />
                  {t("shared.declCommon.caenError")}
                </span>
              )}
            </div>

            <div className="field span2">
              <label>{t("shared.d394.phone")}</label>
              <input
                id="d394-telefon"
                className="input num"
                value={telefon}
                onChange={(e) => setTelefon(e.target.value)}
                placeholder="0721000000"
                maxLength={15}
              />
            </div>

            {/* Reprezentant */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              {t("shared.d394.repTitle")}
            </div>
            <div className="field span2">
              <label>{t("shared.d394.repName")} <span className="req">*</span></label>
              <input
                id="d394-denr"
                className="input"
                value={denR}
                onChange={(e) => setDenR(e.target.value)}
                placeholder={t("shared.d394.ph.repName")}
              />
            </div>
            <div className="field">
              <label>{t("shared.d394.repRole")}</label>
              <input
                id="d394-functie"
                className="input"
                value={functieReprez}
                onChange={(e) => setFunctieReprez(e.target.value)}
                placeholder={t("shared.d394.ph.repRole")}
              />
            </div>
            <div className="field">
              <label>{t("shared.d394.capacity")}</label>
              <input
                id="d394-calitate"
                className="input"
                value={calitateIntocmit}
                onChange={(e) => setCalitateIntocmit(e.target.value)}
                placeholder={t("shared.d394.ph.capacity")}
                disabled={tipIntocmit !== 0}
                style={tipIntocmit !== 0 ? { opacity: 0.55, background: "var(--fill)" } : undefined}
              />
            </div>
            <div className="field span2">
              <label>{t("shared.d394.repAddress")}</label>
              <input
                id="d394-adresa"
                className="input"
                value={adresaR}
                onChange={(e) => setAdresaR(e.target.value)}
                placeholder={t("shared.d394.ph.repAddress")}
              />
            </div>

            {/* Întocmit */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              {t("shared.d394.preparerTitle")}
            </div>
            <div className="field">
              <label>{t("shared.d394.preparerType")}</label>
              <select
                id="d394-tipintocmit"
                className="select"
                value={String(tipIntocmit)}
                onChange={(e) => setTipIntocmit(Number(e.target.value))}
              >
                <option value="0">{t("shared.d394.preparerType0")}</option>
                <option value="1">{t("shared.d394.preparerType1")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("shared.d394.preparerCif")}</label>
              <input
                id="d394-cifintocmit"
                className="input num"
                value={cifIntocmit}
                onChange={(e) => setCifIntocmit(e.target.value)}
                placeholder="0"
              />
            </div>
            <div className="field span2">
              <label>{t("shared.d394.preparerName")} <span className="req">*</span></label>
              <input
                id="d394-denintocmit"
                className="input"
                value={denIntocmit}
                onChange={(e) => setDenIntocmit(e.target.value)}
                placeholder={t("shared.d394.ph.preparerName")}
              />
            </div>

            {/* Flags */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              {t("shared.declCommon.flagsTitle")}
            </div>
            <div className="field span2" style={{ gap: 9 }}>
              <CheckRow checked={sistemTva} onChange={setSistemTva}>
                {t("shared.d394.flags.sistemTva")}
              </CheckRow>
              <CheckRow checked={opEfectuate} onChange={setOpEfectuate}>
                {t("shared.d394.flags.opEfectuate")}
              </CheckRow>
              <CheckRow checked={optiune} onChange={setOptiune}>
                {t("shared.d394.flags.optiune")}
              </CheckRow>
              <CheckRow checked={prsAfiliat} onChange={setPrsAfiliat}>
                Persoane afiliate
              </CheckRow>
              <CheckRow checked={solicit} onChange={setSolicit}>
                {t("shared.declCommon.vatRefund")}
              </CheckRow>
            </div>

            {/* Cartuș G (încasări AMEF) + facturi simplificate — opțional, introdus manual pe cotă */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              {t("shared.d394.cashTitle")}
            </div>
            <div className="field span2" style={{ gap: 9 }}>
              <CheckRow checked={showCash} onChange={setShowCash}>
                {t("shared.d394.cashToggle")}
              </CheckRow>
            </div>
            {showCash && (
              <>
                <div className="field span2">
                  <span className="hint">{t("shared.d394.cashHint")}</span>
                </div>
                <div className="field">
                  <label>{t("shared.d394.nrBf")}</label>
                  <input
                    className="input num"
                    value={nrBfI1}
                    onChange={(e) => setNrBfI1(e.target.value.replace(/\D/g, ""))}
                    placeholder="0"
                    inputMode="numeric"
                  />
                </div>
                <div className="field" />
                {cashTable(t("shared.d394.amefTitle"), [
                  { label: "Î1 bază", f: "bazaI1" },
                  { label: "Î1 TVA", f: "tvaI1" },
                  { label: "Î2 bază", f: "bazaI2" },
                  { label: "Î2 TVA", f: "tvaI2" },
                ])}
                <div className="field span2">
                  <span className="hint">
                    {t("shared.d394.amefTotals", { i1: incasariI1, i2: incasariI2 })}
                  </span>
                </div>
                {cashTable(t("shared.d394.fsTitle"), [
                  { label: "FSL bază", f: "bazaFsl" },
                  { label: "FSL TVA", f: "tvaFsl" },
                  { label: "cu cod bază", f: "bazaFslCod" },
                  { label: "cu cod TVA", f: "tvaFslCod" },
                ])}
                {cashTable(t("shared.d394.achTitle"), [
                  { label: "FSA bază", f: "bazaFsa" },
                  { label: "FSA TVA", f: "tvaFsa" },
                  { label: "FSAI bază", f: "bazaFsai" },
                  { label: "FSAI TVA", f: "tvaFsai" },
                  { label: "BFAI bază", f: "bazaBfai" },
                  { label: "BFAI TVA", f: "tvaBfai" },
                ])}
              </>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={() => close()}>
            {t("shared.common.cancel")}
          </button>
          {onPreview && (
            <button
              className="pill-btn"
              disabled={!canSubmit || previewing}
              style={!canSubmit || previewing ? { opacity: 0.6, cursor: "default" } : undefined}
              onClick={handlePreview}
              title={t("declarations.d394.previewXml")}
            >
              <Ic name="eye" />
              {previewing ? t("declarations.d394.previewing") : t("declarations.d394.previewXml")}
            </button>
          )}
          <button
            className="btn-dark"
            disabled={!canSubmit}
            style={!canSubmit ? { opacity: 0.6, cursor: "default" } : undefined}
            onClick={handleSubmit}
          >
            <Ic name="dl" />
            {t("shared.declCommon.exportXml")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
