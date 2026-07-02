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
import { queryKeys } from "@/lib/queries";
import { Ic } from "@/components/shared/Ic";

const todayISO = () => new Date().toISOString().slice(0, 10);

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

const STATUS_BADGE_CLASS: Record<string, string> = {
  granted: "badge badge--blue",
  settled: "badge badge--green",
  returned: "badge badge--gray",
  draft: "badge badge--yellow",
  approved: "badge badge--green",
};

/** Status badge — labels live in locales (deconturi.status.*). */
function StatusBadge({ s }: { s: string }) {
  const { t } = useTranslation();
  return (
    <span className={STATUS_BADGE_CLASS[s] ?? "badge"}>
      {t(`deconturi.status.${s}`, { defaultValue: s })}
    </span>
  );
}

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

/** Category codes — the human labels live in locales (deconturi.categories.*). */
const CATEGORY_VALUES: ExpenseLineInput["category"][] = [
  "diurna",
  "transport",
  "cazare",
  "combustibil",
  "alte",
];

// ── DiurnaPanel ───────────────────────────────────────────────────────────────
function DiurnaPanel({ calc }: { calc: DiurnaCalc }) {
  const { t } = useTranslation();
  const impozabil = parseFloat(calc.diurnaImpozabila) > 0;
  return (
    <div className="decont-diurna-panel">
      <div className="decont-diurna-grid">
        <div>
          <div className="decont-diurna-label">{t("deconturi.diurna.granted")}</div>
          <div className="decont-diurna-value">{fmtRON(calc.diurnaAcordata)}</div>
        </div>
        <div>
          <div className="decont-diurna-label">{t("deconturi.diurna.nonTaxable")}</div>
          <div className="decont-diurna-value decont-diurna-green">{fmtRON(calc.diurnaNeimpozabila)}</div>
        </div>
        <div>
          <div className="decont-diurna-label">{t("deconturi.diurna.taxable")}</div>
          <div className={`decont-diurna-value${impozabil ? " decont-diurna-red" : ""}`}>
            {fmtRON(calc.diurnaImpozabila)}
          </div>
        </div>
      </div>
      <div className="decont-diurna-caps">
        <span>{t("deconturi.diurna.capALabel")} <strong>{t("deconturi.diurna.perDay", { value: fmtRON(calc.limitAZi) })}</strong></span>
        {" · "}
        <span>{t("deconturi.diurna.capBLabel", { days: calc.workingDaysUsed })} <strong>{t("deconturi.diurna.perDay", { value: fmtRON(calc.limitBZi) })}</strong></span>
        {" · "}
        <span>{t("deconturi.diurna.capAppliedLabel")} <strong>{t("deconturi.diurna.perDay", { value: fmtRON(calc.capZi) })}</strong></span>
      </div>
      {impozabil && (
        <div className="decont-diurna-warn">
          <strong>{t("deconturi.diurna.warnTitle")}</strong>{" "}
          {t("deconturi.diurna.warnBody", { amount: fmtRON(calc.diurnaImpozabila) })}
        </div>
      )}
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────
export function DeconturiPage() {
  const { t } = useTranslation();
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
  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

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

  const activeCompany = companies.find((c) => c.id === companyId);
  const companyName = activeCompany?.legalName ?? "—";

  const grantedAdvances = advances.filter((a) => a.status === "granted");

  // ── Advance mutations ─────────────────────────────────────────────────────
  const createAdvance = useMutation({
    mutationFn: () => {
      if (!companyId) throw new Error(t("deconturi.selectCompanyErr"));
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
      notify.success(t("deconturi.advances.notify.granted"));
    },
    onError: (e) => notify.error(formatError(e, t("deconturi.advances.notify.grantError"))),
  });

  const returnAdvance = useMutation({
    mutationFn: (id: string) => {
      if (!companyId) throw new Error(t("deconturi.selectCompanyErr"));
      return api.deconturi.returnAdvance(id, companyId, returnDate);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      setReturningId(null);
      notify.success(t("deconturi.advances.notify.returned"));
    },
    onError: (e) => notify.error(formatError(e, t("deconturi.advances.notify.returnError"))),
  });

  const deleteAdvance = useMutation({
    mutationFn: async (id: string) => {
      const ok = await confirm(t("deconturi.advances.confirmDelete"), { kind: "warning" });
      if (!ok) throw new Error("cancelled");
      return api.deconturi.deleteAdvance(id, companyId!);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      notify.success(t("deconturi.advances.notify.deleted"));
    },
    onError: (e) => {
      if ((e as Error).message !== "cancelled") notify.error(formatError(e, t("deconturi.advances.notify.deleteError")));
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
      if (!companyId) throw new Error(t("deconturi.selectCompanyErr"));
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
      notify.success(t("deconturi.reports.notify.created"));
    },
    onError: (e) => notify.error(formatError(e, t("deconturi.reports.notify.createError"))),
  });

  const approveReport = useMutation({
    mutationFn: (id: string) => {
      if (!companyId) throw new Error(t("deconturi.selectCompanyErr"));
      return api.deconturi.approveReport(id, companyId, approveDate);
    },
    onSuccess: (full) => {
      void qc.invalidateQueries({ queryKey: ["expense_reports", companyId ?? ""] });
      void qc.invalidateQueries({ queryKey: ["treasury_advances", companyId ?? ""] });
      setApprovingId(null);
      setSelectedReport(full);
      notify.success(t("deconturi.reports.notify.approved"));
    },
    onError: (e) => notify.error(formatError(e, t("deconturi.reports.notify.approveError"))),
  });

  const deleteReport = useMutation({
    mutationFn: async (id: string) => {
      const ok = await confirm(t("deconturi.reports.confirmDelete"), { kind: "warning" });
      if (!ok) throw new Error("cancelled");
      return api.deconturi.deleteReport(id, companyId!);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["expense_reports", companyId ?? ""] });
      if (selectedReport?.report.id === approvingId) setSelectedReport(null);
      notify.success(t("deconturi.reports.notify.deleted"));
    },
    onError: (e) => {
      if ((e as Error).message !== "cancelled") notify.error(formatError(e, t("deconturi.reports.notify.deleteError")));
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
          <p>{t("deconturi.selectCompany")}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* ── Header ── */}
      <div className="page-head">
        <div>
          <h1>{t("deconturi.title")}</h1>
          <p className="sub">
            {t("deconturi.sub", { company: companyName })}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="btn-dark"
            disabled={!advAmount || !advDate || createAdvance.isPending}
            onClick={() => { setTab("avansuri"); createAdvance.mutate(); }}
          >
            <Ic name="plus" /> {t("deconturi.grantForm.submit")}
          </button>
        </div>
      </div>

      {/* ── Tabs card ── */}
      <div className="scr-card" style={{ marginBottom: 16 }}>
        <div className="scr-toolbar">
          <div className="tabs">
            <div
              className={"tab" + (tab === "avansuri" ? " active" : "")}
              onClick={() => setTab("avansuri")}
            >
              {t("deconturi.tabs.advances")}<span className="cnt">{advances.length}</span>
            </div>
            <div
              className={"tab" + (tab === "deconturi" ? " active" : "")}
              onClick={() => setTab("deconturi")}
            >
              {t("deconturi.tabs.reports")}<span className="cnt">{reports.length}</span>
            </div>
          </div>
          <div className="spacer" />
        </div>

        {/* ── "Acorda avans" inline form (always visible, export style) ── */}
        <div style={{ padding: "4px 16px 0" }}>
          <div style={{ fontSize: 13, fontWeight: 600, padding: "12px 0 2px" }}>{t("deconturi.grantForm.title")}</div>
        </div>
        <div className="fgrid-form">
          <div className="field">
            <label>{t("deconturi.grantForm.employee")}</label>
            <input
              className="input"
              value={advEmployee}
              onChange={(e) => setAdvEmployee(e.target.value)}
              placeholder={t("deconturi.grantForm.employeePh")}
            />
          </div>
          <div className="field">
            <label>{t("deconturi.grantForm.amount")}</label>
            <input
              className="input num"
              type="number"
              step="0.01"
              min="0"
              value={advAmount}
              onChange={(e) => setAdvAmount(e.target.value)}
              placeholder={t("deconturi.grantForm.amountPh")}
              style={{ textAlign: "right" }}
            />
          </div>
          <div className="field">
            <label>{t("deconturi.grantForm.date")}</label>
            <input
              className="input num"
              type="date"
              value={advDate}
              onChange={(e) => setAdvDate(e.target.value)}
            />
          </div>
          <div className="field">
            <label>{t("deconturi.grantForm.purpose")}</label>
            <input
              className="input"
              value={advNotes}
              onChange={(e) => setAdvNotes(e.target.value)}
              placeholder={t("deconturi.grantForm.purposePh")}
            />
          </div>
          <div className="field">
            <label>{t("deconturi.grantForm.method")}</label>
            <select
              className="select"
              value={advMethod}
              onChange={(e) => setAdvMethod(e.target.value as "cash" | "bank")}
            >
              <option value="cash">{t("deconturi.grantForm.methodCash")}</option>
              <option value="bank">{t("deconturi.grantForm.methodBank")}</option>
            </select>
          </div>
        </div>
        <div style={{ display: "flex", justifyContent: "flex-end", padding: "4px 16px 16px" }}>
          <button
            className="btn-dark"
            disabled={!advAmount || !advDate || createAdvance.isPending}
            onClick={() => createAdvance.mutate()}
          >
            <Ic name="banknotes" />
            {createAdvance.isPending ? t("deconturi.grantForm.processing") : t("deconturi.grantForm.submit")}
          </button>
        </div>
      </div>

      {/* ── Avansuri table card ── */}
      {tab === "avansuri" && (
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("deconturi.advances.listTitle")}</div>
          </div>
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 130 }}>{t("deconturi.advances.table.date")}</th>
                <th>{t("deconturi.advances.table.employee")}</th>
                <th>{t("deconturi.advances.table.purpose")}</th>
                <th className="r" style={{ width: 130 }}>{t("deconturi.advances.table.amount")}</th>
                <th className="r" style={{ width: 130 }}>{t("deconturi.advances.table.balance")}</th>
                <th style={{ width: 120 }}>{t("deconturi.advances.table.status")}</th>
                <th style={{ width: 120 }}></th>
              </tr>
            </thead>
            {advances.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={7} style={{ padding: 0 }}>
                    <div className="empty">
                      <div className="ei"><Ic name="banknotes" /></div>
                      <b>{t("deconturi.advances.empty.title")}</b>
                      {t("deconturi.advances.empty.hint")}
                    </div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <tbody>
                {advances.map((adv) => (
                  <tr key={adv.id}>
                    <td>{fmtDate(adv.grantedDate)}</td>
                    <td>{adv.employeeId ?? "—"}</td>
                    <td>{adv.notes ?? "—"}</td>
                    <td className="r">{fmtRON(adv.amount)}</td>
                    <td className="r">{fmtRON(adv.amount)}</td>
                    <td><StatusBadge s={adv.status} /></td>
                    <td>
                      {adv.status === "granted" && (
                        <div style={{ display: "flex", gap: 4 }}>
                          <button
                            className="btn btn-sm btn-outline"
                            onClick={() => setReturningId(adv.id)}
                          >
                            {t("deconturi.advances.return")}
                          </button>
                          <button
                            className="btn btn-sm btn-danger-outline"
                            onClick={() => deleteAdvance.mutate(adv.id)}
                          >
                            {t("deconturi.advances.delete")}
                          </button>
                        </div>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            )}
          </table>

          {/* Return inline form */}
          {returningId && (
            <div className="inline-form">
              <h4>{t("deconturi.advances.returnForm.title")}</h4>
              <div className="field">
                <label>{t("deconturi.advances.returnForm.date")}</label>
                <input
                  className="input"
                  type="date"
                  value={returnDate}
                  onChange={(e) => setReturnDate(e.target.value)}
                />
              </div>
              <div style={{ display: "flex", justifyContent: "flex-end", gap: "0.5rem", marginTop: "0.75rem" }}>
                <button className="btn btn-outline" onClick={() => setReturningId(null)}>
                  {t("deconturi.advances.returnForm.cancel")}
                </button>
                <button
                  className="btn-dark"
                  disabled={returnAdvance.isPending}
                  onClick={() => returnAdvance.mutate(returningId)}
                >
                  {returnAdvance.isPending ? t("deconturi.advances.returnForm.processing") : t("deconturi.advances.returnForm.confirm")}
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ── Deconturi tab ── */}
      {tab === "deconturi" && (
        <>
          {/* ── Create form card ── */}
          <div className="scr-card" style={{ marginBottom: 16 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("deconturi.reportForm.title")}</div>
            </div>
            <div className="fgrid-form">
              <div className="field">
                <label>{t("deconturi.reportForm.employee")}</label>
                <input
                  className="input"
                  value={rEmployee}
                  onChange={(e) => setREmployee(e.target.value)}
                  placeholder={t("deconturi.reportForm.employeePh")}
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.advance")}</label>
                <select
                  className="select"
                  value={rAdvanceId}
                  onChange={(e) => setRAdvanceId(e.target.value)}
                >
                  <option value="">{t("deconturi.reportForm.noAdvance")}</option>
                  {grantedAdvances.map((a) => (
                    <option key={a.id} value={a.id}>
                      {fmtDate(a.grantedDate)} · {fmtRON(a.amount)} · {a.employeeId ?? "—"}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.destination")}</label>
                <input
                  className="input"
                  value={rDest}
                  onChange={(e) => setRDest(e.target.value)}
                  placeholder={t("deconturi.reportForm.destinationPh")}
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.from")}</label>
                <input
                  className="input"
                  type="date"
                  value={rFrom}
                  onChange={(e) => { setRFrom(e.target.value); setLiveCalc(null); }}
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.to")}</label>
                <input
                  className="input"
                  type="date"
                  value={rTo}
                  onChange={(e) => setRTo(e.target.value)}
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.days")}</label>
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
                <label>{t("deconturi.reportForm.diurna")}</label>
                <input
                  className="input num"
                  type="number"
                  step="0.01"
                  min="0"
                  value={rDiurna}
                  onChange={(e) => { setRDiurna(e.target.value); setLiveCalc(null); }}
                  placeholder="0.00"
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.salary")}</label>
                <input
                  className="input num"
                  type="number"
                  step="0.01"
                  min="0"
                  value={rSalar}
                  onChange={(e) => { setRSalar(e.target.value); setLiveCalc(null); }}
                  placeholder="ex. 4000.00"
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.reportDate")}</label>
                <input
                  className="input"
                  type="date"
                  value={rDate}
                  onChange={(e) => setRDate(e.target.value)}
                />
              </div>
              <div className="field">
                <label>{t("deconturi.reportForm.notes")}</label>
                <input
                  className="input"
                  value={rNotes}
                  onChange={(e) => setRNotes(e.target.value)}
                  placeholder={t("deconturi.reportForm.notesPh")}
                />
              </div>
            </div>

            {rDiurna && rDays && rSalar && rFrom && (
              <div style={{ padding: "0 16px 8px" }}>
                <button
                  className="btn btn-sm btn-outline"
                  onClick={triggerDiurnaCalc}
                  type="button"
                >
                  {t("deconturi.reportForm.computeCap")}
                </button>
              </div>
            )}
            {liveCalc && (
              <div style={{ padding: "0 16px 8px" }}>
                <DiurnaPanel calc={liveCalc} />
              </div>
            )}

            {/* Expense lines */}
            <div style={{ padding: "0 16px 8px" }}>
              <div style={{ fontSize: 13, fontWeight: 600, padding: "4px 0 8px" }}>{t("deconturi.reportForm.linesTitle")}</div>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("deconturi.reportForm.lines.category")}</th>
                    <th>{t("deconturi.reportForm.lines.description")}</th>
                    <th className="r" style={{ width: 120 }}>{t("deconturi.reportForm.lines.amount")}</th>
                    <th className="r" style={{ width: 100 }}>{t("deconturi.reportForm.lines.vat")}</th>
                    <th style={{ width: 100 }}>{t("deconturi.reportForm.lines.account")}</th>
                    <th style={{ width: 40 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((line, i) => (
                    <tr key={i}>
                      <td>
                        <select
                          className="select"
                          value={line.category}
                          onChange={(e) => updateLine(i, { category: e.target.value as LineForm["category"] })}
                        >
                          {CATEGORY_VALUES.map((c) => (
                            <option key={c} value={c}>{t(`deconturi.categories.${c}`)}</option>
                          ))}
                        </select>
                      </td>
                      <td>
                        <input
                          className="input"
                          value={line.description}
                          onChange={(e) => updateLine(i, { description: e.target.value })}
                          placeholder={t("deconturi.reportForm.lines.descriptionPh")}
                        />
                      </td>
                      <td>
                        <input
                          className="input num"
                          type="number"
                          step="0.01"
                          min="0"
                          value={line.amount}
                          onChange={(e) => updateLine(i, { amount: e.target.value })}
                          placeholder="0.00"
                          style={{ textAlign: "right" }}
                        />
                      </td>
                      <td>
                        <input
                          className="input num"
                          type="number"
                          step="0.01"
                          min="0"
                          value={line.vatAmount}
                          onChange={(e) => updateLine(i, { vatAmount: e.target.value })}
                          placeholder="—"
                          style={{ textAlign: "right" }}
                        />
                      </td>
                      <td>
                        <input
                          className="input"
                          value={line.accountCode}
                          onChange={(e) => updateLine(i, { accountCode: e.target.value })}
                          placeholder={t("deconturi.reportForm.lines.accountPh")}
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
              <button className="btn btn-sm btn-outline" onClick={addLine} type="button" style={{ marginTop: 8 }}>
                {t("deconturi.reportForm.lines.add")}
              </button>
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", padding: "4px 16px 16px" }}>
              <button
                className="btn-dark"
                disabled={!rDate || lines.every((l) => !l.amount) || createReport.isPending}
                onClick={() => createReport.mutate()}
              >
                <Ic name="banknotes" />
                {createReport.isPending ? t("deconturi.reportForm.creating") : t("deconturi.reportForm.submit")}
              </button>
            </div>
          </div>

          {/* ── Deconturi list card ── */}
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">{t("deconturi.reports.listTitle")}</div>
            </div>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 130 }}>{t("deconturi.reports.table.date")}</th>
                  <th>{t("deconturi.reports.table.employee")}</th>
                  <th>{t("deconturi.reports.table.destination")}</th>
                  <th className="r" style={{ width: 130 }}>{t("deconturi.reports.table.diurna")}</th>
                  <th style={{ width: 120 }}>{t("deconturi.reports.table.status")}</th>
                  <th style={{ width: 140 }}></th>
                </tr>
              </thead>
              {reports.length === 0 ? (
                <tbody>
                  <tr>
                    <td colSpan={6} style={{ padding: 0 }}>
                      <div className="empty">
                        <div className="ei"><Ic name="banknotes" /></div>
                        <b>{t("deconturi.reports.empty.title")}</b>
                        {t("deconturi.reports.empty.hint")}
                      </div>
                    </td>
                  </tr>
                </tbody>
              ) : (
                <tbody>
                  {reports.map((r) => (
                    <tr
                      key={r.id}
                      className={selectedReport?.report.id === r.id ? "row--selected" : ""}
                      onClick={() => loadReportFull(r.id)}
                      style={{ cursor: "pointer" }}
                    >
                      <td>{fmtDate(r.reportDate)}</td>
                      <td>{r.employeeId ?? "—"}</td>
                      <td>{r.destination ?? "—"}</td>
                      <td className="r">
                        {r.diurnaAcordata ? fmtRON(r.diurnaAcordata) : "—"}
                        {r.diurnaImpozabila && parseFloat(r.diurnaImpozabila) > 0 && (
                          <span className="badge badge--red ml-1" title={t("deconturi.reports.taxableExcess")}>!</span>
                        )}
                      </td>
                      <td><StatusBadge s={r.status} /></td>
                      <td onClick={(e) => e.stopPropagation()}>
                        {r.status === "draft" && (
                          <div style={{ display: "flex", gap: 4 }}>
                            <button
                              className="btn btn-sm btn-primary"
                              onClick={() => setApprovingId(r.id)}
                            >
                              {t("deconturi.reports.approve")}
                            </button>
                            <button
                              className="btn btn-sm btn-danger-outline"
                              onClick={() => deleteReport.mutate(r.id)}
                            >
                              {t("deconturi.reports.delete")}
                            </button>
                          </div>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              )}
            </table>

            {/* Approve inline form */}
            {approvingId && (
              <div className="inline-form">
                <h4>{t("deconturi.reports.approveForm.title")}</h4>
                <div className="field">
                  <label>{t("deconturi.reports.approveForm.date")}</label>
                  <input
                    className="input"
                    type="date"
                    value={approveDate}
                    onChange={(e) => setApproveDate(e.target.value)}
                  />
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: "0.5rem", marginTop: "0.75rem" }}>
                  <button className="btn btn-outline" onClick={() => setApprovingId(null)}>
                    {t("deconturi.reports.approveForm.cancel")}
                  </button>
                  <button
                    className="btn-dark"
                    disabled={approveReport.isPending}
                    onClick={() => approveReport.mutate(approvingId)}
                  >
                    {approveReport.isPending ? t("deconturi.reports.approveForm.processing") : t("deconturi.reports.approveForm.confirm")}
                  </button>
                </div>
              </div>
            )}

            {/* Selected report detail */}
            {selectedReport && (
              <div className="decont-detail">
                <div className="decont-detail-header">
                  <h4>
                    {t("deconturi.reports.detail.title", {
                      destination: selectedReport.report.destination ?? "—",
                      date: fmtDate(selectedReport.report.reportDate),
                    })}{" "}
                    <StatusBadge s={selectedReport.report.status} />
                  </h4>
                  {selectedReport.report.delegationFrom && (
                    <div className="decont-detail-meta">
                      {t("deconturi.reports.detail.delegation", {
                        from: fmtDate(selectedReport.report.delegationFrom),
                        to: fmtDate(selectedReport.report.delegationTo),
                        days: selectedReport.report.days,
                      })}
                    </div>
                  )}
                </div>

                {selectedReport.diurnaCalc && (
                  <DiurnaPanel calc={selectedReport.diurnaCalc} />
                )}

                {selectedReport.lines.length > 0 && (
                  <div className="decont-lines">
                    <h5>{t("deconturi.reports.detail.linesTitle")}</h5>
                    <table className="scr-table">
                      <thead>
                        <tr>
                          <th>{t("deconturi.reports.detail.lines.category")}</th>
                          <th>{t("deconturi.reports.detail.lines.description")}</th>
                          <th className="r">{t("deconturi.reports.detail.lines.amount")}</th>
                          <th className="r">{t("deconturi.reports.detail.lines.vat")}</th>
                          <th>{t("deconturi.reports.detail.lines.account")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {selectedReport.lines.map((l) => (
                          <tr key={l.id}>
                            <td>{t(`deconturi.categories.${l.category}`, { defaultValue: l.category })}</td>
                            <td>{l.description ?? "—"}</td>
                            <td className="r">{fmtRON(l.amount)}</td>
                            <td className="r">{l.vatAmount ? fmtRON(l.vatAmount) : "—"}</td>
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
                  {t("deconturi.reports.detail.print")}
                </button>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
