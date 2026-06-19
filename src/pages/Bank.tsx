/**
 * Bank.tsx — Registru de casă (cod 14-4-7A).
 *
 * Reuses the existing `api.gl.generalLedger` command and renders the sheet for
 * account 5311 (Casa în lei) as the legal daily cash register:
 *   - Opening balance (sold inițial)
 *   - Movement rows grouped by day, with Încasări (debit) / Plăți (credit) / Sold
 *   - "Total zi" subtotal row after each day's entries
 *   - Closing balance (sold final)
 *
 * Print / Save as PDF reuses the XmlViewerModal pattern:
 *   - wraps the printable `.docv` element with `buildStandaloneHtml`
 *   - demo mode → `window.open`; Tauri → `api.declarations.openDocInBrowser`
 */

import { Fragment, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { MonthPicker } from "@/components/shared/MonthPicker";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { isDemoMode } from "@/lib/demo";
import { buildStandaloneHtml } from "@/lib/doc-render/doc-html";
import type { LedgerAccount, LedgerEntry } from "@/types";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
/** Formatează numai ISO-uri (YYYY-MM-DD); orice alt format trece neatins. */
const fmtD = (s: string) => (/^\d{4}-\d{2}-\d{2}/.test(s) ? fmtRoDate(s.slice(0, 10)) : s || "—");

function periodDateRange(year: number, month: number): { dateFrom: string; dateTo: string } {
  const mm      = String(month).padStart(2, "0");
  const lastDay = new Date(year, month, 0).getDate();
  return {
    dateFrom: `${year}-${mm}-01`,
    dateTo:   `${year}-${mm}-${String(lastDay).padStart(2, "0")}`,
  };
}

/** Group ledger entries by their date (ISO date key → entries array). */
function groupByDay(entries: LedgerEntry[]): Array<{ day: string; rows: LedgerEntry[] }> {
  const map = new Map<string, LedgerEntry[]>();
  for (const e of entries) {
    const key = e.date.slice(0, 10); // normalize to YYYY-MM-DD
    if (!map.has(key)) map.set(key, []);
    map.get(key)!.push(e);
  }
  return Array.from(map.entries()).map(([day, rows]) => ({ day, rows }));
}

/** Cash account 5311 is a debit-balance account: balance = debit − credit (≥ 0 in normal operation). */
const cashBalance = (debit: string | number, credit: string | number) =>
  parseDec(debit) - parseDec(credit);

// ─── Component ───────────────────────────────────────────────────────────────

export function BankPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const MONTHS = [
    t("gl.months.jan"), t("gl.months.feb"), t("gl.months.mar"),
    t("gl.months.apr"), t("gl.months.may"), t("gl.months.jun"),
    t("gl.months.jul"), t("gl.months.aug"), t("gl.months.sep"),
    t("gl.months.oct"), t("gl.months.nov"), t("gl.months.dec"),
  ];

  const now = new Date();
  const [selectedYear,  setSelectedYear]  = useState(now.getFullYear());
  const [selectedMonth, setSelectedMonth] = useState(now.getMonth() + 1);
  const [openPop,       setOpenPop]       = useState(false);
  const [loading,       setLoading]       = useState(false);
  const [account,       setAccount]       = useState<LedgerAccount | null | undefined>(undefined);

  const { dateFrom, dateTo } = periodDateRange(selectedYear, selectedMonth);
  const periodLabel = `${MONTHS[selectedMonth - 1]} ${selectedYear}`;

  // Track loaded context so we don't double-load.
  const attempted = useRef<string>("");

  // Close MonthPicker on outside click.
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop(false);
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  // ── Auto-load when company / period changes ────────────────────────────────
  useEffect(() => {
    if (!activeCompanyId) return;
    const key = `${activeCompanyId}|${dateFrom}`;
    if (attempted.current === key) return;
    attempted.current = key;
    setAccount(undefined); // loading sentinel
    void load(activeCompanyId, dateFrom, dateTo);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCompanyId, dateFrom]);

  async function load(companyId: string, from: string, to: string) {
    setLoading(true);
    try {
      const sheets = await api.gl.generalLedger(companyId, from, to);
      const cash = sheets.find((s) => s.accountCode === "5311") ?? null;
      setAccount(cash);
    } catch (err) {
      notify.error(formatError(err, t("bank.loadError")));
      setAccount(null);
    } finally {
      setLoading(false);
    }
  }

  // ── Print / Save PDF ───────────────────────────────────────────────────────
  const printRef = useRef<HTMLDivElement>(null);

  const handlePrint = async () => {
    const el = printRef.current;
    if (!el) {
      notify.error(t("bank.printError"));
      return;
    }
    const fileName = `registru-casa-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.html`;
    const html = buildStandaloneHtml(t("bank.title"), el.outerHTML);
    if (isDemoMode()) {
      const w = window.open("", "_blank");
      if (w) { w.document.write(html); w.document.close(); }
      return;
    }
    try {
      await api.declarations.openDocInBrowser(html, fileName);
    } catch (err) {
      notify.error(formatError(err, t("bank.printError")));
    }
  };

  // ── No company ────────────────────────────────────────────────────────────
  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("bank.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("bank.noCompany")}
        </div>
      </div>
    );
  }

  // ── Derived data ──────────────────────────────────────────────────────────
  const days = account ? groupByDay(account.entries) : [];
  const openingBal = account ? cashBalance(account.openingDebit, account.openingCredit) : 0;
  const closingBal = account ? cashBalance(account.closingDebit, account.closingCredit) : 0;

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("bank.title")}</h1>
          <p className="sub">{periodLabel} · {t("bank.account5311")}</p>
        </div>
        <div className="head-actions">
          {/* Period picker */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(!openPop)}
            >
              <Ic name="calendar" />
              {periodLabel}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop && (
              <MonthPicker
                year={selectedYear}
                month={selectedMonth}
                monthsFull={MONTHS}
                prevYearLabel={t("declarations.periodPop.prevYear")}
                nextYearLabel={t("declarations.periodPop.nextYear")}
                onPrevYear={() => setSelectedYear(selectedYear - 1)}
                onNextYear={() => setSelectedYear(selectedYear + 1)}
                onPick={(m) => { setSelectedMonth(m); setOpenPop(false); }}
              />
            )}
          </div>

          {/* Print button */}
          <button
            className="pill-btn"
            disabled={!account || loading}
            onClick={() => void handlePrint()}
          >
            <Ic name="dl" />
            {t("bank.print")}
          </button>
        </div>
      </div>

      {/* Content card */}
      <div className="scr-card">
        {loading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>
            {t("gl.common.loading")}
          </div>
        ) : account === null || (account === undefined && !loading) ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("bank.empty")}
          </div>
        ) : account ? (
          /* Printable registru — wrapped in .docv so buildStandaloneHtml picks up the right CSS */
          <div className="docv" ref={printRef}>
            {/* Document header */}
            <div className="docv-title" style={{ marginBottom: 2 }}>{t("bank.title")}</div>
            <div
              className="docv-title docv-title-sub"
              style={{ fontSize: 13, fontWeight: 500, textAlign: "center", marginBottom: 16 }}
            >
              {t("bank.account5311")} · {periodLabel}
            </div>

            {/* Opening balance row */}
            <table className="scr-table" style={{ marginBottom: 0 }}>
              <thead>
                <tr>
                  <th>{t("bank.colData")}</th>
                  <th>{t("bank.colDocument")}</th>
                  <th>{t("bank.colExplicatie")}</th>
                  <th>{t("bank.colContrapartida")}</th>
                  <th className="r">{t("bank.colIncasari")}</th>
                  <th className="r">{t("bank.colPlati")}</th>
                  <th className="r">{t("bank.colSold")}</th>
                </tr>
              </thead>
              <tbody>
                {/* Sold inițial */}
                <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                  <td colSpan={6}>{t("bank.soldInitial")}</td>
                  <td className="r num">{fmtRON(openingBal)}</td>
                </tr>

                {/* Day groups */}
                {days.map(({ day, rows }) => {
                  const dayDebit  = rows.reduce((s, r) => s + parseDec(r.debit),  0);
                  const dayCredit = rows.reduce((s, r) => s + parseDec(r.credit), 0);
                  // The end-of-day sold is the running balance of the last entry in the day.
                  const eodBalance = parseDec(rows[rows.length - 1].balance);

                  return rows.map((entry, idx) => (
                    <Fragment key={`${day}-${idx}`}>
                      <tr>
                        <td className="num">{fmtD(day)}</td>
                        <td><span className="doc">{entry.document || "—"}</span></td>
                        <td>{entry.explanation || "—"}</td>
                        <td>{entry.contra ? <span className="doc">{entry.contra}</span> : <span className="muted">—</span>}</td>
                        <td className="r num">
                          {parseDec(entry.debit) > 0 ? fmtRON(entry.debit) : <span className="muted">—</span>}
                        </td>
                        <td className="r num">
                          {parseDec(entry.credit) > 0 ? fmtRON(entry.credit) : <span className="muted">—</span>}
                        </td>
                        <td className="r num">{fmtRON(parseDec(entry.balance))}</td>
                      </tr>
                      {/* Total zi row after last entry of the day */}
                      {idx === rows.length - 1 && (
                        <tr style={{ background: "var(--fill)", fontStyle: "italic" }}>
                          <td colSpan={4} style={{ paddingLeft: 12 }}>{t("bank.totalZi")} — {fmtD(day)}</td>
                          <td className="r num">{dayDebit > 0 ? fmtRON(dayDebit) : <span className="muted">—</span>}</td>
                          <td className="r num">{dayCredit > 0 ? fmtRON(dayCredit) : <span className="muted">—</span>}</td>
                          <td className="r num">{fmtRON(eodBalance)}</td>
                        </tr>
                      )}
                    </Fragment>
                  ));
                })}

                {/* Sold final */}
                <tr style={{ background: "var(--bg-table-header)", fontWeight: 700 }}>
                  <td colSpan={4}>{t("bank.soldFinal")}</td>
                  <td className="r num">{fmtRON(account.totalDebit)}</td>
                  <td className="r num">{fmtRON(account.totalCredit)}</td>
                  <td className="r num">{fmtRON(closingBal)}</td>
                </tr>
              </tbody>
            </table>
          </div>
        ) : null}
      </div>
    </div>
  );
}
