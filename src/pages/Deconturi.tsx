/**
 * Deconturi & Avansuri — Treasury advances (542) + expense reports with per-diem engine.
 *
 * Tabs:
 *   "Avansuri de trezorerie" — grant / return / delete advances
 *   "Deconturi de cheltuieli" — create / approve / delete expense reports with diurnă calc
 *
 * GL monografie (posted automatically):
 *   Grant:   542 D = 5311/5121 C
 *   Decont:  cheltuieli D + 4426 D = 542 C (diurna_neimpozabila only)
 *   Return:  5311/5121 D = 542 C
 *
 * Diurnă engine: min(A=2.5×23, B=salariu×3/zile_lucratoare) × zile_delegare.
 * Taxable excess is SHOWN (flagged) but NOT posted to GL.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { api } from "@/lib/tauri";
import type {
  ExpenseReportFull,
  DiurnaCalc,
  ExpenseLineInput,
} from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";

const todayISO = () => new Date().toISOString().slice(0, 10);

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

const statusBadge = (s: string) => {
  const map: Record<string, string> = {
    granted: "badge badge--blue",
    settled: "badge badge--green",
    returned: "badge badge--gray",
    draft: "badge badge--yellow",
    approved: "badge badge--green",
  };
  const labels: Record<string, string> = {
    granted: "Acordat",
    settled: "Decontat",
    returned: "Restituit",
    draft: "Ciornă",
    approved: "Aprobat",
  };
  return <span className={map[s] ?? "badge"}>{labels[s] ?? s}</span>;
};

const methodLabel = (m: string) => (m === "bank" ? "Transfer bancar" : "Numerar");

// ── Empty line template ───────────────────────────────────────────────────────
type LineForm = {
  category: ExpenseLineInput["category"];
  description: string;
  amount: string;
  vatAmount: string;
  accountCode: string;
};

const emptyLine = (): LineForm => ({
  category: "alte",
  description: "",
  amount: "",
  vatAmount: "",
  accountCode: "",
});

const CATEGORIES: { value: ExpenseLineInput["category"]; label: string }[] = [
  { value: "diurna", label: "Diurnă" },
  { value: "transport", label: "Transport" },
  { value: "cazare", label: "Cazare" },
  { value: "combustibil", label: "Combustibil" },
  { value: "alte", label: "Alte cheltuieli" },
];

// ── DiurnaPanel ───────────────────────────────────────────────────────────────
function DiurnaPanel({ calc }: { calc: DiurnaCalc }) {
  const impozabil = parseFloat(calc.diurnaImpozabila) > 0;
  return (
    <div className="decont-diurna-panel">
      <div className="decont-diurna-grid">
        <div>
          <div className="decont-diurna-label">Diurnă acordată</div>
          <div className="decont-diurna-value">{fmtRON(calc.diurnaAcordata)}</div>
        </div>
        <div>
          <div className="decont-diurna-label">Neimpozabilă</div>
          <div className="decont-diurna-value decont-diurna-green">{fmtRON(calc.diurnaNeimpozabila)}</div>
        </div>
        <div>
          <div className="decont-diurna-label">Impozabilă</div>
          <div className={`decont-diurna-value${impozabil ? " decont-diurna-red" : ""}`}>
            {fmtRON(calc.diurnaImpozabila)}
          </div>
        </div>
      </div>
      <div className="decont-diurna-caps">
        <span>Plafon A (2,5×23 lei): <strong>{fmtRON(calc.limitAZi)}/zi</strong></span>
        {" · "}
        <span>Plafon B (sal×3/{calc.workingDaysUsed} zile luc.): <strong>{fmtRON(calc.limitBZi)}/zi</strong></span>
        {" · "}
        <span>Cap aplicat: <strong>{fmtRON(calc.capZi)}/zi</strong></span>
      </div>
      {impozabil && (
        <div className="decont-diurna-warn">
          <strong>Atenție:</strong> Surplusul impozabil de {fmtRON(calc.diurnaImpozabila)} trebuie raportat
          la statul de salarii (inclusă în baza de calcul CAS/CASS/impozit). NU este postat automat în GL.
        </div>
      )}
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────
export function DeconturiPage() {
  useTranslation(); // initializes i18n context
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const [tab, setTab] = useState<"avansuri" | "deconturi">("avansuri");

  // ── Avansuri state ────────────────────────────────────────────────────────
  const [advEmployee, setAdvEmployee] = useState("");
  const [advAmount, setAdvAmount] = useState("");
  const [advDate, setAdvDate] = useState(todayISO());
  const [advMethod, setAdvMethod] = useState<"cash" | "bank">("cash");
  const [advNotes, setAdvNotes] = useState("");
  const [returnDate, setReturnDate] = useState(todayISO());
  const [returningId, setReturningId] = useState<string | null>(null);

  // ── Deconturi state ───────────────────────────────────────────────────────
  const [rEmployee, setREmployee] = useState("");
  const [rAdvanceId, setRAdvanceId] = useState("");
  const [rFrom, setRFrom] = useState("");
  const [rTo, setRTo] = useState("");
  const [rDest, setRDest] = useState("");
  const [rDays, setRDays] = useState("");
  const [rDiurna, setRDiurna] = useState("");
  const [rSalar, setRSalar] = useState("");
  const [rDate, setRDate] = useState(todayISO());
  const [rNotes, setRNotes] = useState("");
  const [lines, setLines] = useState<LineForm[]>([emptyLine()]);
  const [approveDate, setApproveDate] = useState(todayISO());
  const [approvingId, setApprovingId] = useState<string | null>(null);
  const [liveCalc, setLiveCalc] = useState<DiurnaCalc | null>(null);
  const [selectedReport, setSelectedReport] = useState<ExpenseReportFull | null>(null);

  // ── Queries ───────────────────────────────────────────────────────────────
  const { data: advances = [] } = useQuery({
    queryKey: ["treasury_advances", companyId ?? ""],
    queryFn: () => api.deconturi.listAdvances(companyId!),
    enabled: !!companyId,
  });

  const { data: reports = [] } = useQuery({
    queryKey: ["expense_reports", companyId ?? ""],
    queryFn: () => api.deconturi.listReports(companyId!),
    enabled: !!companyId,
  });

  const grantedAdvances = advances.filter((a) => a.status === "granted");

  // ── Advance mutations ─────────────────────────────────────────────────────
  const createAdvance = useMutation({
    mutationFn: () => {
      if (!companyId) throw new Error("Selectați o companie");
      return api.deconturi.createAdvance({
        companyId,
        employeeId: advEmployee.trim() || null,
        amount: advAmount,
        grantedDate: advDate,
        method: advMethod,
        notes: advNotes.trim() || null,
      });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      setAdvAmount("");
      setAdvEmployee("");
      setAdvNotes("");
      notify.success("Avans acordat și înregistrat în GL (542).");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la acordarea avansului.")),
  });

  const returnAdvance = useMutation({
    mutationFn: (id: string) => {
      if (!companyId) throw new Error("Selectați o companie");
      return api.deconturi.returnAdvance(id, companyId, returnDate);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      setReturningId(null);
      notify.success("Avans restituit. Nota GL (5311/542) înregistrată.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la restituire.")),
  });

  const deleteAdvance = useMutation({
    mutationFn: async (id: string) => {
      const ok = await confirm("Ștergeți definitiv acest avans?", { kind: "warning" });
      if (!ok) throw new Error("cancelled");
      return api.deconturi.deleteAdvance(id, companyId!);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      notify.success("Avans șters.");
    },
    onError: (e) => {
      if ((e as Error).message !== "cancelled") notify.error(formatError(e, "Eroare la ștergere."));
    },
  });

  // ── Diurnă live calc ──────────────────────────────────────────────────────
  const triggerDiurnaCalc = async () => {
    if (!companyId || !rDiurna || !rDays || !rSalar || !rFrom) {
      setLiveCalc(null);
      return;
    }
    const parts = rFrom.split("-");
    if (parts.length < 2) return;
    const year = parseInt(parts[0]);
    const month = parseInt(parts[1]);
    try {
      const calc = await api.deconturi.computeDiurna(
        companyId,
        rDiurna,
        parseInt(rDays),
        rSalar,
        year,
        month,
      );
      setLiveCalc(calc);
    } catch {
      setLiveCalc(null);
    }
  };

  // ── Report mutations ──────────────────────────────────────────────────────
  const createReport = useMutation({
    mutationFn: () => {
      if (!companyId) throw new Error("Selectați o companie");
      return api.deconturi.createReport({
        companyId,
        advanceId: rAdvanceId || null,
        employeeId: rEmployee.trim() || null,
        delegationFrom: rFrom || null,
        delegationTo: rTo || null,
        destination: rDest.trim() || null,
        days: rDays ? parseInt(rDays) : null,
        diurnaAcordata: rDiurna || null,
        salariuBaza: rSalar || null,
        reportDate: rDate,
        notes: rNotes.trim() || null,
        lines: lines
          .filter((l) => l.amount)
          .map((l) => ({
            category: l.category,
            description: l.description.trim() || null,
            amount: l.amount,
            vatAmount: l.vatAmount || null,
            accountCode: l.accountCode.trim() || null,
          })),
      });
    },
    onSuccess: (full) => {
      void qc.invalidateQueries({ queryKey: ["expense_reports", companyId ?? ""] });
      setREmployee("");
      setRAdvanceId("");
      setRFrom("");
      setRTo("");
      setRDest("");
      setRDays("");
      setRDiurna("");
      setRSalar("");
      setRNotes("");
      setLines([emptyLine()]);
      setLiveCalc(null);
      setSelectedReport(full);
      notify.success("Decont creat (ciornă).");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la creare decont.")),
  });

  const approveReport = useMutation({
    mutationFn: (id: string) => {
      if (!companyId) throw new Error("Selectați o companie");
      return api.deconturi.approveReport(id, companyId, approveDate);
    },
    onSuccess: (full) => {
      void qc.invalidateQueries({ queryKey: ["expense_reports", companyId ?? ""] });
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      setApprovingId(null);
      setSelectedReport(full);
      notify.success("Decont aprobat. Nota GL înregistrată.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la aprobare.")),
  });

  const deleteReport = useMutation({
    mutationFn: async (id: string) => {
      const ok = await confirm("Ștergeți definitiv acest decont?", { kind: "warning" });
      if (!ok) throw new Error("cancelled");
      return api.deconturi.deleteReport(id, companyId!);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["expense_reports", companyId ?? ""] });
      if (selectedReport?.report.id === approvingId) setSelectedReport(null);
      notify.success("Decont șters.");
    },
    onError: (e) => {
      if ((e as Error).message !== "cancelled") notify.error(formatError(e, "Eroare la ștergere."));
    },
  });

  const loadReportFull = async (id: string) => {
    if (!companyId) return;
    const full = await api.deconturi.getReport(id, companyId);
    setSelectedReport(full);
  };

  // ── Line helpers ──────────────────────────────────────────────────────────
  const updateLine = (i: number, patch: Partial<LineForm>) => {
    setLines((prev) => prev.map((l, idx) => (idx === i ? { ...l, ...patch } : l)));
  };

  const addLine = () => setLines((prev) => [...prev, emptyLine()]);
  const removeLine = (i: number) => setLines((prev) => prev.filter((_, idx) => idx !== i));

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="state-row muted">
          <p>Selectați o companie pentru a gestiona deconturile.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner">
      {/* ── Header ── */}
      <div className="page-head">
        <div>
          <h1 className="page-title">Deconturi & Avansuri de trezorerie</h1>
          <div className="page-sub">
            Avansuri 542 · Deconturi cu diurnă (HG 714/2018, CF art.76) · GL automat
          </div>
        </div>
      </div>

      {/* ── Tabs inside scr-card toolbar ── */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            <button
              className={`tab${tab === "avansuri" ? " active" : ""}`}
              onClick={() => setTab("avansuri")}
            >
              Avansuri de trezorerie
              {advances.filter((a) => a.status === "granted").length > 0 && (
                <span className="tab-badge">{advances.filter((a) => a.status === "granted").length}</span>
              )}
            </button>
            <button
              className={`tab${tab === "deconturi" ? " active" : ""}`}
              onClick={() => setTab("deconturi")}
            >
              Deconturi de cheltuieli
              {reports.filter((r) => r.status === "draft").length > 0 && (
                <span className="tab-badge">{reports.filter((r) => r.status === "draft").length}</span>
              )}
            </button>
          </div>
          <div className="spacer" />
        </div>

      {/* ══════════════════════════════════════════════════════════════════════
          TAB 1: AVANSURI
      ══════════════════════════════════════════════════════════════════════ */}
      {tab === "avansuri" && (
        <div className="panel-split">
          {/* ── Create form ── */}
          <div className="scr-card">
            <h3 className="card-title">Acordare avans</h3>
            <div className="field">
              <label>Angajat (opțional)</label>
              <input
                className="input"
                value={advEmployee}
                onChange={(e) => setAdvEmployee(e.target.value)}
                placeholder="Nume angajat"
              />
            </div>
            <div className="field">
              <label>Suma (RON) *</label>
              <input
                className="input"
                type="number"
                step="0.01"
                min="0"
                value={advAmount}
                onChange={(e) => setAdvAmount(e.target.value)}
                placeholder="0.00"
              />
            </div>
            <div className="field">
              <label>Data acordare *</label>
              <input
                className="input"
                type="date"
                value={advDate}
                onChange={(e) => setAdvDate(e.target.value)}
              />
            </div>
            <div className="field">
              <label>Mod plată</label>
              <select
                className="input"
                value={advMethod}
                onChange={(e) => setAdvMethod(e.target.value as "cash" | "bank")}
              >
                <option value="cash">Numerar (5311)</option>
                <option value="bank">Transfer bancar (5121)</option>
              </select>
            </div>
            <div className="field">
              <label>Note</label>
              <input
                className="input"
                value={advNotes}
                onChange={(e) => setAdvNotes(e.target.value)}
                placeholder="Opțional"
              />
            </div>
            <div className="form-hint">
              Nota GL: <code>542 D = {advMethod === "bank" ? "5121" : "5311"} C</code>
            </div>
            <button
              className="btn-dark"
              disabled={!advAmount || !advDate || createAdvance.isPending}
              onClick={() => createAdvance.mutate()}
            >
              {createAdvance.isPending ? "Se procesează..." : "Acordă avans"}
            </button>
          </div>

          {/* ── Advances list ── */}
          <div className="scr-card">
            <h3 className="card-title">Avansuri ({advances.length})</h3>
            {advances.length === 0 ? (
              <div className="table-empty">Niciun avans înregistrat.</div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Data</th>
                    <th>Angajat</th>
                    <th>Sumă</th>
                    <th>Mod</th>
                    <th>Status</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {advances.map((adv) => (
                    <tr key={adv.id}>
                      <td>{fmtDate(adv.grantedDate)}</td>
                      <td>{adv.employeeId ?? "—"}</td>
                      <td className="text-right">{fmtRON(adv.amount)}</td>
                      <td>{methodLabel(adv.method)}</td>
                      <td>{statusBadge(adv.status)}</td>
                      <td className="table-actions">
                        {adv.status === "granted" && (
                          <>
                            <button
                              className="btn btn-sm btn-outline"
                              onClick={() => setReturningId(adv.id)}
                            >
                              Restituie
                            </button>
                            <button
                              className="btn btn-sm btn-danger-outline"
                              onClick={() => deleteAdvance.mutate(adv.id)}
                            >
                              Șterge
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            {/* Return modal */}
            {returningId && (
              <div className="inline-form">
                <h4>Restituire avans</h4>
                <div className="field">
                  <label>Data restituire</label>
                  <input
                    className="input"
                    type="date"
                    value={returnDate}
                    onChange={(e) => setReturnDate(e.target.value)}
                  />
                </div>
                <div className="form-actions">
                  <button
                    className="btn-dark"
                    disabled={returnAdvance.isPending}
                    onClick={() => returnAdvance.mutate(returningId)}
                  >
                    {returnAdvance.isPending ? "Se procesează..." : "Confirmă restituire"}
                  </button>
                  <button className="btn btn-outline" onClick={() => setReturningId(null)}>
                    Anulează
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* ══════════════════════════════════════════════════════════════════════
          TAB 2: DECONTURI
      ══════════════════════════════════════════════════════════════════════ */}
      {tab === "deconturi" && (
        <div className="panel-split">
          {/* ── Create form ── */}
          <div className="scr-card">
            <h3 className="card-title">Decont nou</h3>
            <div className="field">
              <label>Angajat (opțional)</label>
              <input
                className="input"
                value={rEmployee}
                onChange={(e) => setREmployee(e.target.value)}
                placeholder="Nume angajat"
              />
            </div>
            <div className="field">
              <label>Avans legat (opțional)</label>
              <select
                className="input"
                value={rAdvanceId}
                onChange={(e) => setRAdvanceId(e.target.value)}
              >
                <option value="">— fără avans —</option>
                {grantedAdvances.map((a) => (
                  <option key={a.id} value={a.id}>
                    {fmtDate(a.grantedDate)} · {fmtRON(a.amount)} · {a.employeeId ?? "—"}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Destinație</label>
              <input
                className="input"
                value={rDest}
                onChange={(e) => setRDest(e.target.value)}
                placeholder="ex. București"
              />
            </div>
            <div className="form-row">
              <div className="field">
                <label>De la</label>
                <input
                  className="input"
                  type="date"
                  value={rFrom}
                  onChange={(e) => { setRFrom(e.target.value); setLiveCalc(null); }}
                />
              </div>
              <div className="field">
                <label>Până la</label>
                <input
                  className="input"
                  type="date"
                  value={rTo}
                  onChange={(e) => setRTo(e.target.value)}
                />
              </div>
            </div>
            <div className="form-row">
              <div className="field">
                <label>Zile delegare</label>
                <input
                  className="input"
                  type="number"
                  min="1"
                  value={rDays}
                  onChange={(e) => { setRDays(e.target.value); setLiveCalc(null); }}
                  placeholder="1"
                />
              </div>
              <div className="field">
                <label>Diurnă acordată (RON)</label>
                <input
                  className="input"
                  type="number"
                  step="0.01"
                  min="0"
                  value={rDiurna}
                  onChange={(e) => { setRDiurna(e.target.value); setLiveCalc(null); }}
                  placeholder="0.00"
                />
              </div>
            </div>
            <div className="field">
              <label>Salariu brut bază (RON) — pentru plafonul B</label>
              <input
                className="input"
                type="number"
                step="0.01"
                min="0"
                value={rSalar}
                onChange={(e) => { setRSalar(e.target.value); setLiveCalc(null); }}
                placeholder="ex. 4000.00"
              />
            </div>
            {rDiurna && rDays && rSalar && rFrom && (
              <div style={{ marginBottom: "0.5rem" }}>
                <button
                  className="btn btn-sm btn-outline"
                  onClick={triggerDiurnaCalc}
                  type="button"
                >
                  Calculează plafon diurnă
                </button>
              </div>
            )}
            {liveCalc && <DiurnaPanel calc={liveCalc} />}

            {/* Expense lines */}
            <div className="field">
              <label>Linii cheltuieli</label>
              <table className="expense-lines-table">
                <thead>
                  <tr>
                    <th>Categorie</th>
                    <th>Descriere</th>
                    <th>Sumă (RON)</th>
                    <th>TVA ded.</th>
                    <th>Cont</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((line, i) => (
                    <tr key={i}>
                      <td>
                        <select
                          className="input"
                          value={line.category}
                          onChange={(e) => updateLine(i, { category: e.target.value as LineForm["category"] })}
                        >
                          {CATEGORIES.map((c) => (
                            <option key={c.value} value={c.value}>{c.label}</option>
                          ))}
                        </select>
                      </td>
                      <td>
                        <input
                          className="input"
                          value={line.description}
                          onChange={(e) => updateLine(i, { description: e.target.value })}
                          placeholder="Descriere"
                        />
                      </td>
                      <td>
                        <input
                          className="input text-right"
                          type="number"
                          step="0.01"
                          min="0"
                          value={line.amount}
                          onChange={(e) => updateLine(i, { amount: e.target.value })}
                          placeholder="0.00"
                        />
                      </td>
                      <td>
                        <input
                          className="input text-right"
                          type="number"
                          step="0.01"
                          min="0"
                          value={line.vatAmount}
                          onChange={(e) => updateLine(i, { vatAmount: e.target.value })}
                          placeholder="—"
                        />
                      </td>
                      <td>
                        <input
                          className="input"
                          value={line.accountCode}
                          onChange={(e) => updateLine(i, { accountCode: e.target.value })}
                          placeholder="auto"
                        />
                      </td>
                      <td>
                        <button
                          className="btn btn-xs btn-danger-outline"
                          onClick={() => removeLine(i)}
                          disabled={lines.length === 1}
                          type="button"
                        >
                          ×
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <button className="btn btn-sm btn-outline" onClick={addLine} type="button">
                + Linie nouă
              </button>
            </div>

            <div className="form-row">
              <div className="field">
                <label>Data decont *</label>
                <input
                  className="input"
                  type="date"
                  value={rDate}
                  onChange={(e) => setRDate(e.target.value)}
                />
              </div>
              <div className="field">
                <label>Note</label>
                <input
                  className="input"
                  value={rNotes}
                  onChange={(e) => setRNotes(e.target.value)}
                  placeholder="Opțional"
                />
              </div>
            </div>

            <button
              className="btn-dark"
              disabled={!rDate || lines.every((l) => !l.amount) || createReport.isPending}
              onClick={() => createReport.mutate()}
            >
              {createReport.isPending ? "Se creează..." : "Salvează decont (ciornă)"}
            </button>
          </div>

          {/* ── Reports list + detail ── */}
          <div className="scr-card">
            <h3 className="card-title">Deconturi ({reports.length})</h3>
            {reports.length === 0 ? (
              <div className="table-empty">Niciun decont înregistrat.</div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>Data</th>
                    <th>Destinație</th>
                    <th>Angajat</th>
                    <th>Diurnă</th>
                    <th>Status</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {reports.map((r) => (
                    <tr
                      key={r.id}
                      className={selectedReport?.report.id === r.id ? "row--selected" : ""}
                      onClick={() => loadReportFull(r.id)}
                      style={{ cursor: "pointer" }}
                    >
                      <td>{fmtDate(r.reportDate)}</td>
                      <td>{r.destination ?? "—"}</td>
                      <td>{r.employeeId ?? "—"}</td>
                      <td className="text-right">
                        {r.diurnaAcordata ? fmtRON(r.diurnaAcordata) : "—"}
                        {r.diurnaImpozabila && parseFloat(r.diurnaImpozabila) > 0 && (
                          <span className="badge badge--red ml-1" title="Surplus impozabil">!</span>
                        )}
                      </td>
                      <td>{statusBadge(r.status)}</td>
                      <td className="table-actions" onClick={(e) => e.stopPropagation()}>
                        {r.status === "draft" && (
                          <>
                            <button
                              className="btn btn-sm btn-primary"
                              onClick={() => setApprovingId(r.id)}
                            >
                              Aprobă
                            </button>
                            <button
                              className="btn btn-sm btn-danger-outline"
                              onClick={() => deleteReport.mutate(r.id)}
                            >
                              Șterge
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            {/* Approve inline form */}
            {approvingId && (
              <div className="inline-form">
                <h4>Aprobare decont</h4>
                <div className="field">
                  <label>Data aprobare</label>
                  <input
                    className="input"
                    type="date"
                    value={approveDate}
                    onChange={(e) => setApproveDate(e.target.value)}
                  />
                </div>
                <div className="form-actions">
                  <button
                    className="btn-dark"
                    disabled={approveReport.isPending}
                    onClick={() => approveReport.mutate(approvingId)}
                  >
                    {approveReport.isPending ? "Se procesează..." : "Confirmă aprobare + GL"}
                  </button>
                  <button className="btn btn-outline" onClick={() => setApprovingId(null)}>
                    Anulează
                  </button>
                </div>
              </div>
            )}

            {/* Selected report detail */}
            {selectedReport && (
              <div className="decont-detail">
                <div className="decont-detail-header">
                  <h4>
                    Decont: {selectedReport.report.destination ?? "—"} ·{" "}
                    {fmtDate(selectedReport.report.reportDate)}{" "}
                    {statusBadge(selectedReport.report.status)}
                  </h4>
                  {selectedReport.report.delegationFrom && (
                    <div className="decont-detail-meta">
                      Delegare: {fmtDate(selectedReport.report.delegationFrom)} –{" "}
                      {fmtDate(selectedReport.report.delegationTo)} ·{" "}
                      {selectedReport.report.days} zile
                    </div>
                  )}
                </div>

                {selectedReport.diurnaCalc && (
                  <DiurnaPanel calc={selectedReport.diurnaCalc} />
                )}

                {selectedReport.lines.length > 0 && (
                  <div className="decont-lines">
                    <h5>Linii cheltuieli</h5>
                    <table className="scr-table">
                      <thead>
                        <tr>
                          <th>Categorie</th>
                          <th>Descriere</th>
                          <th>Sumă</th>
                          <th>TVA ded.</th>
                          <th>Cont</th>
                        </tr>
                      </thead>
                      <tbody>
                        {selectedReport.lines.map((l) => (
                          <tr key={l.id}>
                            <td>{CATEGORIES.find((c) => c.value === l.category)?.label ?? l.category}</td>
                            <td>{l.description ?? "—"}</td>
                            <td className="text-right">{fmtRON(l.amount)}</td>
                            <td className="text-right">{l.vatAmount ? fmtRON(l.vatAmount) : "—"}</td>
                            <td>{l.accountCode}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}

                <button
                  className="btn btn-sm btn-outline"
                  onClick={() => window.print()}
                  style={{ marginTop: "0.5rem" }}
                >
                  Tipărire decont
                </button>
              </div>
            )}
          </div>
        </div>
      )}
      </div>
    </div>
  );
}
