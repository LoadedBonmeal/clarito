/**
 * Dashboard — Privire generală, date reale din backend.
 * Wave 5 — rf look: PageHeader + Segmented + Banner + StatCard + SectionCard + rf-tbl
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";

import { Banner, Btn, Empty } from "@/components/rf";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { Ic } from "@/components/shared/Ic";
import { queryClient, queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";

type PeriodMode = "today" | "week" | "month" | "ytd";

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString("ro-RO", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function DashboardPage() {
  const navigate  = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [periodMode, setPeriodMode] = useState<PeriodMode>("month");
  const [refreshing, setRefreshing] = useState(false);
  const [monthPopOpen, setMonthPopOpen] = useState(false);

  useEffect(() => {
    if (!monthPopOpen) return;
    const h = () => setMonthPopOpen(false);
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [monthPopOpen]);

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
    const pad = (n: number) => String(n).padStart(2, "0");
    const fmt = (d: Date) =>
      `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    const todayStr = fmt(now);
    if (periodMode === "today") return [todayStr, todayStr];
    if (periodMode === "week") {
      const day     = now.getDay();
      const diffToMon = day === 0 ? -6 : 1 - day;
      const mon = new Date(now);
      mon.setDate(now.getDate() + diffToMon);
      const sun = new Date(mon);
      sun.setDate(mon.getDate() + 6);
      return [fmt(mon), fmt(sun)];
    }
    if (periodMode === "ytd") return [`${now.getFullYear()}-01-01`, todayStr];
    const firstDay = new Date(now.getFullYear(), now.getMonth(), 1);
    const lastDay  = new Date(now.getFullYear(), now.getMonth() + 1, 0);
    return [fmt(firstDay), fmt(lastDay)];
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [periodMode, currentMonth]);

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
      months.push({ key: `${d.getFullYear()}-${pad(d.getMonth() + 1)}`, label: d.toLocaleDateString("ro-RO", { month: "short" }) });
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

  // ── "Total facturat" + delta vs previous month (shown only in month mode) ──
  const totalFacturat = totalNet + totalVat;
  const { deltaPct, deltaDir, prevMonthLabel } = useMemo(() => {
    const monthTotal = (key: string) =>
      invoices
        .filter((inv) => inv.issueDate.slice(0, 7) === key)
        .reduce((s, inv) => s + parseDec(inv.subtotalAmount) + parseDec(inv.vatAmount), 0);
    const prevD = new Date(now.getFullYear(), now.getMonth() - 1, 1);
    const pad = (n: number) => String(n).padStart(2, "0");
    const prevKey = `${prevD.getFullYear()}-${pad(prevD.getMonth() + 1)}`;
    const curKey = `${now.getFullYear()}-${pad(now.getMonth() + 1)}`;
    const prevTotal = monthTotal(prevKey);
    const label = prevD.toLocaleDateString("ro-RO", { month: "long" });
    if (prevTotal <= 0) return { deltaPct: null as number | null, deltaDir: "neutral" as "up" | "down" | "neutral", prevMonthLabel: label };
    const pct = Math.round(((monthTotal(curKey) - prevTotal) / prevTotal) * 100);
    return { deltaPct: pct, deltaDir: (pct >= 0 ? "up" : "down") as "up" | "down" | "neutral", prevMonthLabel: label };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [invoices, currentMonth]);

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
    if (t.includes("VALID") || t.includes("ACCEPT")) return "checkC";
    if (t.includes("SYNC")) return "sync";
    if (t.includes("IMPORT") || t.includes("RECEIV")) return "docDown";
    return "mail";
  };

  const monthName = now.toLocaleDateString("ro-RO", { month: "long" });
  const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const periodLabel =
    periodMode === "today" ? "Astăzi"
    : periodMode === "week" ? "Săptămâna aceasta"
    : periodMode === "ytd" ? `Anul ${now.getFullYear()}`
    : `${cap(monthName)} ${now.getFullYear()}`;
  const headDate = cap(now.toLocaleDateString("ro-RO", { weekday: "long", day: "numeric", month: "long", year: "numeric" }));

  // De încasat = issued, not-yet-finalized invoices in the period (proxy for outstanding).
  const openInvoices = periodInvoices.filter((i) => ["VALIDATED", "SUBMITTED", "QUEUED"].includes(i.status));
  const deIncasat = openInvoices.reduce((s, i) => s + parseDec(i.totalAmount), 0);

  const STATUS_CHIP: Record<string, { cls: string; icon: string; label: string }> = {
    DRAFT:     { cls: "sent", icon: "docText", label: "Ciornă" },
    QUEUED:    { cls: "wait", icon: "clock", label: "În coadă" },
    SUBMITTED: { cls: "wait", icon: "send", label: "Trimisă" },
    VALIDATED: { cls: "paid", icon: "checkC", label: "Validată" },
    REJECTED:  { cls: "late", icon: "xMark", label: "Respinsă" },
    STORNED:   { cls: "sent", icon: "undo", label: "Stornată" },
  };

  const PERIODS: { v: PeriodMode; label: string }[] = [
    { v: "today", label: "Astăzi" },
    { v: "week", label: "Săptămâna aceasta" },
    { v: "month", label: cap(monthName) + " " + now.getFullYear() },
    { v: "ytd", label: `Anul ${now.getFullYear()}` },
  ];

  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>Privire generală</h1></div></div>
        <Empty icon="buildings" title="Selectați o companie activă pentru a vedea datele din tabloul de bord.">
          Alegeți o companie din setări pentru a continua.
        </Empty>
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
        <div className="rf-col" style={{ marginBottom: 20 }}>
          {invoicesError && (
            <QueryErrorBanner error={invoicesErr} label="facturile" onRetry={() => void refetchInvoices()} />
          )}
          {lastRejected && (
            <Banner variant="error" title="Factură respinsă de ANAF"
              actions={<Btn variant="danger" size="sm" onClick={() => void navigate({ to: "/invoices/$id", params: { id: lastRejected.id } })}>Vezi factura</Btn>}>
              Factura <b className="rf-mono">{lastRejected.fullNumber}</b> a fost respinsă de ANAF
              {lastRejected.rejectionReason && (<>: <i>{lastRejected.rejectionReason}</i></>)}.
            </Banner>
          )}
          {regimeStatus && (regimeStatus.level === "exceeded" || regimeStatus.level === "approaching") && (
            <Banner variant={regimeStatus.level === "exceeded" ? "error" : "warning"}
              title={regimeStatus.level === "exceeded" ? "Plafon microîntreprindere depășit" : "Vă apropiați de plafonul de microîntreprindere"}>
              Cifra de afaceri {currentYear}: <b className="rf-mono">{regimeStatus.ytdTurnoverRon}</b> lei ({regimeStatus.pct}% din plafonul ≈ 100.000 EUR la cursul {regimeStatus.eurRate}).
            </Banner>
          )}
          {vatReg && vatReg.applicable && (vatReg.level === "exceeded" || vatReg.level === "approaching") && (
            <Banner variant={vatReg.level === "exceeded" ? "error" : "warning"}
              title={vatReg.level === "exceeded" ? "Plafon de scutire TVA depășit — înregistrarea în scopuri de TVA e obligatorie" : "Vă apropiați de plafonul de scutire TVA"}>
              Cifra de afaceri {currentYear}: <b className="rf-mono">{vatReg.ytdTurnoverRon}</b> lei ({vatReg.pct}% din {vatReg.plafonRon} lei, art. 310).
            </Banner>
          )}
          {regimeStatus && (regimeStatus.cashVatLevel === "exceeded" || regimeStatus.cashVatLevel === "approaching") && (
            <Banner variant={regimeStatus.cashVatLevel === "exceeded" ? "error" : "warning"}
              title={regimeStatus.cashVatLevel === "exceeded" ? "Plafon TVA la încasare depășit" : "Vă apropiați de plafonul TVA la încasare"}>
              Cifra de afaceri {currentYear}: <b className="rf-mono">{regimeStatus.ytdTurnoverRon}</b> lei (plafon {regimeStatus.cashVatPlafonRon} lei).
            </Banner>
          )}
          {intrastat && ([
            { label: "expedieri", f: intrastat.dispatches },
            { label: "introduceri", f: intrastat.arrivals },
          ] as const)
            .filter(({ f }) => f.level === "exceeded" || f.level === "approaching")
            .map(({ label, f }) => (
              <Banner key={label} variant={f.level === "exceeded" ? "error" : "warning"}
                title={f.level === "exceeded" ? `Prag Intrastat depășit (${label}) — declarație lunară obligatorie` : `Vă apropiați de pragul Intrastat (${label})`}>
                Valoare {label} {currentYear}: <b className="rf-mono">{f.ytdRon}</b> lei ({f.pct}% din {intrastat.thresholdRon} lei).
              </Banner>
            ))}
        </div>
      )}

      <div className="page-head">
        <div>
          <h1>Privire generală</h1>
          <p className="sub">{headDate} · {activeCompany?.legalName ?? ""}</p>
        </div>
        <div className="head-actions">
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button className="pill-btn" onMouseDown={(e) => e.stopPropagation()} onClick={() => setMonthPopOpen((o) => !o)}>
              <Ic name="calendar" /><span>{periodLabel}</span><Ic name="chevD" cls="ic" />
            </button>
            {monthPopOpen && (
              <div className="pop show" style={{ left: 0, top: 42, width: 210 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">Perioadă</div>
                {PERIODS.map((p) => (
                  <button key={p.v} className="pop-item" onClick={() => { setPeriodMode(p.v); setMonthPopOpen(false); }}>
                    <span style={{ flex: 1 }}>{p.label}</span>
                    {periodMode === p.v && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
          <button
            className={`sq-btn spin-btn${refreshing ? " spinning" : ""}`}
            aria-label="Reîmprospătează"
            disabled={refreshing}
            onClick={async () => { setRefreshing(true); try { await queryClient.refetchQueries({ type: "active" }); notify.success("Date actualizate"); } finally { setRefreshing(false); } }}
          >
            <Ic name="sync" />
          </button>
          <button className="btn-dark" onClick={() => void navigate({ to: "/invoices/new" })}>
            <Ic name="plus" />Factură nouă
          </button>
        </div>
      </div>

      {/* KPIs */}
      <div className="kpis">
        <div className="kpi">
          <div className="top"><span className="klabel">Total facturat</span><Ic name="docUp" /></div>
          <div className="val num">{fmtInt(totalFacturat)} <span className="cur">RON</span></div>
          <div className="delta">
            {periodMode === "month" && deltaPct != null
              ? (<><span className="ar">{deltaDir === "up" ? "↑" : "↓"} {Math.abs(deltaPct)}%</span> față de {prevMonthLabel}</>)
              : `${periodInvoices.length} facturi · ${fmtInt(totalNet)} net`}
          </div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">De încasat</span><Ic name="incasat" /></div>
          <div className="val num">{fmtInt(deIncasat)} <span className="cur">RON</span></div>
          <div className="delta">{openInvoices.length} facturi deschise</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">Facturi primite</span><Ic name="docDown" /></div>
          <div className="val num">{receivedTotal} <span className="cur">documente</span></div>
          <div className="delta">sincronizate din SPV</div>
        </div>
        <div className="kpi">
          <div className="top"><span className="klabel">TVA de colectat</span><Ic name="calc" /></div>
          <div className="val num">{fmtInt(totalVat)} <span className="cur">RON</span></div>
          <div className="delta">din {periodInvoices.length} facturi</div>
        </div>
      </div>

      {/* mid: chart + activity */}
      <div className="mid">
        <div className="card">
          <div className="card-head">
            <div>
              <div className="ct">Facturare lunară</div>
              <div className="cs">Emise vs. primite · {curChart.label}: <b>{curChart.emise}</b> emise · <b>{curChart.primite}</b> primite</div>
            </div>
            <div className="legend">
              <span className="lg"><span className="sw" style={{ background: "var(--black)" }} />Emise</span>
              <span className="lg"><span className="sw" style={{ background: "#D4D4D8" }} />Primite</span>
            </div>
          </div>
          <div className="chart">
            {chartData.map((d) => {
              const h = 170;
              const eH = Math.max(4, Math.round((d.emise / chartMax) * h));
              const pH = Math.max(4, Math.round((d.primite / chartMax) * h));
              return (
                <div key={d.key} className={`bar-col${d.key === curChart.key ? " curr" : ""}`}>
                  <div className="bar-tip"><b>{d.label}</b><span><i className="d" />Emise<em className="num">{d.emise}</em></span><span><i className="l" />Primite<em className="num">{d.primite}</em></span></div>
                  <div className="bar-stack">
                    <div className="bar b-emise" style={{ height: eH }} />
                    <div className="bar b-primite" style={{ height: pH }} />
                  </div>
                  <div className="mlab">{d.label}</div>
                </div>
              );
            })}
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <div className="ct">Activitate SPV</div>
            <a className="cs" style={{ textDecoration: "none", cursor: "pointer" }} onClick={() => void navigate({ to: "/notifications" })}>Vezi tot</a>
          </div>
          <div className="activity">
            {timelineItems.length === 0 ? (
              <div style={{ padding: "22px 4px", fontSize: 12.5, color: "var(--text-2)", textAlign: "center" }}>Fără notificări recente</div>
            ) : timelineItems.map((n) => (
              <div key={n.id} className="act-item">
                <div className="act-ic"><Ic name={actIcon(n.notificationType)} /></div>
                <div className="act-tx">
                  <div className="a1">{n.title}</div>
                  <div className="a2">{fmtTime(n.createdAt)}{n.body ? ` · ${n.body}` : ""}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* recent table */}
      <div className="table-wrap">
        <div className="tbar">
          <div className="tt">Facturi recente</div>
          <div className="tbar-actions">
            <a className="see-all" style={{ cursor: "pointer" }} onClick={() => void navigate({ to: "/invoices" })}>Vezi toate<Ic name="chevR" /></a>
          </div>
        </div>
        <table>
          <thead>
            <tr>
              <th className="c-client">Client</th>
              <th className="c-doc">Document</th>
              <th className="c-data">Data</th>
              <th className="c-scad">Scadența</th>
              <th className="c-val r">Valoare</th>
              <th className="c-status">Status ANAF</th>
            </tr>
          </thead>
          <tbody>
            {recentInvoices.length === 0 ? (
              <tr><td colSpan={6} style={{ textAlign: "center", padding: 24, color: "var(--text-2)" }}>
                Fără facturi. <button type="button" className="rf-link" onClick={() => void navigate({ to: "/invoices/new" })}>Creează prima factură →</button>
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

      <div style={{ fontSize: 11.5, color: "var(--dim)", padding: "4px 0 8px" }}>
        Datele se actualizează automat la fiecare 60 s. Toate sumele sunt în <b>RON</b>.
      </div>
    </div>
  );
}
