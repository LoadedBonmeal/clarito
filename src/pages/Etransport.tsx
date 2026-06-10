/**
 * EtransportPage — RO e-Transport UIT declaration. Captures the goods/partner/transport/route,
 * validates, generates the schema-v2 XML, and submits to ANAF (live OAuth API — unlike D300/D394,
 * e-Transport IS API-automatable). Obtain the UIT BEFORE the transport starts.
 */

import { useState } from "react";
import { useQuery, useMutation } from "@tanstack/react-query";

import { PageHeader } from "@/components/rf";
import { SectionCard, Btn, Banner, Badge } from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { EtransportDeclaration, EtransportGood, EtransportDoc } from "@/types";

const OPERATION_TYPES: { value: string; label: string }[] = [
  { value: "10", label: "10 — Achiziție intracomunitară" },
  { value: "12", label: "12 — Operațiuni în sistem lohn (intrare)" },
  { value: "14", label: "14 — Stocuri la dispoziția clientului (intrare)" },
  { value: "20", label: "20 — Livrare intracomunitară" },
  { value: "22", label: "22 — Operațiuni în sistem lohn (ieșire)" },
  { value: "24", label: "24 — Stocuri la dispoziția clientului (ieșire)" },
  { value: "30", label: "30 — Transport pe teritoriul național" },
  { value: "40", label: "40 — Import" },
  { value: "50", label: "50 — Export" },
  { value: "60", label: "60 — Tranzacție intracomunitară (non-transfer)" },
  { value: "70", label: "70 — Transport în cadrul achiziției intracom." },
];

const DOC_TYPES: { value: string; label: string }[] = [
  { value: "10", label: "CMR" },
  { value: "20", label: "Factură" },
  { value: "30", label: "Aviz de însoțire" },
  { value: "9999", label: "Altele" },
];

const emptyGood = (): EtransportGood => ({
  codScopOperatiune: "101",
  codTarifar: "",
  denumireMarfa: "",
  cantitate: 0,
  codUnitateMasura: "KGM",
  greutateBruta: 0,
});

const emptyDoc = (): EtransportDoc => ({ tipDocument: "20", numarDocument: "", dataDocument: "" });

const inp: React.CSSProperties = { width: "100%" };
const lbl: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 3, fontSize: 12 };
const muted: React.CSSProperties = { color: "var(--rf-text-muted)" };

export function EtransportPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: declRecords = [], refetch: refetchDecls } = useQuery({
    queryKey: ["etransportDecls", activeCompanyId],
    queryFn: () => api.etransport.listDeclarations(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const [codTipOperatiune, setOp] = useState("30");
  const [goods, setGoods] = useState<EtransportGood[]>([emptyGood()]);
  const [partnerName, setPartnerName] = useState("");
  const [partnerCountry, setPartnerCountry] = useState("RO");
  const [partnerCode, setPartnerCode] = useState("");
  const [nrVehicul, setNrVehicul] = useState("");
  const [dataTransport, setDataTransport] = useState("");
  const [startLoc, setStartLoc] = useState("");
  const [finalLoc, setFinalLoc] = useState("");
  const [judetStart, setJudetStart] = useState("");
  const [judetFinal, setJudetFinal] = useState("");
  const [documents, setDocuments] = useState<EtransportDoc[]>([emptyDoc()]);
  const [errors, setErrors] = useState<string[]>([]);

  const build = (): EtransportDeclaration => ({
    codDeclarant: company?.cui ?? "",
    codTipOperatiune,
    goods,
    partner: { codTara: partnerCountry, cod: partnerCode, denumire: partnerName },
    transport: { nrVehicul, dataTransport },
    locStart: { codJudet: judetStart ? Number(judetStart) : null, denumireLocalitate: startLoc },
    locFinal: { codJudet: judetFinal ? Number(judetFinal) : null, denumireLocalitate: finalLoc },
    documents,
  });

  const setGood = (i: number, patch: Partial<EtransportGood>) =>
    setGoods((g) => g.map((row, j) => (j === i ? { ...row, ...patch } : row)));
  const setDoc = (i: number, patch: Partial<EtransportDoc>) =>
    setDocuments((d) => d.map((row, j) => (j === i ? { ...row, ...patch } : row)));

  const validate = useMutation({
    mutationFn: () => api.etransport.validate(build()),
    onSuccess: (errs) => {
      setErrors(errs);
      if (errs.length === 0) notify.success("Declarație validă.");
    },
    onError: (e) => notify.error(formatError(e, "Validare eșuată.")),
  });

  const genXml = useMutation({
    mutationFn: () => api.etransport.generateXml(build()),
    onSuccess: () => notify.success("XML generat (valid)."),
    onError: (e) => {
      const msg = formatError(e, "Generare XML eșuată.");
      setErrors([msg]);
      notify.error(msg);
    },
  });

  const submit = useMutation({
    mutationFn: () => api.etransport.submit(activeCompanyId!, build()),
    onSuccess: (res) => {
      notify.success(res.UIT ? `UIT obținut: ${res.UIT}` : `Trimis (index ${res.index_incarcare}).`);
      void refetchDecls();
    },
    onError: (e) => notify.error(formatError(e, "Trimiterea la ANAF a eșuat.")),
  });

  return (
    <div className="rf-page">
      <PageHeader title="e-Transport (UIT)" sub={<Badge variant="info">schema v2</Badge>} />
      <div className="rf-page-body">
        <SectionCard icon="declaration" title="Declarație UIT">
          <div style={{ padding: "0 16px 12px" }}>
            <Banner variant="info">
              Obțineți UIT-ul <b>înainte</b> de începerea transportului (cu cel mult 3 zile înainte);
              valabil 5 zile (15 pentru achiziții intracomunitare). Obligatoriu pentru vehicule
              ≥ 2,5 t cu marfă &gt; 500 kg sau &gt; 10.000 lei, plus tot transportul intracomunitar /
              import / export.
            </Banner>
          </div>

          <div style={{ padding: "0 16px 16px", display: "grid", gap: 12 }}>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <label style={lbl}>
                <span style={muted}>Declarant (CUI)</span>
                <input className="rf-input" style={inp} value={company?.cui ?? ""} disabled />
              </label>
              <label style={lbl}>
                <span style={muted}>Tip operațiune</span>
                <select className="rf-select" style={inp} value={codTipOperatiune} onChange={(e) => setOp(e.target.value)}>
                  {OPERATION_TYPES.map((o) => (
                    <option key={o.value} value={o.value}>{o.label}</option>
                  ))}
                </select>
              </label>
            </div>

            {/* Goods */}
            <div>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
                <b style={{ fontSize: 12.5 }}>Bunuri transportate</b>
                <Btn variant="ghost" size="sm" icon="plus" onClick={() => setGoods((g) => [...g, emptyGood()])}>Adaugă</Btn>
              </div>
              {goods.map((g, i) => (
                <div key={i} style={{ display: "grid", gridTemplateColumns: "2fr 1fr 1fr 1fr 1fr auto", gap: 8, marginBottom: 6 }}>
                  <input className="rf-input" placeholder="Denumire marfă" value={g.denumireMarfa} onChange={(e) => setGood(i, { denumireMarfa: e.target.value })} />
                  <input className="rf-input" placeholder="Cod NC" value={g.codTarifar ?? ""} onChange={(e) => setGood(i, { codTarifar: e.target.value })} />
                  <input className="rf-input" placeholder="Cantitate" inputMode="decimal" value={g.cantitate || ""} onChange={(e) => setGood(i, { cantitate: Number(e.target.value) })} />
                  <input className="rf-input" placeholder="UM (KGM)" value={g.codUnitateMasura} onChange={(e) => setGood(i, { codUnitateMasura: e.target.value })} />
                  <input className="rf-input" placeholder="Greut. brută (kg)" inputMode="decimal" value={g.greutateBruta || ""} onChange={(e) => setGood(i, { greutateBruta: Number(e.target.value) })} />
                  <Btn variant="ghost" size="sm" icon="trash" disabled={goods.length <= 1} onClick={() => setGoods((arr) => arr.filter((_, j) => j !== i))} />
                </div>
              ))}
            </div>

            {/* Partner */}
            <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr 1fr", gap: 12 }}>
              <label style={lbl}><span style={muted}>Partener — denumire</span><input className="rf-input" value={partnerName} onChange={(e) => setPartnerName(e.target.value)} /></label>
              <label style={lbl}><span style={muted}>Țară (ISO-2)</span><input className="rf-input" value={partnerCountry} onChange={(e) => setPartnerCountry(e.target.value.toUpperCase())} /></label>
              <label style={lbl}><span style={muted}>Cod partener</span><input className="rf-input" value={partnerCode} onChange={(e) => setPartnerCode(e.target.value)} /></label>
            </div>

            {/* Transport */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <label style={lbl}><span style={muted}>Nr. vehicul</span><input className="rf-input" value={nrVehicul} onChange={(e) => setNrVehicul(e.target.value.toUpperCase())} /></label>
              <label style={lbl}><span style={muted}>Data transport</span><input className="rf-input" type="date" value={dataTransport} onChange={(e) => setDataTransport(e.target.value)} /></label>
            </div>
            {/* Route — județ (cod 1..52) + localitate sunt ambele necesare pentru o adresă validă */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 2fr 1fr 2fr", gap: 12 }}>
              <label style={lbl}><span style={muted}>Județ plecare (cod)</span><input className="rf-input" inputMode="numeric" value={judetStart} onChange={(e) => setJudetStart(e.target.value)} /></label>
              <label style={lbl}><span style={muted}>Localitate plecare</span><input className="rf-input" value={startLoc} onChange={(e) => setStartLoc(e.target.value)} /></label>
              <label style={lbl}><span style={muted}>Județ sosire (cod)</span><input className="rf-input" inputMode="numeric" value={judetFinal} onChange={(e) => setJudetFinal(e.target.value)} /></label>
              <label style={lbl}><span style={muted}>Localitate sosire</span><input className="rf-input" value={finalLoc} onChange={(e) => setFinalLoc(e.target.value)} /></label>
            </div>

            {/* Documents */}
            <div>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
                <b style={{ fontSize: 12.5 }}>Documente transport</b>
                <Btn variant="ghost" size="sm" icon="plus" onClick={() => setDocuments((d) => [...d, emptyDoc()])}>Adaugă</Btn>
              </div>
              {documents.map((d, i) => (
                <div key={i} style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr auto", gap: 8, marginBottom: 6 }}>
                  <select className="rf-select" value={d.tipDocument} onChange={(e) => setDoc(i, { tipDocument: e.target.value })}>
                    {DOC_TYPES.map((t) => <option key={t.value} value={t.value}>{t.label}</option>)}
                  </select>
                  <input className="rf-input" placeholder="Număr" value={d.numarDocument ?? ""} onChange={(e) => setDoc(i, { numarDocument: e.target.value })} />
                  <input className="rf-input" type="date" value={d.dataDocument ?? ""} onChange={(e) => setDoc(i, { dataDocument: e.target.value })} />
                  <Btn variant="ghost" size="sm" icon="trash" disabled={documents.length <= 1} onClick={() => setDocuments((arr) => arr.filter((_, j) => j !== i))} />
                </div>
              ))}
            </div>

            {errors.length > 0 && (
              <Banner variant="error">
                <ul style={{ margin: 0, paddingLeft: 18 }}>{errors.map((e, i) => <li key={i}>{e}</li>)}</ul>
              </Banner>
            )}

            <div style={{ display: "flex", gap: 8 }}>
              <Btn variant="secondary" size="sm" disabled={validate.isPending} onClick={() => validate.mutate()}>Validează</Btn>
              <Btn variant="secondary" size="sm" disabled={genXml.isPending} onClick={() => genXml.mutate()}>Generează XML</Btn>
              <Btn variant="primary" size="sm" icon="anaf" disabled={submit.isPending || !activeCompanyId} onClick={() => submit.mutate()}>
                {submit.isPending ? "Se trimite…" : "Trimite la ANAF"}
              </Btn>
            </div>
            {submit.data?.UIT && (
              <Banner variant="success">UIT: <b>{submit.data.UIT}</b> — tipăriți-l pe documentul de transport.</Banner>
            )}
          </div>
        </SectionCard>

        {/* Evidența UIT: codul e valabil 5 zile (național) / 15 zile (intracomunitar, import-export)
            de la transmitere — un transport pornit cu UIT expirat e sancționabil. */}
        {declRecords.length > 0 && (
          <SectionCard icon="truck" title="Declarații transmise (evidența UIT)"
            subtitle="UIT valabil 5 zile (național) / 15 zile (intracomunitar / import-export)">
            <div className="rf-tbl-wrap">
              <table className="rf-tbl">
                <thead>
                  <tr><th>UIT</th><th>Operațiune</th><th>Partener</th><th>Vehicul</th><th>Transmis</th><th>Expiră</th><th></th></tr>
                </thead>
                <tbody>
                  {declRecords.map((d) => {
                    const now = Date.now() / 1000;
                    const expired = d.expiresAt < now;
                    const expiringSoon = !expired && d.expiresAt - now < 86_400;
                    return (
                      <tr key={d.id} style={{ opacity: expired ? 0.55 : 1 }}>
                        <td className="rf-mono">{d.uit ?? "—"}{d.testMode ? " (test)" : ""}</td>
                        <td className="rf-mono">{d.codTipOperatiune}</td>
                        <td>{d.partnerName || "—"}</td>
                        <td className="rf-mono">{d.vehicle || "—"}</td>
                        <td className="rf-mono">{new Date(d.submittedAt * 1000).toLocaleDateString("ro-RO")}</td>
                        <td className="rf-mono">{new Date(d.expiresAt * 1000).toLocaleDateString("ro-RO")}</td>
                        <td>
                          {expired ? (
                            <Badge variant="error">expirat</Badge>
                          ) : expiringSoon ? (
                            <Badge variant="warning">expiră în &lt;24h</Badge>
                          ) : (
                            <Badge variant="success">valabil</Badge>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </SectionCard>
        )}
      </div>
    </div>
  );
}
