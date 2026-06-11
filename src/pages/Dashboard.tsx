/**
 * Dashboard — Privire generală, date reale din backend.
 * Wave 5 — rf look: PageHeader + Segmented + Banner + StatCard + SectionCard + rf-tbl
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Banner } from "@/components/shared/Banner";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { Ic } from "@/components/shared/Ic";
import { queryClient, queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";

/** Count-up once on mount — 500ms ease-out, ro-RO formatted (design stat-card rule). */
function CountUp({ value }: { value: number }) {
  const [shown, setShown] = useState(0);
  useEffect(() => {
    if (typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      setShown(value);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (t: number) => {
      const p = Math.min(1, (t - t0) / 500);
      setShown(value * (1 - Math.pow(1 - p, 3)));
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return <>{Math.round(shown).toLocaleString("ro-RO")}</>;
}

function fmtTime(unix: number, locale: string): string {
  return new Date(unix * 1000).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function DashboardPage() {
  const { t, i18n } = useTranslation();
  const navigate  = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [selDate, setSelDate] = useState<Date>(() => new Date());
  const [viewYM, setViewYM] = useState<{ y: number; m: number }>(() => { const d = new Date(); return { y: d.getFullYear(), m: d.getMonth() }; });
  const [refreshing, setRefreshing] = useState(false);
  const [monthPopOpen, setMonthPopOpen] = useState(false);
  const [colPopOpen, setColPopOpen] = useState(false);
  const [hiddenCols, setHiddenCols] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!monthPopOpen && !colPopOpen) return;
    const h = () => { setMonthPopOpen(false); setColPopOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [monthPopOpen, colPopOpen]);

  const selectedYM = `${selDate.getFullYear()}-${String(selDate.getMonth() + 1).padStart(2, "0")}`;

  // ── Queries ────────────────────────────────────────────────────────────────

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn:  () => api.companies.list(),
  });

  const invoiceFilter = useMemo(
    () => ({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    [activeCompanyId],
  );
  const {
    data:   invoicesPage,
    isError: invoicesError,
    error:  invoicesErr,
    refetch: refetchInvoices,
  } = useQuery({
    queryKey: queryKeys.invoices.list(invoiceFilter),
    queryFn:  () => api.invoices.list(invoiceFilter),
    enabled:  !!activeCompanyId,
  });

  const { data: notifications = [] } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn:  () => api.notifications.list(false),
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn:  () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled:  !!activeCompanyId,
  });

  // Received invoices — for the "Facturi primite" KPI + the emise-vs-primite chart.
  const receivedFilter = useMemo(
    () => ({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    [activeCompanyId],
  );
  const { data: receivedPage } = useQuery({
    queryKey: queryKeys.received.list(receivedFilter),
    queryFn:  () => api.received.list(receivedFilter),
    enabled:  !!activeCompanyId,
  });
  const receivedItems = receivedPage?.items ?? [];
  const receivedTotal = receivedPage?.total ?? 0;

  // Micro-enterprise ceiling monitor (100.000 EUR, OUG 89/2025). Conversia plafonului se face la
  // cursul BNR de la ÎNCHIDEREA EXERCIȚIULUI PRECEDENT (31.12 anul anterior) — NU la cursul zilei.
  // Pentru 2026 cursul oficial 31.12.2025 = 5.0985 RON/EUR (folosit și ca fallback offline).
  const currentYear = new Date().getFullYear();
  const OFFICIAL_EOY_EUR: Record<number, number> = { 2026: 5.0985 };
  const { data: regimeStatus } = useQuery({
    queryKey: ["taxRegimeStatus", activeCompanyId, currentYear],
    enabled:  !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: async () => {
      let eur = OFFICIAL_EOY_EUR[currentYear] ?? 5.0;
      try {
        eur = await api.bnr.fetchRate("EUR", `${currentYear - 1}-12-31`);
      } catch {
        /* offline — rămâne constanta oficială de închidere de an */
      }
      const status = await api.companies.taxRegimeStatus(activeCompanyId!, currentYear, eur);
      return { ...status, eurRate: eur };
    },
  });

  // Plafonul de scutire TVA (art. 310, Legea 141/2025): 395.000 lei CA anuală — relevant doar
  // pentru neplătitorii de TVA; depășirea obligă la înregistrarea în scopuri de TVA.
  const { data: vatReg } = useQuery({
    queryKey: ["vatRegistrationStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: () => api.companies.vatRegistrationStatus(activeCompanyId!, currentYear),
  });

  const { data: intrastat } = useQuery({
    queryKey: ["intrastatStatus", activeCompanyId, currentYear],
    enabled: !!activeCompanyId,
    staleTime: 5 * 60_000,
    queryFn: () =>
      api.declarations.intrastatStatus(activeCompanyId!, new Date().toISOString().slice(0, 10)),
  });

  // ── Derived data ───────────────────────────────────────────────────────────

  const contactMap = useMemo(
    () => Object.fromEntries(contacts.map((c) => [c.id, c])),
    [contacts],
  );

  const invoices    = invoicesPage?.items ?? [];

  const now = new Date();
  const currentMonth = now.toISOString().split("T")[0].slice(0, 7);

  const [periodFrom, periodTo] = useMemo((): [string, string] => {
    const [y, m] = selectedYM.split("-").map(Number);
    const last = new Date(y, m, 0).getDate();
    return [`${selectedYM}-01`, `${selectedYM}-${String(last).padStart(2, "0")}`];
  }, [selectedYM]);

  const periodInvoices = useMemo(
    () => invoices.filter((inv) => inv.issueDate >= periodFrom && inv.issueDate <= periodTo),
    [invoices, periodFrom, periodTo],
  );

  const totalNet = useMemo(
    () => periodInvoices.reduce((s, inv) => s + parseDec(inv.subtotalAmount), 0),
    [periodInvoices],
  );
  const totalVat = useMemo(
    () => periodInvoices.reduce((s, inv) => s + parseDec(inv.vatAmount), 0),
    [periodInvoices],
  );

  const lastRejected = useMemo(
    () => invoices.find((inv) => inv.status === "REJECTED"),
    [invoices],
  );

  const recentInvoices   = invoices.slice(0, 10);
  const timelineItems    = notifications.slice(0, 8);

  // ── Monthly emise-vs-primite chart (last 6 months, by document count) ───────
  const chartData = useMemo(() => {
    const pad = (n: number) => String(n).padStart(2, "0");
    const months: { key: string; label: string }[] = [];
    for (let i = 5; i >= 0; i--) {
      const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
      const CAP = ["Ian", "Feb", "Mar", "Apr", "Mai", "Iun", "Iul", "Aug", "Sep", "Oct", "Nov", "Dec"];
      months.push({ key: `${d.getFullYear()}-${pad(d.getMonth() + 1)}`, label: CAP[d.getMonth()] });
    }
    const emiseBy: Record<string, number> = {};
    const primiteBy: Record<string, number> = {};
    for (const inv of invoices) { const m = inv.issueDate.slice(0, 7); emiseBy[m] = (emiseBy[m] ?? 0) + 1; }
    for (const r of receivedItems) { const m = r.issueDate.slice(0, 7); primiteBy[m] = (primiteBy[m] ?? 0) + 1; }
    return months.map((mo) => ({ ...mo, emise: emiseBy[mo.key] ?? 0, primite: primiteBy[mo.key] ?? 0 }));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [invoices, receivedItems, currentMonth]);
  const chartMax = Math.max(1, ...chartData.flatMap((d) => [d.emise, d.primite]));
  const curChart = chartData[chartData.length - 1] ?? { key: "", label: "", emise: 0, primite: 0 };

  // ── "Total facturat" + delta vs the month before the selected one ──
  const totalFacturat = totalNet + totalVat;
  const { deltaPct, deltaDir, prevMonthLabel } = useMemo(() => {
    const monthTotal = (key: string) =>
      invoices
        .filter((inv) => inv.issueDate.slice(0, 7) === key)
        .reduce((s, inv) => s + parseDec(inv.subtotalAmount) + parseDec(inv.vatAmount), 0);
    const [y, m] = selectedYM.split("-").map(Number);
    const prevD = new Date(y, m - 2, 1);
    const pad = (n: number) => String(n).padStart(2, "0");
    const prevKey = `${prevD.getFullYear()}-${pad(prevD.getMonth() + 1)}`;
    const prevTotal = monthTotal(prevKey);
    const label = prevD.toLocaleDateString(i18n.language, { month: "long" });
    if (prevTotal <= 0) return { deltaPct: null as number | null, deltaDir: "neutral" as "up" | "down" | "neutral", prevMonthLabel: label };
    const pct = Math.round(((monthTotal(selectedYM) - prevTotal) / prevTotal) * 100);
    return { deltaPct: pct, deltaDir: (pct >= 0 ? "up" : "down") as "up" | "down" | "neutral", prevMonthLabel: label };
  }, [invoices, selectedYM, i18n.language]);

  const activeCompany    = companies.find((c) => c.id === activeCompanyId) ?? companies[0];

  // ── Render helpers ──────────────────────────────────────────────────────────
  const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
  const fmtInt = (n: number) => Math.round(n).toLocaleString("ro-RO");
  const fmtRoDate = (iso: string) => {
    if (!iso) return "—";
    const [y, m, d] = iso.split("-");
    return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
  };
  const ini = (s: string | undefined) =>
    (s ?? "—").replace(/[^A-Za-zĂÂÎȘȚ ]/g, "").split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase() || "—";
  const actIcon = (type: string) => {
    const t = type.toUpperCase();
    if (t.includes("REJECT")) return "xMark";
    if (t.includes("SENT") || t.includes("SUBMIT") || t.includes("VALID") || t.includes("ACCEPT")) return "send";
    if (t.includes("SYNC")) return "sync";
    if (t.includes("IMPORT") || t.includes("RECEIV")) return "docText";
    return "mail";
  };

  const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const MONTHS_FULL = [
    t("dashboard.months.jan"), t("dashboard.months.feb"), t("dashboard.months.mar"),
    t("dashboard.months.apr"), t("dashboard.months.may"), t("dashboard.months.jun"),
    t("dashboard.months.jul"), t("dashboard.months.aug"), t("dashboard.months.sep"),
    t("dashboard.months.oct"), t("dashboard.months.nov"), t("dashboard.months.dec"),
  ];
  const selM = selDate.getMonth() + 1;
  const periodLabel = `${MONTHS_FULL[selDate.getMonth()]} ${selDate.getFullYear()}`;
  const headDate = cap(selDate.toLocaleDateString(i18n.language, { weekday: "long", day: "numeric", month: "long", year: "numeric" }));
  const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
  const WD = [
    t("dashboard.weekdays.su"), t("dashboard.weekdays.mo"), t("dashboard.weekdays.tu"),
    t("dashboard.weekdays.we"), t("dashboard.weekdays.th"), t("dashboard.weekdays.fr"),
    t("dashboard.weekdays.sa"),
  ];
  const calStart = new Date(viewYM.y, viewYM.m, 1).getDay();

  // De încasat = issued, not-yet-finalized invoices in the period (proxy for outstanding).
  const openInvoices = periodInvoices.filter((i) => ["VALIDATED", "SUBMITTED", "QUEUED"].includes(i.status));
  const deIncasat = openInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);
  const receivedSum = receivedItems.reduce((s, r) => s + parseDec(r.totalAmount), 0);

  const STATUS_CHIP: Record<string, { cls: string; icon: string; label: string }> = {
    DRAFT:     { cls: "sent", icon: "docText", label: t("dashboard.status.draft") },
    QUEUED:    { cls: "wait", icon: "clock", label: t("dashboard.status.queued") },
    SUBMITTED: { cls: "wait", icon: "send", label: t("dashboard.status.submitted") },
    VALIDATED: { cls: "paid", icon: "check", label: t("dashboard.status.validated") },
    REJECTED:  { cls: "late", icon: "xMark", label: t("dashboard.status.rejected") },
    STORNED:   { cls: "sent", icon: "undo", label: t("dashboard.status.storned") },
  };

  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("dashboard.title")}</h1></div></div>
        <div style={{ padding: "48px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          <b style={{ display: "block", marginBottom: 4, color: "var(--text)" }}>{t("dashboard.noCompany.title")}</b>
          {t("dashboard.noCompany.hint")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner">
      {/* Real plafon / status monitors — shown only when triggered */}
      {(invoicesError || lastRejected ||
        (regimeStatus && (regimeStatus.level === "exceeded" || regimeStatus.level === "approaching")) ||
        (regimeStatus && (regimeStatus.cashVatLevel === "exceeded" || regimeStatus.cashVatLevel === "approaching")) ||
        (vatReg && vatReg.applicable && (vatReg.level === "exceeded" || vatReg.level === "approaching")) ||
        (intrastat && (intrastat.dispatches.level !== "ok" || intrastat.arrivals.level !== "ok"))) && (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 20 }}>
          {invoicesError && (
            <QueryErrorBanner error={invoicesErr} label={t("dashboard.errors.invoicesLabel")} onRetry={() => void refetchInvoices()} />
          )}
          {lastRejected && (
            <Banner variant="error" title={t("dashboard.banners.rejectedTitle")}
              actions={<button className="pill-btn" style={{ color: "var(--red)" }} onClick={() => void navigate({ to: "/invoices/$id", params: { id: lastRejected.id } })}>{t("dashboard.banners.viewInvoice")}</button>}>
              {t("dashboard.banners.rejectedPrefix")} <b className="num">{lastRejected.fullNumber}</b> {t("dashboard.banners.rejectedSuffix")}
              {lastRejected.rejectionReason && (<>: <i>{lastRejected.rejectionReason}</i></>)}.
            </Banner>
          )}
          {regimeStatus && (regimeStatus.level === "exceeded" || regimeStatus.level === "approaching") && (
            <Banner variant={regimeStatus.level === "exceeded" ? "error" : "warning"}
              title={regimeStatus.level === "exceeded" ? t("dashboard.banners.microExceeded") : t("dashboard.banners.microApproaching")}>
              {t("dashboard.banners.turnoverPrefix", { year: currentYear })} <b className="num">{regimeStatus.ytdTurnoverRon}</b> {t("dashboard.banners.microBody", { pct: regimeStatus.pct, rate: regimeStatus.eurRate })}
            </Banner>
          )}
          {vatReg && vatReg.applicable && (vatReg.level === "exceeded" || vatReg.level === "approaching") && (
            <Banner variant={vatReg.level === "exceeded" ? "error" : "warning"}
              title={vatReg.level === "exceeded" ? t("dashboard.banners.vatExemptExceeded") : t("dashboard.banners.vatExemptApproaching")}>
              {t("dashboard.banners.turnoverPrefix", { year: currentYear })} <b className="num">{vatReg.ytdTurnoverRon}</b> {t("dashboard.banners.vatExemptBody", { pct: vatReg.pct, plafon: vatReg.plafonRon })}
            </Banner>
          )}
          {regimeStatus && (regimeStatus.cashVatLevel === "exceeded" || regimeStatus.cashVatLevel === "approaching") && (
            <Banner variant={regimeStatus.cashVatLevel === "exceeded" ? "error" : "warning"}
              title={regimeStatus.cashVatLevel === "exceeded" ? t("dashboard.banners.cashVatExceeded") : t("dashboard.banners.cashVatApproaching")}>
              {t("dashboard.banners.turnoverPrefix", { year: currentYear })} <b className="num">{regimeStatus.ytdTurnoverRon}</b> {t("dashboard.banners.cashVatBody", { plafon: regimeStatus.cashVatPlafonRon })}
            </Banner>
          )}
          {intrastat && ([
            { label: t("dashboard.banners.intrastatDispatches"), f: intrastat.dispatches },
            { label: t("dashboard.banners.intrastatArrivals"), f: intrastat.arrivals },
          ] as const)
            .filter(({ f }) => f.level === "exceeded" || f.level === "approaching")
            .map(({ label, f }) => (
              <Banner key={label} variant={f.level === "exceeded" ? "error" : "warning"}
                title={f.level === "exceeded" ? t("dashboard.banners.intrastatExceeded", { label }) : t("dashboard.banners.intrastatApproaching", { label })}>
                {t("dashboard.banners.intrastatValuePrefix", { label, year: currentYear })} <b className="num">{f.ytdRon}</b> {t("dashboard.banners.intrastatBody", { pct: f.pct, threshold: intrastat.thresholdRon })}
              </Banner>
            ))}
        </div>
      )}

      <div className="page-head">
        <div>
          <h1>{t("dashboard.title")}</h1>
          <p className="sub">{headDate} · {activeCompany?.legalName ?? ""}</p>
        </div>
        <div className="head-actions">
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => { if (!monthPopOpen) setViewYM({ y: selDate.getFullYear(), m: selDate.getMonth() }); setMonthPopOpen((o) => !o); }}
            >
              <Ic name="calendar" /><span>{periodLabel}</span><Ic name="chevD" cls="ic" />
            </button>
            {monthPopOpen && (
              <div className="pop show" style={{ left: 0, top: 42, width: 288, padding: 10 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="cal-head">
                  <button className="cal-nav" aria-label={t("dashboard.actions.prevMonth")} onClick={() => setViewYM((v) => { const d = new Date(v.y, v.m - 1, 1); return { y: d.getFullYear(), m: d.getMonth() }; })}>
                    <svg viewBox="0 0 24 24"><path d="M15.75 19.5 8.25 12l7.5-7.5" /></svg>
                  </button>
                  <div className="cal-title">{MONTHS_FULL[viewYM.m]} {viewYM.y}</div>
                  <button className="cal-nav" aria-label={t("dashboard.actions.nextMonth")} onClick={() => setViewYM((v) => { const d = new Date(v.y, v.m + 1, 1); return { y: d.getFullYear(), m: d.getMonth() }; })}>
                    <svg viewBox="0 0 24 24"><path d="m8.25 4.5 7.5 7.5-7.5 7.5" /></svg>
                  </button>
                </div>
                <div className="cal-wd">{WD.map((d) => <span key={d}>{d}</span>)}</div>
                <div className="cal-grid">
                  {Array.from({ length: 42 }, (_, i) => {
                    const cell = new Date(viewYM.y, viewYM.m, i - calStart + 1);
                    const out = cell.getMonth() !== viewYM.m;
                    const cls = `cal-day${out ? " out" : ""}${sameDay(cell, now) ? " today" : ""}${sameDay(cell, selDate) ? " sel" : ""}`;
                    return (
                      <button key={i} className={cls} onClick={() => { setSelDate(cell); setViewYM({ y: cell.getFullYear(), m: cell.getMonth() }); setMonthPopOpen(false); }}>
                        {cell.getDate()}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
          <button
            className={`sq-btn spin-btn${refreshing ? " spinning" : ""}`}
            aria-label={t("dashboard.actions.refresh")}
            disabled={refreshing}
            onClick={async () => { setRefreshing(true); try { await queryClient.refetchQueries({ type: "active" }); notify.success(t("dashboard.notify.refreshed")); } finally { setRefreshing(false); } }}
          >
            <Ic name="sync" />
          </button>
          <button className="btn-dark" onClick={() => void navigate({ to: "/invoices/new" })}>
            <Ic name="plus" />{t("dashboard.actions.newInvoice")}
          </button>
        </div>
      </div>

      {/* KPIs */}
      <div className="kpis">
        <div className="kpi">
          <div className="top"><span className="klabel">{t("dashboard.kpi.totalInvoiced")}</span><Ic name="docUp" /></div>
          <div className="val num"><CountUp value={totalFacturat} /> <span className="cur">RON</span></div>
          <div className="delta">
            {deltaPct != null
              ? (<><span className="ar">{deltaDir === "up" ? "↑" : "↓"} {Math.abs(deltaPct)}%</span> {t("dashboard.kpi.vsPrev", { month: prevMonthLabel })}</>)
              : t("dashboard.kpi.invoicesNet", { count: periodInvoices.length, net: fmtInt(totalNet) })}
          </div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">{t("dashboard.kpi.receivable")}</span><Ic name="incasat" /></div>
          <div className="val num"><CountUp value={deIncasat} /> <span className="cur">RON</span></div>
          <div className="delta">{t("dashboard.kpi.openInvoices", { count: openInvoices.length })}</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">{t("dashboard.kpi.received")}</span><Ic name="docDown" /></div>
          <div className="val num"><CountUp value={receivedSum} /> <span className="cur">RON</span></div>
          <div className="delta">{t("dashboard.kpi.documents", { count: receivedTotal })}</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">{t("dashboard.kpi.vatToCollect")}</span><Ic name="calc" /></div>
          <div className="val num"><CountUp value={totalVat} /> <span className="cur">RON</span></div>
          <div className="delta">{t("dashboard.kpi.vatDue", { month: RO_MON[selM % 12] })}</div>
        </div>
      </div>

      {/* mid: chart + activity */}
      <div className="mid">
        <div className="card">
          <div className="card-head">
            <div>
              <div className="ct">{t("dashboard.chart.title")}</div>
              <div className="cs">{t("dashboard.chart.sub")} · {curChart.label}: <b>{curChart.emise}</b> {t("dashboard.chart.issuedLc")} · <b>{curChart.primite}</b> {t("dashboard.chart.receivedLc")}</div>
            </div>
            <div className="legend">
              <span className="lg"><span className="sw" style={{ background: "var(--black)" }} />{t("dashboard.chart.issued")}</span>
              <span className="lg"><span className="sw" style={{ background: "#D4D4D8" }} />{t("dashboard.chart.received")}</span>
            </div>
          </div>
          <div className="chart">
            {chartData.map((d) => {
              const h = 170;
              const eH = Math.max(4, Math.round((d.emise / chartMax) * h));
              const pH = Math.max(4, Math.round((d.primite / chartMax) * h));
              return (
                <div key={d.key} className={`bar-col${d.key === curChart.key ? " curr" : ""}`}>
                  <div className="bar-tip"><b>{d.label}</b><span><i className="d" />{t("dashboard.chart.issued")}<em className="num">{d.emise}</em></span><span><i className="l" />{t("dashboard.chart.received")}<em className="num">{d.primite}</em></span></div>
                  <div className="bar-stack">
                    <div className="bar b-emise anim-bar" style={{ height: eH, animationDelay: `${chartData.indexOf(d) * 40}ms` }} />
                    <div className="bar b-primite anim-bar" style={{ height: pH, animationDelay: `${chartData.indexOf(d) * 40 + 20}ms` }} />
                  </div>
                  <div className="mlab">{d.label}</div>
                </div>
              );
            })}
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <div className="ct">{t("dashboard.activity.title")}</div>
            <a className="cs" style={{ textDecoration: "none", cursor: "pointer" }} onClick={() => void navigate({ to: "/notifications" })}>{t("dashboard.activity.seeAll")}</a>
          </div>
          <div className="activity">
            {timelineItems.length === 0 ? (
              <div style={{ padding: "22px 4px", fontSize: 12.5, color: "var(--text-2)", textAlign: "center" }}>{t("dashboard.activity.empty")}</div>
            ) : timelineItems.map((n) => (
              <div key={n.id} className="act-item">
                <div className="act-ic"><Ic name={actIcon(n.notificationType)} /></div>
                <div className="act-tx">
                  <div className="a1">{n.title}</div>
                  <div className="a2">{n.body || fmtTime(n.createdAt, i18n.language)}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* recent table */}
      <div className="table-wrap">
        <div className="tbar">
          <div className="tt">{t("dashboard.table.title")}</div>
          <div className="tbar-actions">
            <a className="see-all" style={{ cursor: "pointer" }} onClick={() => void navigate({ to: "/invoices" })}>{t("dashboard.table.seeAll")}<Ic name="chevR" /></a>
            <button className="pill-btn" onMouseDown={(e) => e.stopPropagation()} onClick={() => setColPopOpen((o) => !o)}>
              <Ic name="columns" />{t("dashboard.table.columns")}
            </button>
            {colPopOpen && (
              <div className="pop show" style={{ right: 0, top: 40, width: 230 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("dashboard.table.manageColumns")}</div>
                <div className="col-row locked"><Ic name="users" /><span className="cl">{t("dashboard.table.client")}</span><span className="tog on" /></div>
                {([["doc", t("dashboard.table.document"), "docText"], ["data", t("dashboard.table.date"), "calendar"], ["scad", t("dashboard.table.due"), "clock"], ["val", t("dashboard.table.amount"), "calc"]] as const).map(([key, label, icon]) => (
                  <div key={key} className="col-row" onClick={() => setHiddenCols((s) => ({ ...s, [key]: !s[key] }))}>
                    <Ic name={icon} /><span className="cl">{label}</span><span className={`tog${hiddenCols[key] ? "" : " on"}`} />
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
        <table className={Object.entries(hiddenCols).filter(([, v]) => v).map(([k]) => `hide-${k}`).join(" ")}>
          <thead>
            <tr>
              <th className="c-client">{t("dashboard.table.client")}</th>
              <th className="c-doc">{t("dashboard.table.document")}</th>
              <th className="c-data">{t("dashboard.table.date")}</th>
              <th className="c-scad">{t("dashboard.table.due")}</th>
              <th className="c-val r">{t("dashboard.table.amount")}</th>
              <th className="c-status">{t("dashboard.table.statusAnaf")}</th>
            </tr>
          </thead>
          <tbody>
            {recentInvoices.length === 0 ? (
              <tr><td colSpan={6} style={{ textAlign: "center", padding: 24, color: "var(--text-2)" }}>
                {t("dashboard.table.empty")} <button type="button" className="link" onClick={() => void navigate({ to: "/invoices/new" })}>{t("dashboard.table.createFirst")}</button>
              </td></tr>
            ) : recentInvoices.map((inv) => {
              const chip = STATUS_CHIP[inv.status] ?? STATUS_CHIP.DRAFT;
              return (
                <tr key={inv.id} className="clickable" onClick={() => void navigate({ to: "/invoices/$id", params: { id: inv.id } })}>
                  <td className="c-client"><div className="cli"><span className="cli-ava">{ini(contactMap[inv.contactId]?.legalName)}</span>{contactMap[inv.contactId]?.legalName ?? "—"}</div></td>
                  <td className="c-doc"><span className="doc">{inv.fullNumber}</span></td>
                  <td className="c-data num">{fmtRoDate(inv.issueDate)}</td>
                  <td className="c-scad num">{fmtRoDate(inv.dueDate)}</td>
                  <td className="c-val r num">{fmtInt(parseDec(inv.totalAmount))} RON</td>
                  <td className="c-status"><span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{chip.label}</span></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

    </div>
  );
}
