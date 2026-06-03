/**
 * D300SubmissionModal — colectează câmpurile D300Submission necesare pentru exportul
 * oficial ANAF (schema v12). Câmpurile stabile (bancă / IBAN) sunt pre-completate
 * din înregistrarea companiei active.
 *
 * P7 — rf kit: Modal, Field, Input, Select, Checkbox, Btn.
 */

import { useState } from "react";

import {
  Modal,
  Field,
  Input,
  Select,
  Checkbox,
  Btn,
} from "@/components/rf";
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

  // ── Validation ───────────────────────────────────────────────────────────────

  const caenValid = /^\d{4}$/.test(caen.trim());
  const canSubmit = numeDeclar.trim() !== "" && prenumeDeclar.trim() !== "" && caenValid;

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

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title="Export oficial D300 — date suplimentare"
      width={560}
      footer={
        <>
          <Btn variant="ghost" onClick={() => onOpenChange(false)}>Anulează</Btn>
          <Btn variant="primary" disabled={!canSubmit} onClick={handleSubmit}>
            Exportă XML oficial
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {/* Declarant */}
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Nume declarant" required>
            <Input
              id="d300-nume"
              value={numeDeclar}
              onChange={(e) => setNumeDeclar(e.target.value)}
              placeholder="Popescu"
              error={numeDeclar.length > 0 && numeDeclar.trim() === ""}
            />
          </Field>
          <Field label="Prenume declarant" required>
            <Input
              id="d300-prenume"
              value={prenumeDeclar}
              onChange={(e) => setPrenumeDeclar(e.target.value)}
              placeholder="Ion"
              error={prenumeDeclar.length > 0 && prenumeDeclar.trim() === ""}
            />
          </Field>
        </div>
        <Field label="Funcție declarant">
          <Input
            id="d300-functie"
            value={functieDeclar}
            onChange={(e) => setFunctieDeclar(e.target.value)}
            placeholder="Administrator"
          />
        </Field>

        {/* Companie */}
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field
            label="Cod CAEN principal"
            required
            error={caen.length > 0 && !caenValid ? "Codul CAEN trebuie să aibă exact 4 cifre." : undefined}
          >
            <Input
              id="d300-caen"
              value={caen}
              onChange={(e) => setCaen(e.target.value)}
              placeholder="6201"
              maxLength={4}
              error={caen.length > 0 && !caenValid}
            />
          </Field>
          <Field label="Tip decont">
            <Select
              id="d300-tip"
              value={tipDecont}
              onChange={(e) => setTipDecont(e.target.value)}
            >
              {TIP_DECONT_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </Select>
          </Field>
        </div>

        {/* Bancă */}
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Bancă">
            <Input
              id="d300-banca"
              value={banca}
              onChange={(e) => setBanca(e.target.value)}
              placeholder="BRD, BCR…"
            />
          </Field>
          <Field label="Cont IBAN">
            <Input
              id="d300-cont"
              value={cont}
              onChange={(e) => setCont(e.target.value)}
              placeholder="RO49AAAA1B31007593840000"
            />
          </Field>
        </div>

        {/* Temei + Pro-rata */}
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Temei legal" help="0 = standard, 2 = alt temei">
            <Select
              id="d300-temei"
              value={String(temei)}
              onChange={(e) => setTemei(Number(e.target.value))}
            >
              <option value="0">0 — Standard</option>
              <option value="2">2 — Alt temei</option>
            </Select>
          </Field>
          <Field label="Pro-rată TVA (%)" help="100 = nu se aplică pro-rată">
            <Input
              id="d300-prorata"
              type="number"
              num
              min="0"
              max="100"
              step="0.01"
              value={String(proRata)}
              onChange={(e) => setProRata(Number(e.target.value))}
            />
          </Field>
        </div>

        {/* Nr. evidență */}
        <Field label="Nr. din Registrul persoanelor impozabile" help="0 dacă nu aplicabil">
          <Input
            id="d300-nrevid"
            value={nrEvid}
            onChange={(e) => setNrEvid(e.target.value)}
            placeholder="0"
          />
        </Field>

        {/* Flags */}
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 4 }}>
            Opțiuni suplimentare
          </div>
          <Checkbox
            checked={depusReprezentant}
            onChange={(e) => setDepusReprezentant(e.target.checked)}
          >
            Depus prin reprezentant fiscal
          </Checkbox>
          <Checkbox
            checked={solicitRamb}
            onChange={(e) => setSolicitRamb(e.target.checked)}
          >
            Solicită rambursare TVA
          </Checkbox>
          <Checkbox
            checked={bifaInterne}
            onChange={(e) => setBifaInterne(e.target.checked)}
          >
            Operațiuni interne (bifă)
          </Checkbox>
          <Checkbox
            checked={bifaCereale}
            onChange={(e) => setBifaCereale(e.target.checked)}
          >
            Operațiuni cu cereale
          </Checkbox>
          <Checkbox
            checked={bifaMob}
            onChange={(e) => setBifaMob(e.target.checked)}
          >
            Telefoane mobile / dispozitive electronice (mobilier)
          </Checkbox>
          <Checkbox
            checked={bifaDisp}
            onChange={(e) => setBifaDisp(e.target.checked)}
          >
            Dispozitive (telefoane, tablete, laptopuri)
          </Checkbox>
          <Checkbox
            checked={bifaCons}
            onChange={(e) => setBifaCons(e.target.checked)}
          >
            Construcții
          </Checkbox>
        </div>
      </div>
    </Modal>
  );
}
