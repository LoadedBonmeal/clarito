/**
 * Dashboard — Privire generală, date reale din backend.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryClient, queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtShortcut } from "@/lib/platform";
import { parseDec, fmtRON } from "@/lib/utils";
import { notify } from "@/lib/toasts";

const DOT_COLORS = [
  "#2848A1", "#7C3AED", "#0891B2", "#D97706", "#16A34A",
  "#0369A1", "#E11D48", "#65A30D", "#525252", "#B45309",
];
function dotColor(cui: string): string {
  let h = 0;
  for (let i = 0; i < cui.length; i++) h = (h * 31 + cui.charCodeAt(i)) >>> 0;
  return DOT_COLORS[h % DOT_COLORS.length];
}

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleTimeString("ro-RO", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function notifKind(type: string): string {
  const t = type.toUpperCase();
  if (t.includes("REJECT")) return "error";
  if (t.includes("VALID")) return "ok";
  if (t.includes("WARN") || t.includes("EXPIR")) return "warn";
  return "info";
}

export function DashboardPage() {
  const navigate = useNavigate();
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [periodMode, setPeriodMode] = useState<'today' | 'week' | 'month' | 'ytd'>('month');
  const [refreshing, setRefreshing] = useState(false);

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const invoiceFilter = useMemo(
    () => ({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 200 } }),
    [activeCompanyId],
  );
  const { data: invoicesPage, isError: invoicesError, error: invoicesErr, refetch: refetchInvoices } = useQuery({
    queryKey: queryKeys.invoices.list(invoiceFilter),
    queryFn: () => api.invoices.list(invoiceFilter),
  });

  const { data: notifications = [] } = useQuery({
    queryKey: queryKeys.notifications.list(false),
    queryFn: () => api.notifications.list(false),
  });

  const { data: unreadCount = 0 } = useQuery({
    queryKey: queryKeys.notifications.unreadCount(),
    queryFn: () => api.notifications.unreadCount(),
    refetchInterval: 60_000,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: isAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });
  const anafConnected = !activeCompanyId || !!isAnafAuth;

  const contactMap = useMemo(
    () => Object.fromEntries(contacts.map((c) => [c.id, c])),
    [contacts],
  );

  const invoices = invoicesPage?.items ?? [];
  const invoiceTotal = invoicesPage?.total ?? 0;

  const now = new Date();
  const currentMonth = now.toISOString().split("T")[0].slice(0, 7);

  // Compute [from, to] date range strings (YYYY-MM-DD) for the selected period
  const [periodFrom, periodTo] = useMemo((): [string, string] => {
    const pad = (n: number) => String(n).padStart(2, '0');
    const fmt = (d: Date) =>
      `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

    const todayStr = fmt(now);

    if (periodMode === 'today') {
      return [todayStr, todayStr];
    }
    if (periodMode === 'week') {
      const day = now.getDay(); // 0=Sun..6=Sat; Romanian week starts Monday
      const diffToMon = (day === 0 ? -6 : 1 - day);
      const mon = new Date(now);
      mon.setDate(now.getDate() + diffToMon);
      const sun = new Date(mon);
      sun.setDate(mon.getDate() + 6);
      return [fmt(mon), fmt(sun)];
    }
    if (periodMode === 'ytd') {
      return [`${now.getFullYear()}-01-01`, todayStr];
    }
    // 'month' — first to last day of current month
    const firstDay = new Date(now.getFullYear(), now.getMonth(), 1);
    const lastDay = new Date(now.getFullYear(), now.getMonth() + 1, 0);
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

  const validatedCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "VALIDATED").length,
    [periodInvoices],
  );
  const rejectedCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "REJECTED").length,
    [periodInvoices],
  );
  const draftCount = useMemo(
    () => periodInvoices.filter((inv) => inv.status === "DRAFT").length,
    [periodInvoices],
  );

  const overdue = useMemo(
    () =>
      invoices.filter((inv) => {
        if (!inv.dueDate) return false;
        const due = new Date(inv.dueDate);
        const todayMidnight = new Date();
        todayMidnight.setHours(0, 0, 0, 0);
        const isPastDue = due < todayMidnight;
        // Exclude drafts, storned and rejected invoices — ACCEPTED/SUBMITTED/VALIDATED
        // invoices with a past due date are considered potentially unpaid (overdue).
        const isActive = !["DRAFT", "STORNED", "REJECTED"].includes(inv.status);
        return isPastDue && isActive;
      }),
    [invoices],
  );

  const overdueTotal = useMemo(
    () => overdue.reduce((s, inv) => s + parseDec(inv.totalAmount), 0),
    [overdue],
  );

  const lastRejected = useMemo(
    () => invoices.find((inv) => inv.status === "REJECTED"),
    [invoices],
  );

  const recent = invoices.slice(0, 10);
  const timelineItems = notifications.slice(0, 8);

  const activeCompany = companies.find((c) => c.id === activeCompanyId) ?? companies[0];

  const monthLabel = now.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });

  const hour = now.getHours();
  const greeting = hour < 12 ? "Bună dimineața" : hour < 17 ? "Bună ziua" : "Bună seara";
  const todayStr = now.toLocaleDateString("ro-RO", {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">RoFactura</span>
          {t("dashboard.title")}
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Perioadă:</span>
          <div className="seg" role="tablist" aria-label="Perioadă">
            <button type="button" className={`seg-item ${periodMode === 'today' ? 'active' : ''}`} onClick={() => setPeriodMode('today')}>Astăzi</button>
            <button type="button" className={`seg-item ${periodMode === 'week' ? 'active' : ''}`} onClick={() => setPeriodMode('week')}>Săptămâna</button>
            <button type="button" className={`seg-item ${periodMode === 'month' ? 'active' : ''}`} onClick={() => setPeriodMode('month')}>{monthLabel}</button>
            <button type="button" className={`seg-item ${periodMode === 'ytd' ? 'active' : ''}`} onClick={() => setPeriodMode('ytd')}>YTD</button>
          </div>
          <button type="button" className="btn compact" onClick={() => notify.info('Export disponibil în R8')}>
            <Icon name="download" size={11} /> Export
          </button>
          <button
            type="button"
            className="btn"
            disabled={refreshing}
            onClick={async () => {
              setRefreshing(true);
              try {
                await queryClient.refetchQueries({ type: "active" });
                notify.success("Date actualizate");
              } finally {
                setRefreshing(false);
              }
            }}
          >
            <Icon name="refresh" size={12} /> {refreshing ? "Se actualizează…" : "Reîmprospătează"}{" "}
            {!refreshing && <span className="kbd" style={{ marginLeft: 4 }}>F5</span>}
          </button>
        </span>
      </div>

      <div className="content-body">
      {invoicesError && (
        <div style={{ padding: "0 14px" }}>
          <QueryErrorBanner error={invoicesErr} label="facturile" onRetry={() => void refetchInvoices()} />
        </div>
      )}
      {lastRejected && (
        <div
          className="callout callout-warn"
          style={{ margin: "10px 14px 0", display: "flex", gap: 12, alignItems: "center" }}
        >
          <Icon name="alert" size={15} />
          <p style={{ margin: 0, flex: 1 }}>
            Factura <strong>{lastRejected.fullNumber}</strong>
            {contactMap[lastRejected.contactId]?.legalName && (
              <> către <strong>{contactMap[lastRejected.contactId]?.legalName}</strong></>
            )}
            {' '}a fost respinsă de ANAF
            {lastRejected.rejectionReason && (
              <>: <em>{lastRejected.rejectionReason}</em></>
            )}.
          </p>
          <button
            type="button"
            className="fix"
            onClick={() =>
              navigate({ to: "/invoices/$id", params: { id: lastRejected.id } })
            }
          >
            <Icon name="edit" size={11} /> Corectează
          </button>
          <button
            type="button"
            className="btn compact"
            onClick={() =>
              navigate({ to: "/invoices/$id", params: { id: lastRejected.id } })
            }
          >
            <Icon name="eye" size={11} /> Vezi factura
          </button>
        </div>
      )}

      <div className="dash">
        <div className="dash-summary">
          <span className="b">
            {greeting}
            {activeCompany ? `, ${activeCompany.legalName}` : ""}.
          </span>{" "}
          Astăzi este {todayStr}.{" "}
          {unreadCount > 0 && (
            <>
              <span className="pill">
                <Icon name="bell" size={11} />
                {unreadCount} mesaje SPV neprocesate
              </span>{" "}
            </>
          )}
          {rejectedCount > 0 && (
            <>
              <span className="pill">
                <Icon name="alert" size={11} />
                {rejectedCount}{" "}
                {rejectedCount === 1 ? "factură respinsă" : "facturi respinse"} de ANAF
              </span>{" "}
            </>
          )}
          În perioada selectată ați emis{" "}
          <span className="b">
            {periodInvoices.length} {periodInvoices.length === 1 ? "factură" : "facturi"}
          </span>{" "}
          totalizând{" "}
          <span className="b tnum">{fmtRON(totalNet + totalVat)} RON</span>
          {periodInvoices.length > 0 && (
            <>
              , dintre care <span className="b">{validatedCount} validate</span> de ANAF
              {rejectedCount > 0 && (
                <>
                  {" "}și <span className="neg">{rejectedCount} respinse</span>
                </>
              )}
            </>
          )}
          .
        </div>

        <div className="kpi-strip">
          <div className="kpi-cell k-sales">
            <span className="lbl">Vânzări · {monthLabel}</span>
            <span className="val tnum">{fmtRON(totalNet)}</span>
            <span className="sub">RON net · {periodInvoices.length} facturi</span>
          </div>
          <div className="kpi-cell k-vat">
            <span className="lbl">TVA colectată</span>
            <span className="val tnum">{fmtRON(totalVat)}</span>
            <span className="sub">din {periodInvoices.length} facturi în perioada selectată</span>
          </div>
          <div className="kpi-cell k-invoices">
            <span className="lbl">Facturi emise · {monthLabel.split(" ")[0]}</span>
            <span className="val tnum">{periodInvoices.length}</span>
            <span className="sub">
              {validatedCount} validate · {rejectedCount} respinse · {draftCount} schițe
            </span>
          </div>
          <div className="kpi-cell k-overdue">
            <span className="lbl">De încasat · Restanțe</span>
            <span className="val tnum">{fmtRON(overdueTotal)}</span>
            <span className={overdue.length > 0 ? "delta down" : "sub"}>
              {overdue.length > 0
                ? `▼ ${overdue.length} facturi cu termen depășit`
                : "Fără restanțe"}
            </span>
          </div>
        </div>

        <div className="dash-row">
          <div className="panel">
            <div className="panel-header">
              <span>{t("nav.companies")} administrate · {companies.length} {companies.length === 1 ? "companie" : "companii"}</span>
              <span style={{ display: "flex", gap: 6 }}>
                <button
                  type="button"
                  className="btn compact"
                  onClick={() => navigate({ to: "/companies" })}
                >
                  Vezi toate <Icon name="arrowRight" size={11} />
                </button>
              </span>
            </div>
            <div style={{ maxHeight: 240, overflow: "auto" }}>
              <table className="dt">
                <thead>
                  <tr>
                    <th style={{ width: 96 }}>CUI</th>
                    <th>Denumire</th>
                    <th style={{ width: 110 }}>Localitate</th>
                    <th className="num" style={{ width: 56 }}>SPV</th>
                    <th style={{ width: 84 }}>Serie</th>
                    <th className="num" style={{ width: 80 }}>Ultima nr.</th>
                  </tr>
                </thead>
                <tbody>
                  {companies.slice(0, 8).map((c) => (
                    <tr
                      key={c.id}
                      className={c.id === activeCompanyId ? "selected" : ""}
                      style={{ cursor: "pointer" }}
                      onClick={() =>
                        navigate({ to: "/companies/$id", params: { id: c.id } })
                      }
                    >
                      <td>
                        <span className="mono">{c.cui}</span>
                      </td>
                      <td>
                        <span
                          style={{
                            display: "inline-block",
                            width: 6,
                            height: 6,
                            background: dotColor(c.cui),
                            marginRight: 6,
                            verticalAlign: "middle",
                          }}
                        />
                        {c.legalName}
                      </td>
                      <td className="muted">{c.city}</td>
                      <td className="num">
                        {c.spvEnabled ? (
                          <span style={{ color: "#16A34A", display: "inline-flex" }}>
                            <Icon name="check" size={13} />
                          </span>
                        ) : (
                          <span className="dim">
                            <Icon name="x" size={13} />
                          </span>
                        )}
                      </td>
                      <td className="mono">{c.invoiceSeries}</td>
                      <td className="num tnum">{c.lastInvoiceNumber}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          <div className="panel">
            <div className="panel-header">
              <span>Activitate ANAF · live</span>
              <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <span
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 4,
                    fontSize: 10.5,
                    color: anafConnected ? "#16A34A" : "#DC2626",
                    textTransform: "none",
                    letterSpacing: 0,
                  }}
                >
                  <span className={anafConnected ? "anaf-dot" : "anaf-dot err"} style={{ marginRight: 2 }} /> {anafConnected ? "conectat" : "neautentificat"}
                </span>
              </span>
            </div>
            <div className="panel-body" style={{ padding: "4px 12px 8px" }}>
              {timelineItems.length === 0 ? (
                <div style={{ padding: "16px 0", fontSize: 11, color: "var(--text-muted)", textAlign: "center" }}>
                  Fără notificări recente
                </div>
              ) : (
                <div className="timeline">
                  {timelineItems.map((n) => (
                    <div key={n.id} className={"timeline-row " + notifKind(n.notificationType)}>
                      <span className="dot" />
                      <span className="time">{fmtTime(n.createdAt)}</span>
                      <span className="what">
                        {n.title}
                        <span className="meta">{n.body}</span>
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>

        <div className="panel">
          <div className="panel-header">
            <span>{t("dashboard.invoices")} · ultimele 10</span>
            <span style={{ display: "flex", gap: 6 }}>
              <button
                type="button"
                className="btn compact"
                onClick={() => navigate({ to: "/invoices/new" })}
              >
                <Icon name="plus" size={12} /> Factură nouă{" "}
                <span className="kbd" style={{ marginLeft: 6 }}>{fmtShortcut("Ctrl N")}</span>
              </button>
              <button
                type="button"
                className="btn compact"
                onClick={() => navigate({ to: "/invoices" })}
              >
                Vezi toate ({invoiceTotal}) <Icon name="arrowRight" size={11} />
              </button>
            </span>
          </div>
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 130 }}>Nr. factură</th>
                <th style={{ width: 92 }}>Data</th>
                <th>Cumpărător</th>
                <th className="num" style={{ width: 110 }}>Net (RON)</th>
                <th className="num" style={{ width: 90 }}>TVA</th>
                <th className="num" style={{ width: 120 }}>Total</th>
                <th style={{ width: 120 }}>Status ANAF</th>
                <th style={{ width: 110 }}>Index ANAF</th>
                <th style={{ width: 24 }}></th>
              </tr>
            </thead>
            <tbody>
              {recent.length === 0 ? (
                <tr>
                  <td colSpan={9} style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 11 }}>
                    Fără facturi. <button type="button" className="link-btn" onClick={() => navigate({ to: "/invoices/new" })}>Creează prima factură →</button>
                  </td>
                </tr>
              ) : (
                recent.map((inv) => (
                  <tr
                    key={inv.id}
                    style={{ cursor: "pointer" }}
                    onClick={() => navigate({ to: "/invoices/$id", params: { id: inv.id } })}
                  >
                    <td className="mono"><b>{inv.fullNumber}</b></td>
                    <td className="muted">{inv.issueDate}</td>
                    <td>{contactMap[inv.contactId]?.legalName ?? <span className="dim">—</span>}</td>
                    <td className="num tnum">{fmtRON(inv.subtotalAmount)}</td>
                    <td className="num tnum muted">{fmtRON(inv.vatAmount)}</td>
                    <td className="num tnum"><b>{fmtRON(inv.totalAmount)}</b></td>
                    <td><StatusBadge status={inv.status} /></td>
                    <td className="mono dim">{inv.anafIndex ?? "—"}</td>
                    <td>
                      <Icon name="chevronRight" size={12} style={{ color: "var(--text-dim)" }} />
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>

        <div style={{ fontSize: 10.5, color: "var(--text-dim)", padding: "4px 2px 20px" }}>
          Datele se actualizează automat la fiecare 60 secunde. Apasă{" "}
          <span className="kbd">F5</span> pentru reîmprospătare manuală. Toate
          sumele sunt în <b>RON</b>.
        </div>
      </div>
      </div>
    </div>
  );
}
