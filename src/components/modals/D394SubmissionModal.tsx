/**
 * D394SubmissionModal — colectează câmpurile D394Submission necesare pentru exportul
 * oficial ANAF (schema v5). Câmpurile stabile (telefon, reprezentant) sunt
 * inițializate cu valori sensibile.
 *
 * Design re-skin: .modal-back/.modal + .fgrid/.field + .cbx check-rows
 * (pattern din src/pages/Receipts.tsx). Toată logica de validare/submit păstrată.
 */

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { Ic } from "@/components/shared/Ic";
import type { Company, D394Submission } from "@/types";

// ── Constants ─────────────────────────────────────────────────────────────────

const TIP_D394_OPTIONS = [
  { value: "L", label: "L — Lunar" },
  { value: "T", label: "T — Trimestrial" },
  { value: "S", label: "S — Semestrial" },
  { value: "A", label: "A — Anual" },
];

// ── Props ─────────────────────────────────────────────────────────────────────

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  company: Company;
  onSubmit: (submission: D394Submission) => void;
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

export function D394SubmissionModal({ open, onOpenChange, company, onSubmit }: Props) {
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

  // Esc closes the modal.
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onOpenChange(false); };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [open, onOpenChange]);

  // ── Validation ───────────────────────────────────────────────────────────────

  const caenValid = /^\d{4}$/.test(caen.trim());
  const canSubmit = caenValid && denR.trim() !== "" && denIntocmit.trim() !== "";

  // ── Submit ───────────────────────────────────────────────────────────────────

  const handleSubmit = () => {
    if (!canSubmit) return;
    const submission: D394Submission = {
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
    };
    onSubmit(submission);
    onOpenChange(false);
  };

  if (!open) return null;

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onOpenChange(false); }}
    >
      <div className="modal" style={{ width: 580 }}>
        <div className="modal-head">
          <div>
            <div className="mt">Export oficial D394 — date suplimentare</div>
            <div className="ms">Schema ANAF v5 — câmpuri obligatorii pentru PDF-ul inteligent</div>
          </div>
          <button className="modal-x" onClick={() => onOpenChange(false)}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            {/* Tip declarație + CAEN */}
            <div className="field">
              <label>Tip D394</label>
              <select
                id="d394-tip"
                className="select"
                value={tipD394}
                onChange={(e) => setTipD394(e.target.value)}
              >
                {TIP_D394_OPTIONS.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Cod CAEN principal <span className="req">*</span></label>
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
                  Codul CAEN trebuie să aibă exact 4 cifre.
                </span>
              )}
            </div>

            <div className="field span2">
              <label>Telefon</label>
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
              Reprezentant
            </div>
            <div className="field span2">
              <label>Denumire reprezentant <span className="req">*</span></label>
              <input
                id="d394-denr"
                className="input"
                value={denR}
                onChange={(e) => setDenR(e.target.value)}
                placeholder="SC ACME SRL"
              />
            </div>
            <div className="field">
              <label>Funcție reprezentant</label>
              <input
                id="d394-functie"
                className="input"
                value={functieReprez}
                onChange={(e) => setFunctieReprez(e.target.value)}
                placeholder="DIRECTOR"
              />
            </div>
            <div className="field">
              <label>Calitate (când tip=proprie)</label>
              <input
                id="d394-calitate"
                className="input"
                value={calitateIntocmit}
                onChange={(e) => setCalitateIntocmit(e.target.value)}
                placeholder="Reprezentant"
                disabled={tipIntocmit !== 0}
                style={tipIntocmit !== 0 ? { opacity: 0.55, background: "var(--fill)" } : undefined}
              />
            </div>
            <div className="field span2">
              <label>Adresă reprezentant</label>
              <input
                id="d394-adresa"
                className="input"
                value={adresaR}
                onChange={(e) => setAdresaR(e.target.value)}
                placeholder="Str. Exemplu nr. 1, București"
              />
            </div>

            {/* Întocmit */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              Persoana care a întocmit declarația
            </div>
            <div className="field">
              <label>Tip întocmit</label>
              <select
                id="d394-tipintocmit"
                className="select"
                value={String(tipIntocmit)}
                onChange={(e) => setTipIntocmit(Number(e.target.value))}
              >
                <option value="0">0 — Persoana proprie</option>
                <option value="1">1 — Consultant</option>
              </select>
            </div>
            <div className="field">
              <label>CIF persoana care a întocmit</label>
              <input
                id="d394-cifintocmit"
                className="input num"
                value={cifIntocmit}
                onChange={(e) => setCifIntocmit(e.target.value)}
                placeholder="0"
              />
            </div>
            <div className="field span2">
              <label>Denumire persoana care a întocmit <span className="req">*</span></label>
              <input
                id="d394-denintocmit"
                className="input"
                value={denIntocmit}
                onChange={(e) => setDenIntocmit(e.target.value)}
                placeholder="Popescu Ion"
              />
            </div>

            {/* Flags */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              Opțiuni suplimentare
            </div>
            <div className="field span2" style={{ gap: 9 }}>
              <CheckRow checked={sistemTva} onChange={setSistemTva}>
                Sistem TVA la încasare
              </CheckRow>
              <CheckRow checked={opEfectuate} onChange={setOpEfectuate}>
                Operațiuni cu persoane afiliate
              </CheckRow>
              <CheckRow checked={optiune} onChange={setOptiune}>
                Opțiune regim special
              </CheckRow>
              <CheckRow checked={prsAfiliat} onChange={setPrsAfiliat}>
                Persoane afiliate
              </CheckRow>
              <CheckRow checked={solicit} onChange={setSolicit}>
                Solicită rambursare TVA
              </CheckRow>
            </div>
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={() => onOpenChange(false)}>
            Anulează
          </button>
          <button
            className="btn-dark"
            disabled={!canSubmit}
            style={!canSubmit ? { opacity: 0.6, cursor: "default" } : undefined}
            onClick={handleSubmit}
          >
            <Ic name="dl" />
            Exportă XML oficial
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
