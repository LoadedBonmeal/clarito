/**
 * D300SubmissionModal — colectează câmpurile D300Submission necesare pentru exportul
 * oficial ANAF (schema v12). Câmpurile stabile (bancă / IBAN) sunt pre-completate
 * din înregistrarea companiei active.
 *
 * Design re-skin: .modal-back/.modal + .fgrid/.field + .cbx check-rows
 * (pattern din src/pages/Receipts.tsx). Toată logica de validare/submit păstrată.
 */

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { Ic } from "@/components/shared/Ic";
import type { Company, D300Submission } from "@/types";

// ── Constants ─────────────────────────────────────────────────────────────────

const TIP_DECONT_OPTIONS = [
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
  onSubmit: (submission: D300Submission) => void;
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

export function D300SubmissionModal({ open, onOpenChange, company, onSubmit }: Props) {
  const [numeDeclar,       setNumeDeclar]       = useState("");
  const [prenumeDeclar,    setPrenumeDeclar]    = useState("");
  const [functieDeclar,    setFunctieDeclar]    = useState("Administrator");
  const [caen,             setCaen]             = useState("6201");
  const [banca,            setBanca]            = useState(company.bankName ?? "");
  const [cont,             setCont]             = useState(company.iban    ?? "");
  const [tipDecont,        setTipDecont]        = useState("L");
  const [temei,            setTemei]            = useState(0);
  const [depusReprezentant, setDepusReprezentant] = useState(false);
  const [bifaInterne,      setBifaInterne]      = useState(false);
  const [bifaCereale,      setBifaCereale]      = useState(false);
  const [bifaMob,          setBifaMob]          = useState(false);
  const [bifaDisp,         setBifaDisp]         = useState(false);
  const [bifaCons,         setBifaCons]         = useState(false);
  const [solicitRamb,      setSolicitRamb]      = useState(false);
  const [nrEvid,           setNrEvid]           = useState("0");
  const [proRata,          setProRata]          = useState(100.0);

  // Esc closes the modal.
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onOpenChange(false); };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [open, onOpenChange]);

  // ── Validation ───────────────────────────────────────────────────────────────

  const caenValid = /^\d{4}$/.test(caen.trim());
  const proRataValid = Number.isFinite(proRata) && proRata >= 0 && proRata <= 100;
  const canSubmit =
    numeDeclar.trim() !== "" && prenumeDeclar.trim() !== "" && caenValid && proRataValid;

  // ── Submit ───────────────────────────────────────────────────────────────────

  const handleSubmit = () => {
    if (!canSubmit) return;
    const submission: D300Submission = {
      numeDeclar:       numeDeclar.trim(),
      prenumeDeclar:    prenumeDeclar.trim(),
      functieDeclar:    functieDeclar.trim(),
      caen:             caen.trim(),
      banca:            banca.trim(),
      cont:             cont.trim(),
      tipDecont,
      temei,
      depusReprezentant,
      bifaInterne,
      bifaCereale,
      bifaMob,
      bifaDisp,
      bifaCons,
      solicitRamb,
      nrEvid:           nrEvid.trim(),
      proRata,
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
      <div className="modal" style={{ width: 560 }}>
        <div className="modal-head">
          <div>
            <div className="mt">Export oficial D300 — date suplimentare</div>
            <div className="ms">Schema ANAF v12 — câmpuri obligatorii pentru PDF-ul inteligent</div>
          </div>
          <button className="modal-x" onClick={() => onOpenChange(false)}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            {/* Declarant */}
            <div className="field">
              <label>Nume declarant <span className="req">*</span></label>
              <input
                id="d300-nume"
                className="input"
                value={numeDeclar}
                onChange={(e) => setNumeDeclar(e.target.value)}
                placeholder="Popescu"
              />
            </div>
            <div className="field">
              <label>Prenume declarant <span className="req">*</span></label>
              <input
                id="d300-prenume"
                className="input"
                value={prenumeDeclar}
                onChange={(e) => setPrenumeDeclar(e.target.value)}
                placeholder="Ion"
              />
            </div>
            <div className="field span2">
              <label>Funcție declarant</label>
              <input
                id="d300-functie"
                className="input"
                value={functieDeclar}
                onChange={(e) => setFunctieDeclar(e.target.value)}
                placeholder="Administrator"
              />
            </div>

            {/* Companie */}
            <div className="field">
              <label>Cod CAEN principal <span className="req">*</span></label>
              <input
                id="d300-caen"
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
            <div className="field">
              <label>Tip decont</label>
              <select
                id="d300-tip"
                className="select"
                value={tipDecont}
                onChange={(e) => setTipDecont(e.target.value)}
              >
                {TIP_DECONT_OPTIONS.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </div>

            {/* Bancă */}
            <div className="field">
              <label>Bancă</label>
              <input
                id="d300-banca"
                className="input"
                value={banca}
                onChange={(e) => setBanca(e.target.value)}
                placeholder="BRD, BCR…"
              />
            </div>
            <div className="field">
              <label>Cont IBAN</label>
              <input
                id="d300-cont"
                className="input num"
                value={cont}
                onChange={(e) => setCont(e.target.value)}
                placeholder="RO49AAAA1B31007593840000"
              />
            </div>

            {/* Temei + Pro-rata */}
            <div className="field">
              <label>Temei legal</label>
              <select
                id="d300-temei"
                className="select"
                value={String(temei)}
                onChange={(e) => setTemei(Number(e.target.value))}
              >
                <option value="0">0 — Standard</option>
                <option value="2">2 — Alt temei</option>
              </select>
              <span className="hint">0 = standard, 2 = alt temei</span>
            </div>
            <div className="field">
              <label>Pro-rată TVA (%)</label>
              <input
                id="d300-prorata"
                className={`input num${!proRataValid ? " invalid" : ""}`}
                type="number"
                min="0"
                max="100"
                step="0.01"
                value={String(proRata)}
                onChange={(e) => setProRata(Number(e.target.value))}
              />
              <span className="hint">100 = nu se aplică pro-rată</span>
            </div>

            {/* Nr. evidență */}
            <div className="field span2">
              <label>Nr. din Registrul persoanelor impozabile</label>
              <input
                id="d300-nrevid"
                className="input num"
                value={nrEvid}
                onChange={(e) => setNrEvid(e.target.value)}
                placeholder="0"
              />
              <span className="hint">0 dacă nu este aplicabil</span>
            </div>

            {/* Flags */}
            <div className="span2 col-title" style={{ padding: "6px 0 0" }}>
              Opțiuni suplimentare
            </div>
            <div className="field span2" style={{ gap: 9 }}>
              <CheckRow checked={depusReprezentant} onChange={setDepusReprezentant}>
                Depus prin reprezentant fiscal
              </CheckRow>
              <CheckRow checked={solicitRamb} onChange={setSolicitRamb}>
                Solicită rambursare TVA
              </CheckRow>
              <CheckRow checked={bifaInterne} onChange={setBifaInterne}>
                Operațiuni interne (bifă)
              </CheckRow>
              <CheckRow checked={bifaCereale} onChange={setBifaCereale}>
                Operațiuni cu cereale
              </CheckRow>
              <CheckRow checked={bifaMob} onChange={setBifaMob}>
                Telefoane mobile / dispozitive electronice (mobilier)
              </CheckRow>
              <CheckRow checked={bifaDisp} onChange={setBifaDisp}>
                Dispozitive (telefoane, tablete, laptopuri)
              </CheckRow>
              <CheckRow checked={bifaCons} onChange={setBifaCons}>
                Construcții
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
