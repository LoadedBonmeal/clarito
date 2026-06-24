/**
 * Bank.tsx — Registru de casă (cod 14-4-7A / cod 14-4-7/aA).
 *
 * Renders TWO cash registers from the general ledger:
 *   5311 (Casa în lei)   — daily cash register, cod 14-4-7A
 *   5314 (Casa în valută) — foreign-currency cash register, cod 14-4-7/aA
 *
 * A toggle in the toolbar switches between the two views.
 * Both reuse the identical daily încasări/plăți/sold rendering.
 *
 * 5314 (cod 14-4-7/aA) — foreign-amount + exchange-rate columns:
 *   Each 5314 entry now carries amountFxForeign + currencyCode from gl_entry.
 *   The register shows: Document | Explanation | Contra | Foreign Amt + Currency |
 *   Curs (lei ÷ foreign) | Încasări (lei) | Plăți (lei) | Sold (lei).
 *   Entries without FX data (old/RON-only rows) fall back to lei-only display.
 *
 * Print / Save as PDF reuses the XmlViewerModal pattern:
 *   - wraps the printable `.docv` element with `buildStandaloneHtml`
 *   - demo mode → `window.open`; Tauri → `api.declarations.openDocInBrowser`
 */

import { Fragment, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";

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

/** Cash account is a debit-balance account: balance = debit − credit (≥ 0 in normal operation). */
const cashBalance = (debit: string | number, credit: string | number) =>
  parseDec(debit) - parseDec(credit);

// ─── FX helpers ──────────────────────────────────────────────────────────────

/** Format a decimal string to 4 decimal places (exchange-rate display). */
const fmtRate = (n: number) => n.toFixed(4).replace(".", ",");

/** Format a foreign-currency amount (e.g. "100.00 EUR"). */
const fmtFx = (amt: string, ccy: string) => `${amt.replace(".", ",")} ${ccy}`;

/**
 * Implied exchange rate for a 5314 line: lei / foreign.
 * Returns null when the foreign amount is zero (division guard).
 */
function impliedRate(leiAmt: string, fxAmt: string): number | null {
  const lei = parseDec(leiAmt);
  const fx = parseDec(fxAmt);
  if (fx === 0) return null;
  return lei / fx;
}

// ─── RegisterView ─────────────────────────────────────────────────────────────
// Renders the printable daily-register table for one account sheet.

interface RegisterViewProps {
  account: LedgerAccount;
  periodLabel: string;
  accountLabel: string;
  /** When true, renders the extra foreign-amount + curs columns (5314 cod 14-4-7/aA). */
  showFxColumns: boolean;
  printRef: React.RefObject<HTMLDivElement | null>;
  t: (key: string, opts?: Record<string, unknown>) => string;
}

function RegisterView({ account, periodLabel, accountLabel, showFxColumns, printRef, t }: RegisterViewProps) {
  const days = groupByDay(account.entries);
  const openingBal = cashBalance(account.openingDebit, account.openingCredit);
  const closingBal = cashBalance(account.closingDebit, account.closingCredit);

  // 5314 has 2 extra columns (Suma valută + Curs) before Încasări/Plăți/Sold.
  // Standard 5311 register has 7 columns; 5314 register has 9.
  const nBaseCol = showFxColumns ? 6 : 4; // colSpan for left cells in summary rows
  const totalCols = showFxColumns ? 9 : 7;

  return (
    /* Printable registru — wrapped in .docv so buildStandaloneHtml picks up the right CSS */
    <div className="docv" ref={printRef}>
      {/* Document header */}
      <div className="docv-title" style={{ marginBottom: 2 }}>{t("bank.title")}</div>
      <div
        className="docv-title docv-title-sub"
        style={{ fontSize: 13, fontWeight: 500, textAlign: "center", marginBottom: 16 }}
      >
        {accountLabel} · {periodLabel}
      </div>

      {/* Opening balance row */}
      <table className="scr-table" style={{ marginBottom: 0 }}>
        <thead>
          <tr>
            <th style={{ width: 110 }}>{t("bank.colData")}</th>
            <th style={{ width: 130 }}>{t("bank.colDocument")}</th>
            <th>{t("bank.colExplicatie")}</th>
            <th style={{ width: 130 }}>{t("bank.colContrapartida")}</th>
            {showFxColumns && (
              <>
                <th className="r" style={{ width: 130 }}>{t("bank.colSumaValuta")}</th>
                <th className="r" style={{ width: 100 }}>{t("bank.colCurs")}</th>
              </>
            )}
            <th className="r" style={{ width: 130 }}>{t("bank.colIncasari")}</th>
            <th className="r" style={{ width: 130 }}>{t("bank.colPlati")}</th>
            <th className="r" style={{ width: 120 }}>{t("bank.colSold")}</th>
          </tr>
        </thead>
        <tbody>
          {/* Sold inițial */}
          <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
            <td colSpan={totalCols - 1}>{t("bank.soldInitial")}</td>
            <td className="r num">{fmtRON(openingBal)}</td>
          </tr>

          {days.length === 0 ? (
            <tr>
              <td colSpan={totalCols} style={{ padding: 0 }}>
                <div className="empty">
                  <div className="ei"><Ic name="wallet" /></div>
                  <b>Nicio mișcare de numerar ({showFxColumns ? "5314" : "5311"}) în perioada selectată.</b>
                  Înregistrați o încasare sau o plată în numerar.
                </div>
              </td>
            </tr>
          ) : (
            /* Day groups */
            days.map(({ day, rows }) => {
              const dayDebit  = rows.reduce((s, r) => s + parseDec(r.debit),  0);
              const dayCredit = rows.reduce((s, r) => s + parseDec(r.credit), 0);
              // The end-of-day sold is the running balance of the last entry in the day.
              const eodBalance = parseDec(rows[rows.length - 1].balance);

              return rows.map((entry, idx) => {
                // FX columns for 5314: use entry.amountFxForeign when present.
                const hasFx = showFxColumns && entry.amountFxForeign != null && entry.currencyCode != null;
                // The lei amount for this line is debit or credit (whichever is non-zero).
                const leiAmt = parseDec(entry.debit) > 0 ? entry.debit : entry.credit;
                const rate = hasFx ? impliedRate(leiAmt, entry.amountFxForeign!) : null;

                return (
                  <Fragment key={`${day}-${idx}`}>
                    <tr>
                      <td className="num">{fmtD(day)}</td>
                      <td><span className="doc">{entry.document || "—"}</span></td>
                      <td>{entry.explanation || "—"}</td>
                      <td>{entry.contra ? <span className="doc">{entry.contra}</span> : <span className="muted">—</span>}</td>
                      {showFxColumns && (
                        <>
                          <td className="r num">
                            {hasFx
                              ? <span>{fmtFx(entry.amountFxForeign!, entry.currencyCode!)}</span>
                              : <span className="muted">—</span>}
                          </td>
                          <td className="r num">
                            {rate != null
                              ? <span>{fmtRate(rate)}</span>
                              : <span className="muted">—</span>}
                          </td>
                        </>
                      )}
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
                        <td colSpan={nBaseCol} style={{ paddingLeft: 12 }}>{t("bank.totalZi")} — {fmtD(day)}</td>
                        <td className="r num">{dayDebit > 0 ? fmtRON(dayDebit) : <span className="muted">—</span>}</td>
                        <td className="r num">{dayCredit > 0 ? fmtRON(dayCredit) : <span className="muted">—</span>}</td>
                        <td className="r num">{fmtRON(eodBalance)}</td>
                      </tr>
                    )}
                  </Fragment>
                );
              });
            })
          )}

          {/* Sold final */}
          <tr style={{ background: "var(--bg-table-header)", fontWeight: 700 }}>
            <td colSpan={nBaseCol}>{t("bank.soldFinal")}</td>
            <td className="r num">{fmtRON(account.totalDebit)}</td>
            <td className="r num">{fmtRON(account.totalCredit)}</td>
            <td className="r num">{fmtRON(closingBal)}</td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

// ─── Component ───────────────────────────────────────────────────────────────

export function BankPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const { data: companies = [] } = useQuery({
    queryKey: ["companies", "list"],
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);
  const companyName = activeCompany?.legalName ?? "";

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
  // null = absent/loaded-but-empty; undefined = not yet loaded (loading sentinel)
  const [account5311,   setAccount5311]   = useState<LedgerAccount | null | undefined>(undefined);
  const [account5314,   setAccount5314]   = useState<LedgerAccount | null | undefined>(undefined);
  // Toggle between the two registers
  const [activeTab,     setActiveTab]     = useState<"5311" | "5314">("5311");

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
    setAccount5311(undefined); // loading sentinel
    setAccount5314(undefined);
    void load(activeCompanyId, dateFrom, dateTo);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCompanyId, dateFrom]);

  async function load(companyId: string, from: string, to: string) {
    setLoading(true);
    try {
      const sheets = await api.gl.generalLedger(companyId, from, to);
      setAccount5311(sheets.find((s) => s.accountCode === "5311") ?? null);
      // 5314 may be absent if no foreign-currency cash was posted in the period.
      setAccount5314(sheets.find((s) => s.accountCode === "5314") ?? null);
    } catch (err) {
      notify.error(formatError(err, t("bank.loadError")));
      setAccount5311(null);
      setAccount5314(null);
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
    const acctSuffix = activeTab === "5314" ? "5314" : "5311";
    const fileName = `registru-casa-${acctSuffix}-${selectedYear}-${String(selectedMonth).padStart(2, "0")}.html`;
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
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("bank.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("bank.noCompany")}
        </div>
      </div>
    );
  }

  // ── Active account for the current tab ────────────────────────────────────
  const activeAccount = activeTab === "5314" ? account5314 : account5311;
  const activeAccountLabel = activeTab === "5314" ? t("bank.account5314") : t("bank.account5311");

  // Print button disabled when: loading, or the current tab has no data.
  const printDisabled = !activeAccount || loading;

  // ── Summary row values ────────────────────────────────────────────────────
  const openingBal = activeAccount
    ? cashBalance(activeAccount.openingDebit, activeAccount.openingCredit)
    : 0;
  const closingBal = activeAccount
    ? cashBalance(activeAccount.closingDebit, activeAccount.closingCredit)
    : 0;
  const totalIncasari = activeAccount ? parseDec(activeAccount.totalDebit) : 0;
  const totalPlati    = activeAccount ? parseDec(activeAccount.totalCredit) : 0;

  // ── Render ────────────────────────────────────────────────────────────────
  return (
    <div className="main-inner wide">

      {/* Page head */}
      <div className="page-head">
        <div>
          <h1>{t("bank.title")}</h1>
          <p className="sub">
            {t("bank.sub")}{companyName ? ` · ${companyName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          {/* Account toggle tabs: Lei (5311) / Valuta (5314) */}
          <div className="tabs">
            <div
              className={"tab" + (activeTab === "5311" ? " active" : "")}
              onClick={() => setActiveTab("5311")}
            >
              {t("bank.toggle5311")}
            </div>
            <div
              className={"tab" + (activeTab === "5314" ? " active" : "")}
              onClick={() => setActiveTab("5314")}
            >
              {t("bank.toggle5314")}
            </div>
          </div>

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
            disabled={printDisabled}
            onClick={() => void handlePrint()}
          >
            <Ic name="dl" />
            {t("bank.print")}
          </button>
        </div>
      </div>

      {/* Summary row */}
      <div className="sum-row">
        <div className="sum">
          <div className="l">{t("bank.soldInitial")}</div>
          <div className="v num">{fmtRON(openingBal)}</div>
        </div>
        <div className="sum">
          <div className="l">{t("bank.colIncasari")}</div>
          <div className="v num">{fmtRON(totalIncasari)}</div>
        </div>
        <div className="sum">
          <div className="l">{t("bank.colPlati")}</div>
          <div className="v num">{fmtRON(totalPlati)}</div>
        </div>
        <div className="sum">
          <div className="l">{t("bank.soldFinal")}</div>
          <div className="v num">{fmtRON(closingBal)}</div>
        </div>
      </div>

      {/* Content card */}
      <div className="scr-card">
        {loading ? (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 110 }}>{t("bank.colData")}</th>
                <th style={{ width: 130 }}>{t("bank.colDocument")}</th>
                <th>{t("bank.colExplicatie")}</th>
                <th style={{ width: 130 }}>{t("bank.colContrapartida")}</th>
                <th className="r" style={{ width: 130 }}>{t("bank.colIncasari")}</th>
                <th className="r" style={{ width: 130 }}>{t("bank.colPlati")}</th>
                <th className="r" style={{ width: 120 }}>{t("bank.colSold")}</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td colSpan={7} style={{ padding: "24px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                  {t("gl.common.loading")}
                </td>
              </tr>
            </tbody>
          </table>
        ) : activeAccount ? (
          <RegisterView
            account={activeAccount}
            periodLabel={periodLabel}
            accountLabel={activeAccountLabel}
            showFxColumns={activeTab === "5314"}
            printRef={printRef}
            t={t}
          />
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 110 }}>{t("bank.colData")}</th>
                <th style={{ width: 130 }}>{t("bank.colDocument")}</th>
                <th>{t("bank.colExplicatie")}</th>
                <th style={{ width: 130 }}>{t("bank.colContrapartida")}</th>
                <th className="r" style={{ width: 130 }}>{t("bank.colIncasari")}</th>
                <th className="r" style={{ width: 130 }}>{t("bank.colPlati")}</th>
                <th className="r" style={{ width: 120 }}>{t("bank.colSold")}</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td colSpan={7} style={{ padding: 0 }}>
                  <div className="empty">
                    <div className="ei"><Ic name="wallet" /></div>
                    <b>
                      {activeTab === "5314"
                        ? "Nicio mișcare de numerar în valută (5314) în perioada selectată."
                        : "Nicio mișcare de numerar (5311) în perioada selectată."}
                    </b>
                    Înregistrați o încasare sau o plată în numerar.
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        )}
      </div>

    </div>
  );
}
