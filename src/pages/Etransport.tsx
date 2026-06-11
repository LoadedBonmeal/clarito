/**
 * e-Transport (UIT) — verbatim port of the design "eTransport.html":
 *   .page-head (h1 + chip "schema v2" + sub) · .banner info reguli UIT ·
 *   .scr-card "Declarație UIT" (.card-pad → .grid2 declarant/tip operațiune ·
 *   .gsec + .gline bunuri · .grid3 partener · .grid2/.grid4 transport + traseu ·
 *   .gsec + .dline documente · pill-btn Validează / Generează XML · btn-dark
 *   Trimite la ANAF · .banner.ok UIT) · .scr-card registru UIT (.scr-table cu
 *   chips valabil / expiră în 24h / expirat, rânduri expirate cu opacity .6).
 *
 * ALL wiring preserved: api.etransport.validate/generateXml/submit,
 * api.etransport.listDeclarations, api.companies.get, goods/documents
 * add+remove, error list, valabilitate 5/15 zile din expiresAt.
 */

import { useState } from "react";
import { useQuery, useMutation } from "@tanstack/react-query";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { EtransportDeclaration, EtransportGood, EtransportDoc } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
/** Unix epoch (secunde) → `09 iun 2026` (formatul prototipului). */
const fmtRoEpoch = (epoch: number) => {
  const d = new Date(epoch * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}`;
};

/** "Giannis Auto SRL" → "GA" (prototype .cli-ava initials). */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "—";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

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

// Icons NOT in Ic.tsx — inlined verbatim from the prototype.
const INFO_PATH =
  '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';
const CHECK_CIRCLE_PATH =
  '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const BAN_PATH =
  '<path d="M18.364 18.364A9 9 0 0 0 5.636 5.636m12.728 12.728A9 9 0 0 1 5.636 5.636m12.728 12.728L5.636 5.636"/>';

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

  const opLabel = (code: string) => OPERATION_TYPES.find((o) => o.value === code)?.label ?? code;

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>e-Transport (UIT)</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a declara transporturi e-Transport.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <div className="head-title" style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <h1>e-Transport (UIT)</h1>
            <span className="chip sent">schema v2</span>
          </div>
          <p className="sub">Declarații UIT pentru transporturi cu risc fiscal ridicat · RO e-Transport</p>
        </div>
      </div>

      {/* info banner — reguli UIT */}
      <div className="banner">
        <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: INFO_PATH }} />
        <span>
          Obțineți UIT-ul <b>înainte</b> de începerea transportului (cu cel mult 3 zile înainte);
          valabil <b>5 zile</b> (15 pentru achiziții intracomunitare). Obligatoriu pentru vehicule{" "}
          <b>≥ 2,5 t</b> cu marfă <b>&gt; 500 kg</b> sau <b>&gt; 10.000 lei</b>, plus tot
          transportul intracomunitar / import / export.
        </span>
      </div>

      {/* DECLARAȚIE UIT */}
      <div className="scr-card" style={{ marginBottom: 14 }}>
        <div className="scr-toolbar"><div className="tt">Declarație UIT</div></div>
        <div className="card-pad">
          <div className="grid2">
            <div className="field">
              <label>Declarant (CUI)</label>
              <input
                className="input num"
                type="text"
                value={company?.cui ?? ""}
                disabled
                style={{ background: "var(--fill)", color: "var(--text-2)" }}
              />
            </div>
            <div className="field">
              <label>Tip operațiune</label>
              <select className="select" value={codTipOperatiune} onChange={(e) => setOp(e.target.value)}>
                {OPERATION_TYPES.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </div>
          </div>

          {/* Bunuri transportate */}
          <div className="gsec">
            Bunuri transportate{" "}
            <span className="add-sm" onClick={() => setGoods((g) => [...g, emptyGood()])}>
              <svg
                className="ic"
                viewBox="0 0 24 24"
                style={{ width: 13, height: 13 }}
                dangerouslySetInnerHTML={{ __html: '<path d="M12 4.5v15m7.5-7.5h-15"/>' }}
              />
              Adaugă
            </span>
          </div>
          {goods.map((g, i) => (
            <div key={i} className="gline">
              <input
                className="input" type="text" placeholder="Denumire marfă"
                value={g.denumireMarfa}
                onChange={(e) => setGood(i, { denumireMarfa: e.target.value })}
              />
              <input
                className="input num" type="text" placeholder="Cod NC"
                value={g.codTarifar ?? ""}
                onChange={(e) => setGood(i, { codTarifar: e.target.value })}
              />
              <input
                className="input num" type="text" placeholder="Cantitate" inputMode="decimal"
                value={g.cantitate || ""}
                onChange={(e) => setGood(i, { cantitate: Number(e.target.value) })}
              />
              <input
                className="input num" type="text" placeholder="UM (KGM)"
                value={g.codUnitateMasura}
                onChange={(e) => setGood(i, { codUnitateMasura: e.target.value })}
              />
              <input
                className="input num" type="text" placeholder="Greut. brută (kg)" inputMode="decimal"
                style={{ textAlign: "right" }}
                value={g.greutateBruta || ""}
                onChange={(e) => setGood(i, { greutateBruta: Number(e.target.value) })}
              />
              <button
                className="mini-btn" title="Șterge" type="button"
                disabled={goods.length <= 1}
                style={goods.length <= 1 ? { opacity: 0.4, cursor: "default" } : undefined}
                onClick={() => setGoods((arr) => arr.filter((_, j) => j !== i))}
              >
                <Ic name="xMark" />
              </button>
            </div>
          ))}

          {/* Partener */}
          <div className="gsec">Partener</div>
          <div className="grid3">
            <div className="field">
              <label>Denumire</label>
              <input className="input" type="text" value={partnerName} onChange={(e) => setPartnerName(e.target.value)} />
            </div>
            <div className="field">
              <label>Țară (ISO-2)</label>
              <input className="input num" type="text" value={partnerCountry} onChange={(e) => setPartnerCountry(e.target.value.toUpperCase())} />
            </div>
            <div className="field">
              <label>Cod partener</label>
              <input className="input num" type="text" value={partnerCode} onChange={(e) => setPartnerCode(e.target.value)} />
            </div>
          </div>

          {/* Transport */}
          <div className="gsec">Transport</div>
          <div className="grid2">
            <div className="field">
              <label>Nr. vehicul</label>
              <input className="input num" type="text" value={nrVehicul} onChange={(e) => setNrVehicul(e.target.value.toUpperCase())} />
            </div>
            <div className="field">
              <label>Data transport</label>
              <input className="input num" type="date" value={dataTransport} onChange={(e) => setDataTransport(e.target.value)} />
            </div>
          </div>
          {/* Traseu — județ (cod 1..52) + localitate sunt ambele necesare pentru o adresă validă */}
          <div className="grid4" style={{ marginTop: 13 }}>
            <div className="field">
              <label>Județ plecare (cod)</label>
              <input className="input num" type="text" inputMode="numeric" value={judetStart} onChange={(e) => setJudetStart(e.target.value)} />
            </div>
            <div className="field">
              <label>Localitate plecare</label>
              <input className="input" type="text" value={startLoc} onChange={(e) => setStartLoc(e.target.value)} />
            </div>
            <div className="field">
              <label>Județ sosire (cod)</label>
              <input className="input num" type="text" inputMode="numeric" value={judetFinal} onChange={(e) => setJudetFinal(e.target.value)} />
            </div>
            <div className="field">
              <label>Localitate sosire</label>
              <input className="input" type="text" value={finalLoc} onChange={(e) => setFinalLoc(e.target.value)} />
            </div>
          </div>

          {/* Documente transport */}
          <div className="gsec">
            Documente transport{" "}
            <span className="add-sm" onClick={() => setDocuments((d) => [...d, emptyDoc()])}>
              <svg
                className="ic"
                viewBox="0 0 24 24"
                style={{ width: 13, height: 13 }}
                dangerouslySetInnerHTML={{ __html: '<path d="M12 4.5v15m7.5-7.5h-15"/>' }}
              />
              Adaugă
            </span>
          </div>
          {documents.map((d, i) => (
            <div key={i} className="dline">
              <select className="select" value={d.tipDocument} onChange={(e) => setDoc(i, { tipDocument: e.target.value })}>
                {DOC_TYPES.map((t) => <option key={t.value} value={t.value}>{t.label}</option>)}
              </select>
              <input
                className="input num" type="text" placeholder="Număr"
                value={d.numarDocument ?? ""}
                onChange={(e) => setDoc(i, { numarDocument: e.target.value })}
              />
              <input
                className="input num" type="date" placeholder="Data"
                value={d.dataDocument ?? ""}
                onChange={(e) => setDoc(i, { dataDocument: e.target.value })}
              />
              <button
                className="mini-btn" title="Șterge" type="button"
                disabled={documents.length <= 1}
                style={documents.length <= 1 ? { opacity: 0.4, cursor: "default" } : undefined}
                onClick={() => setDocuments((arr) => arr.filter((_, j) => j !== i))}
              >
                <Ic name="xMark" />
              </button>
            </div>
          ))}

          {/* erori de validare — funcționalitate reală, restilizată cu .banner.danger */}
          {errors.length > 0 && (
            <div className="banner danger" style={{ margin: "14px 0 0" }}>
              <Ic name="xMark" />
              <span>
                <ul style={{ margin: 0, paddingLeft: 18 }}>
                  {errors.map((e, i) => <li key={i}>{e}</li>)}
                </ul>
              </span>
            </div>
          )}

          <div style={{ display: "flex", gap: 8, marginTop: 16 }}>
            <button className="pill-btn" type="button" disabled={validate.isPending} onClick={() => validate.mutate()}>
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: CHECK_CIRCLE_PATH }} />
              Validează
            </button>
            <button className="pill-btn" type="button" disabled={genXml.isPending} onClick={() => genXml.mutate()}>
              <Ic name="code" />
              Generează XML
            </button>
            <button
              className="btn-dark send-btn" type="button"
              disabled={submit.isPending}
              onClick={() => submit.mutate()}
            >
              <Ic name="send" />
              {submit.isPending ? "Se trimite…" : "Trimite la ANAF"}
            </button>
          </div>
          {submit.data?.UIT && (
            <div className="banner ok" style={{ margin: "14px 0 0" }}>
              <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: CHECK_CIRCLE_PATH }} />
              <span>
                UIT: <b className="num">{submit.data.UIT}</b> · index încărcare{" "}
                <b className="num">{submit.data.index_incarcare}</b> — tipăriți codul pe documentul
                de transport.
              </span>
            </div>
          )}
        </div>
      </div>

      {/* REGISTRU UIT — valabil 5 zile (național) / 15 zile (intracomunitar, import-export) */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">Declarații transmise (evidența UIT)</div>
          <div className="spacer" />
          <span className="muted" style={{ fontSize: 12 }}>
            UIT valabil 5 zile (național) / 15 zile (intracomunitar / import-export)
          </span>
        </div>
        {declRecords.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Nicio declarație transmisă încă. UIT-urile obținute apar aici.
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr><th>UIT</th><th>Operațiune</th><th>Partener</th><th>Vehicul</th><th>Transmis</th><th>Expiră</th><th>Status</th></tr>
            </thead>
            <tbody>
              {declRecords.map((d) => {
                const now = Date.now() / 1000;
                const expired = d.expiresAt < now;
                const expiringSoon = !expired && d.expiresAt - now < 86_400;
                return (
                  <tr key={d.id} style={expired ? { opacity: 0.6 } : undefined}>
                    <td>
                      <span className="doc" style={{ fontWeight: 700, color: "var(--text)" }}>
                        {d.uit ?? "—"}{d.testMode ? " (test)" : ""}
                      </span>
                    </td>
                    <td>{opLabel(d.codTipOperatiune)}</td>
                    <td>
                      {d.partnerName
                        ? <div className="cli"><span className="cli-ava">{initials(d.partnerName)}</span>{d.partnerName}</div>
                        : <span className="muted">—</span>}
                    </td>
                    <td>{d.vehicle ? <span className="doc">{d.vehicle}</span> : <span className="muted">—</span>}</td>
                    <td className="num">{fmtRoEpoch(d.submittedAt)}</td>
                    <td className="num">{fmtRoEpoch(d.expiresAt)}</td>
                    <td>
                      {expired ? (
                        <span className="chip sent">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: BAN_PATH }} />
                          Expirat
                        </span>
                      ) : expiringSoon ? (
                        <span className="chip wait"><Ic name="clock" cls="sic" />Expiră în 24h</span>
                      ) : (
                        <span className="chip paid">
                          <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: CHECK_CIRCLE_PATH }} />
                          Valabil
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
