/**
 * D394SubmissionModal — colectează câmpurile D394Submission necesare pentru exportul
 * oficial ANAF (schema v5). Câmpurile stabile (telefon, reprezentant) sunt
 * inițializate cu valori sensibile.
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

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title="Export oficial D394 — date suplimentare"
      width={580}
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
        {/* Tip declarație + CAEN */}
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Tip D394">
            <Select
              id="d394-tip"
              value={tipD394}
              onChange={(e) => setTipD394(e.target.value)}
            >
              {TIP_D394_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </Select>
          </Field>
          <Field
            label="Cod CAEN principal"
            required
            error={caen.length > 0 && !caenValid ? "Codul CAEN trebuie să aibă exact 4 cifre." : undefined}
          >
            <Input
              id="d394-caen"
              value={caen}
              onChange={(e) => setCaen(e.target.value)}
              placeholder="6201"
              maxLength={4}
              error={caen.length > 0 && !caenValid}
            />
          </Field>
        </div>

        <Field label="Telefon">
          <Input
            id="d394-telefon"
            value={telefon}
            onChange={(e) => setTelefon(e.target.value)}
            placeholder="0721000000"
            maxLength={15}
          />
        </Field>

        {/* Reprezentant */}
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
          Reprezentant
        </div>
        <Field label="Denumire reprezentant" required>
          <Input
            id="d394-denr"
            value={denR}
            onChange={(e) => setDenR(e.target.value)}
            placeholder="SC ACME SRL"
            error={denR.length > 0 && denR.trim() === ""}
          />
        </Field>
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Funcție reprezentant">
            <Input
              id="d394-functie"
              value={functieReprez}
              onChange={(e) => setFunctieReprez(e.target.value)}
              placeholder="DIRECTOR"
            />
          </Field>
          <Field label="Calitate (când tip=proprie)">
            <Input
              id="d394-calitate"
              value={calitateIntocmit}
              onChange={(e) => setCalitateIntocmit(e.target.value)}
              placeholder="Reprezentant"
              disabled={tipIntocmit !== 0}
            />
          </Field>
        </div>
        <Field label="Adresă reprezentant">
          <Input
            id="d394-adresa"
            value={adresaR}
            onChange={(e) => setAdresaR(e.target.value)}
            placeholder="Str. Exemplu nr. 1, București"
          />
        </Field>

        {/* Întocmit */}
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
          Persoana care a întocmit declarația
        </div>
        <div className="rf-grid-2" style={{ gap: 12 }}>
          <Field label="Tip întocmit">
            <Select
              id="d394-tipintocmit"
              value={String(tipIntocmit)}
              onChange={(e) => setTipIntocmit(Number(e.target.value))}
            >
              <option value="0">0 — Persoana proprie</option>
              <option value="1">1 — Consultant</option>
            </Select>
          </Field>
          <Field label="CIF persoana care a întocmit">
            <Input
              id="d394-cifintocmit"
              num
              value={cifIntocmit}
              onChange={(e) => setCifIntocmit(e.target.value)}
              placeholder="0"
            />
          </Field>
        </div>
        <Field label="Denumire persoana care a întocmit" required>
          <Input
            id="d394-denintocmit"
            value={denIntocmit}
            onChange={(e) => setDenIntocmit(e.target.value)}
            placeholder="Popescu Ion"
            error={denIntocmit.length > 0 && denIntocmit.trim() === ""}
          />
        </Field>

        {/* Flags */}
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 4 }}>
            Opțiuni suplimentare
          </div>
          <Checkbox checked={sistemTva} onChange={(e) => setSistemTva(e.target.checked)}>
            Sistem TVA la încasare
          </Checkbox>
          <Checkbox checked={opEfectuate} onChange={(e) => setOpEfectuate(e.target.checked)}>
            Operațiuni cu persoane afiliate
          </Checkbox>
          <Checkbox checked={optiune} onChange={(e) => setOptiune(e.target.checked)}>
            Opțiune regim special
          </Checkbox>
          <Checkbox checked={prsAfiliat} onChange={(e) => setPrsAfiliat(e.target.checked)}>
            Persoane afiliate
          </Checkbox>
          <Checkbox checked={solicit} onChange={(e) => setSolicit(e.target.checked)}>
            Solicită rambursare TVA
          </Checkbox>
        </div>
      </div>
    </Modal>
  );
}
