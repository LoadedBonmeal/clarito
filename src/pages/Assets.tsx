/**
 * Mijloace fixe — verbatim port of the design "Mijloace fixe.html":
 *   .page-head (title + sub + pill-btn "Mijloc fix nou" + btn-dark spin-btn
 *   "Rulează amortizarea — lună") → .sum-row (valoare de inventar · amortizare
 *   cumulată · valoare rămasă · amortizare lunară) → .scr-card registru
 *   (.tabs În funcțiune/Amortizate integral/Scoase din funcțiune · .scr-search ·
 *   .scr-table cod/descriere/cont/PIF/cost/durată/amortizat/rămas · .row-acts pop)
 *   → .scr-card "Rulare amortizare — stare per activ" (rezultatul rulării) →
 *   .modal-back/.modal rulare amortizare (preview note 6811 = 281x + confirm).
 *
 * ALL wiring preserved: api.assets.list, api.assets.runDepreciation (postează
 * nota 6811 = 281x în GL, OMFP 1802/2014 — începe din luna următoare PIF),
 * api.assets.create/update (AssetModal), api.assets.dispose (DisposeModal,
 * notă 281x + 6583 / 21x), api.assets.delete (confirm), selector lună/an.
 * Coloanele Amortizat/Rămas + sumarele sunt estimări client-side liniare
 * (backend-ul rămâne autoritativ la rulare).
 */

import { useCallback, useMemo, useState, useEffect } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON, parseDec } from "@/lib/utils";
import type { FixedAsset, FixedAssetInput, DepreciationRun, DepreciationMethod } from "@/types";

type TabFilter = "active" | "full" | "disposed";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

/** Full month name (locale-aware) for the depreciation period UI. */
const monthName = (m: number, lng: string): string =>
  new Date(2000, m - 1, 1).toLocaleDateString(lng, { month: "long" });

/** Render at most this many rows (plain table, no virtualizer — design parity). */
const MAX_ROWS = 1000;

// inline icons absent from Ic (verbatim from the prototype)
const SLASH_PATH = '<path d="M18.364 18.364A9 9 0 0 0 5.636 5.636m12.728 12.728A9 9 0 0 1 5.636 5.636m12.728 12.728L5.636 5.636"/>';
const TRASH_PATH = '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

/**
 * Estimare liniară client-side la finele lunii (y, m): amortizarea începe din
 * luna următoare PIF (OMFP 1802/2014), plafonată la durata de viață și la data
 * scoaterii din funcțiune. Backend-ul rămâne sursa de adevăr la rulare.
 */
function estimateAt(a: FixedAsset, y: number, m: number) {
  const cost = parseDec(a.acquisitionCost);
  const life = a.lifeMonths;
  const monthly = life > 0 ? cost / life : 0;
  const pif = a.startUpDate || a.dateOfAcquisition;
  let charged = 0;
  if (pif && life > 0) {
    const [py, pm] = pif.split("-").map(Number);
    let ty = y;
    let tm = m;
    if (a.disposalDate) {
      const [dy, dm] = a.disposalDate.split("-").map(Number);
      if (dy * 12 + dm < ty * 12 + tm) { ty = dy; tm = dm; }
    }
    charged = Math.min(Math.max((ty - py) * 12 + (tm - pm), 0), life);
  }
  const accumulated = monthly * charged;
  return { cost, monthly, charged, accumulated, remaining: cost - accumulated, life };
}

/** Cont de amortizare estimat din contul de imobilizare: 214 → 2814, 2133 → 2813. */
function amortAcctFor(accountId: string): string {
  const c = accountId.trim();
  return `281${c.charAt(2) || "x"}`;
}

// ── RowMenu — design .pop with .pop-item rows (portal-anchored) ───────────────

interface RowMenuItem {
  key: string;
  icon: React.ReactNode;
  label: string;
  danger?: boolean;
  action: () => void;
}

function RowMenu({ items, onClose, anchor }: { items: RowMenuItem[]; onClose: () => void; anchor: DOMRect | null }) {
  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".row-menu-pop")) onClose();
    };
    const tid = setTimeout(() => document.addEventListener("click", h), 0);
    window.addEventListener("scroll", onClose, true);
    return () => {
      clearTimeout(tid);
      document.removeEventListener("click", h);
      window.removeEventListener("scroll", onClose, true);
    };
  }, [onClose]);

  const width = 240;
  const GAP = 4;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const pos: React.CSSProperties = anchor
    ? {
        position: "fixed",
        left: Math.min(Math.max(8, anchor.right - width), vw - width - 8),
        ...(anchor.bottom > vh - 260 ? { bottom: vh - anchor.top + GAP } : { top: anchor.bottom + GAP }),
        zIndex: 100,
        width,
      }
    : { position: "fixed", top: 64, right: 16, zIndex: 100, width };

  return createPortal(
    <div className="row-menu-pop pop show" style={pos} onClick={(e) => e.stopPropagation()}>
      {items.map((item) => (
        <button
          key={item.key}
          type="button"
          className={`pop-item${item.danger ? " danger" : ""}`}
          onClick={item.action}
        >
          {item.icon}
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}

// ── AssetsPage ────────────────────────────────────────────────────────────────

export function AssetsPage() {
  const { t, i18n } = useTranslation();
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [tab, setTab] = useState<TabFilter>("active");
  const [query, setQuery] = useState("");
  const [modal, setModal] = useState<"create" | { edit: FixedAsset } | null>(null);
  const [disposing, setDisposing] = useState<FixedAsset | null>(null);
  const [run, setRun] = useState<DepreciationRun | null>(null);
  const [runPeriod, setRunPeriod] = useState<{ year: number; month: number } | null>(null);
  const [amortOpen, setAmortOpen] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);

  const { closing: amortClosing, close: amortClose } = useAnimatedClose(
    useCallback(() => setAmortOpen(false), []),
  );

  const { data: assets = [] } = useQuery({
    queryKey: ["assets", companyId],
    queryFn: () => api.assets.list(companyId!),
    enabled: !!companyId,
  });

  const period = useMemo(() => {
    const mm = String(month).padStart(2, "0");
    const last = new Date(year, month, 0).getDate();
    return { from: `${year}-${mm}-01`, to: `${year}-${mm}-${String(last).padStart(2, "0")}` };
  }, [year, month]);

  const runMut = useMutation({
    mutationFn: () => api.assets.runDepreciation(companyId!, period.from, period.to),
    onSuccess: (r) => {
      setRun(r);
      setRunPeriod({ year, month });
      setAmortOpen(false);
      r.posted
        ? notify.success(t("assets.notify.posted", { total: r.totalAmount }))
        : notify.info(t("assets.notify.nothingThisMonth"));
    },
    onError: (e) => notify.error(formatError(e, t("assets.notify.runError"))),
  });

  const del = useMutation({
    mutationFn: (id: string) => api.assets.delete(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["assets", companyId] }),
    onError: (e) => notify.error(formatError(e, t("assets.notify.deleteError"))),
  });

  const dispose = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.assets.dispose(companyId!, id, date),
    onSuccess: () => {
      notify.success(t("assets.notify.disposed"));
      setDisposing(null);
      void qc.invalidateQueries({ queryKey: ["assets", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("assets.notify.disposeError"))),
  });

  const nowY = now.getFullYear();
  const nowM = now.getMonth() + 1;

  // Classification + summaries (client-side linear estimates at current month)
  const enriched = useMemo(
    () => assets.map((a) => ({ asset: a, est: estimateAt(a, nowY, nowM) })),
    [assets, nowY, nowM],
  );
  const inService = enriched.filter((e) => e.asset.active && !(e.est.life > 0 && e.est.charged >= e.est.life));
  const fullyAmortized = enriched.filter((e) => e.asset.active && e.est.life > 0 && e.est.charged >= e.est.life);
  const disposed = enriched.filter((e) => !e.asset.active);

  const activeOnes = enriched.filter((e) => e.asset.active);
  const sumCost = activeOnes.reduce((s, e) => s + e.est.cost, 0);
  const sumAccum = activeOnes.reduce((s, e) => s + e.est.accumulated, 0);
  const sumMonthly = activeOnes.reduce((s, e) => s + (e.est.charged < e.est.life ? e.est.monthly : 0), 0);

  const tabRows = tab === "active" ? inService : tab === "full" ? fullyAmortized : disposed;
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return tabRows;
    return tabRows.filter(
      ({ asset }) =>
        asset.assetCode.toLowerCase().includes(q) ||
        asset.description.toLowerCase().includes(q) ||
        asset.accountId.toLowerCase().includes(q),
    );
  }, [tabRows, query]);
  const visibleRows = list.slice(0, MAX_ROWS);

  // Preview rulare — active assets that depreciate in the selected month (estimate)
  const previewRows = useMemo(
    () =>
      enriched
        .filter(({ asset, est }) => {
          if (!asset.active || est.life <= 0) return false;
          const pif = asset.startUpDate || asset.dateOfAcquisition;
          if (!pif) return false;
          const [py, pm] = pif.split("-").map(Number);
          const elapsed = (year - py) * 12 + (month - pm);
          return elapsed >= 1 && elapsed <= est.life;
        })
        .map(({ asset, est }) => ({ asset, monthly: est.monthly })),
    [enriched, year, month],
  );
  const previewTotal = previewRows.reduce((s, r) => s + r.monthly, 0);

  const monthLabel = monthName(month, i18n.language);
  const runLabel = runPeriod
    ? `${monthName(runPeriod.month, i18n.language)} ${runPeriod.year}`
    : `${monthLabel} ${year}`;
  const runAccumTotal = run ? run.states.reduce((s, st) => s + parseDec(st.accumulated), 0) : 0;
  const runRemainTotal = run ? run.states.reduce((s, st) => s + parseDec(st.bookValue), 0) : 0;

  const tabs: Array<{ value: TabFilter; label: string; count: number }> = [
    { value: "active",   label: t("assets.tabs.inService"),      count: inService.length },
    { value: "full",     label: t("assets.tabs.fullyAmortized"), count: fullyAmortized.length },
    { value: "disposed", label: t("assets.tabs.disposed"),       count: disposed.length },
  ];

  if (!companyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("assets.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("assets.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("assets.title")}</h1>
          <p className="sub">
            {t("assets.sub.prefix")} · {t("assets.sub.count", { count: assets.length })} · {t("assets.sub.suffix")}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => setModal("create")}>
            <Ic name="plus" />{t("assets.head.newAsset")}
          </button>
          <button className="btn-dark spin-btn" onClick={() => setAmortOpen(true)}>
            <Ic name="sync" />{t("assets.head.runDepreciation", { month: monthLabel })}
          </button>
        </div>
      </div>

      {/* summary cards (estimare liniară client-side) */}
      <div className="sum-row">
        <div className="sum">
          <div className="l">{t("assets.sum.inventoryValue")}</div>
          <div className="v num">{fmtRON(sumCost)} RON</div>
          <div className="d">{t("assets.sum.inService", { count: activeOnes.length })}</div>
        </div>
        <div className="sum">
          <div className="l">{t("assets.sum.accumulated")}</div>
          <div className="v num">{fmtRON(sumAccum)} RON</div>
          <div className="d">{t("assets.sum.accumulatedDesc")}</div>
        </div>
        <div className="sum">
          <div className="l">{t("assets.sum.remaining")}</div>
          <div className="v num">{fmtRON(sumCost - sumAccum)} RON</div>
          <div className="d">{t("assets.sum.remainingDesc")}</div>
        </div>
        <div className="sum">
          <div className="l">{t("assets.sum.monthly")}</div>
          <div className="v num">{fmtRON(sumMonthly)} RON</div>
          <div className="d">{t("assets.sum.monthlyDesc")}</div>
        </div>
      </div>

      {/* registru */}
      <div className="scr-card" style={{ marginBottom: 14 }}>
        <div className="scr-toolbar">
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${tab === t.value ? " active" : ""}`}
                onClick={() => setTab(t.value)}
              >
                {t.label}<span className="cnt">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search" style={{ width: 190 }}>
            <Ic name="lens" />
            <input
              type="text"
              placeholder={t("assets.search")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {assets.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("assets.states.empty")}
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("assets.states.emptyFiltered")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("assets.table.code")}</th>
                <th>{t("assets.table.description")}</th>
                <th>{t("assets.table.account")}</th>
                <th>{t("assets.table.pif")}</th>
                <th className="r">{t("assets.table.cost")}</th>
                <th className="r">{t("assets.table.lifeMonths")}</th>
                <th className="r">{t("assets.table.amortized")}</th>
                <th className="r">{t("assets.table.remaining")}</th>
                <th className="r" style={{ width: 64 }}></th>
              </tr>
            </thead>
            <tbody>
              {visibleRows.map(({ asset: a, est }) => {
                const menuItems: RowMenuItem[] = [
                  {
                    key: "history",
                    icon: <Ic name="eye" />,
                    label: t("assets.row.history"),
                    // propunere — neimplementat (nu există API de istoric per activ)
                    action: () => { notify.info(t("assets.soon")); setMenuFor(null); setMenuAnchor(null); },
                  },
                  {
                    key: "edit",
                    icon: <Ic name="pen" />,
                    label: t("assets.row.edit"),
                    action: () => { setModal({ edit: a }); setMenuFor(null); setMenuAnchor(null); },
                  },
                ];
                const dangerItems: RowMenuItem[] = [];
                if (a.active) {
                  dangerItems.push({
                    key: "dispose",
                    icon: <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SLASH_PATH }} />,
                    label: t("assets.row.dispose"),
                    danger: true,
                    action: () => { setDisposing(a); setMenuFor(null); setMenuAnchor(null); },
                  });
                }
                dangerItems.push({
                  key: "delete",
                  icon: <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: TRASH_PATH }} />,
                  label: t("assets.row.delete"),
                  danger: true,
                  action: () => {
                    setMenuFor(null); setMenuAnchor(null);
                    void (async () => {
                      if (await confirm(t("assets.confirm.deleteMsg", { name: a.description }), { kind: "warning" })) del.mutate(a.id);
                    })();
                  },
                });
                return (
                  <tr key={a.id} style={a.active ? undefined : { opacity: 0.55 }}>
                    <td><span className="doc">{a.assetCode}</span></td>
                    <td>
                      {a.description}
                      {!a.active && (
                        <span className="chip sent" style={{ marginLeft: 6 }}>{t("assets.row.disposedChip")}</span>
                      )}
                    </td>
                    <td><span className="doc">{a.accountId}</span></td>
                    <td className="num">{fmtRoDate(a.startUpDate || a.dateOfAcquisition)}</td>
                    <td className="r num">{fmtRON(est.cost)}</td>
                    <td className="r num">{t("assets.months", { count: a.lifeMonths })}</td>
                    <td className="r num">{fmtRON(est.accumulated)}</td>
                    <td className="r num">{fmtRON(est.remaining)}</td>
                    <td onClick={(e) => e.stopPropagation()}>
                      <div className="row-acts">
                        <button
                          className="mini-btn"
                          title={t("assets.row.actions")}
                          onClick={(e) => {
                            if (menuFor === a.id) { setMenuFor(null); setMenuAnchor(null); }
                            else { setMenuAnchor(e.currentTarget.getBoundingClientRect()); setMenuFor(a.id); }
                          }}
                        >
                          <Ic name="dots" />
                        </button>
                      </div>
                      {menuFor === a.id && (
                        <RowMenu
                          items={[...menuItems, ...dangerItems]}
                          anchor={menuAnchor}
                          onClose={() => { setMenuFor(null); setMenuAnchor(null); }}
                        />
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {list.length > MAX_ROWS && (
          <div className="tot-foot">
            <span className="muted">{t("assets.footer.showingFirst", { max: MAX_ROWS.toLocaleString(i18n.language), total: list.length.toLocaleString(i18n.language) })}</span>
          </div>
        )}
      </div>

      {/* rulare amortizare — stare per activ */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">{t("assets.run.title", { period: runLabel })}</div>
          <span className="chip sent">{t("assets.run.chip")}</span>
          <div className="spacer" />
          {/* propunere — neimplementat (nu există export pentru rularea de amortizare) */}
          <button className="pill-btn" onClick={() => notify.info(t("assets.soon"))}>
            <Ic name="dl" />{t("assets.run.export")}
          </button>
        </div>
        {run && run.states.length > 0 ? (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("assets.run.table.code")}</th>
                <th>{t("assets.run.table.description")}</th>
                <th className="r">{t("assets.run.table.monthCharge")}</th>
                <th className="r">{t("assets.run.table.accumulated")}</th>
                <th className="r">{t("assets.run.table.remaining")}</th>
                <th>{t("assets.run.table.entry")}</th>
              </tr>
            </thead>
            <tbody>
              {run.states.map((s) => (
                <tr key={s.assetId}>
                  <td><span className="doc">{s.assetCode}</span></td>
                  <td>{s.description}</td>
                  <td className="r num">{fmtRON(s.monthlyCharge)}</td>
                  <td className="r num">{fmtRON(s.accumulated)}</td>
                  <td className="r num">{fmtRON(s.bookValue)}</td>
                  <td><span className="doc">{s.expenseAcct} = {s.amortAcct}</span></td>
                </tr>
              ))}
              <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                <td colSpan={2}>{t("assets.run.total", { period: runLabel })}</td>
                <td className="r num">{fmtRON(run.totalAmount)}</td>
                <td className="r num">{fmtRON(runAccumTotal)}</td>
                <td className="r num">{fmtRON(runRemainTotal)}</td>
                <td>
                  {run.posted ? (
                    <span className="chip paid"><Ic name="check" cls="sic" />{t("assets.run.posted")}</span>
                  ) : (
                    <span className="chip wait"><Ic name="clock" cls="sic" />{t("assets.run.nothingToPost")}</span>
                  )}
                </td>
              </tr>
            </tbody>
          </table>
        ) : (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {run
              ? t("assets.run.emptyRan")
              : t("assets.run.emptyIdle")}
          </div>
        )}
      </div>

      {/* modal rulare amortizare */}
      {amortOpen && (
        <div
          className={`modal-back ${amortClosing ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) amortClose(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt">{t("assets.runModal.title", { month: monthLabel.charAt(0).toUpperCase() + monthLabel.slice(1), year })}</div>
                <div className="ms">
                  {t("assets.runModal.subtitle")}
                </div>
              </div>
              <button className="modal-x" onClick={() => amortClose()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="fgrid" style={{ marginBottom: 14 }}>
                <div className="field">
                  <label>{t("assets.runModal.monthLabel")}</label>
                  <select className="select" value={month} onChange={(e) => setMonth(Number(e.target.value))}>
                    {Array.from({ length: 12 }, (_, i) => (
                      <option key={i + 1} value={i + 1}>{monthName(i + 1, i18n.language)}</option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{t("assets.runModal.yearLabel")}</label>
                  <select className="select num" value={year} onChange={(e) => setYear(Number(e.target.value))}>
                    {[now.getFullYear() - 1, now.getFullYear(), now.getFullYear() + 1].map((y) => (
                      <option key={y} value={y}>{y}</option>
                    ))}
                  </select>
                </div>
              </div>
              {previewRows.length === 0 ? (
                <div style={{ padding: "24px 0", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                  {t("assets.runModal.emptyPreview")}
                </div>
              ) : (
                <table className="scr-table">
                  <thead>
                    <tr><th>{t("assets.runModal.table.asset")}</th><th>{t("assets.runModal.table.entry")}</th><th className="r">{t("assets.runModal.table.amount")}</th></tr>
                  </thead>
                  <tbody>
                    {previewRows.map(({ asset: a, monthly }) => (
                      <tr key={a.id}>
                        <td><span className="doc">{a.assetCode}</span> {a.description}</td>
                        <td><span className="doc">6811 = {amortAcctFor(a.accountId)}</span></td>
                        <td className="r num">{fmtRON(monthly)}</td>
                      </tr>
                    ))}
                    <tr style={{ background: "var(--bg-table-header)", fontWeight: 600 }}>
                      <td colSpan={2}>{t("assets.runModal.total", { month: monthLabel })}</td>
                      <td className="r num">{fmtRON(previewTotal)}</td>
                    </tr>
                  </tbody>
                </table>
              )}
            </div>
            <div className="modal-foot">
              <span className="left">{t("assets.runModal.estimateNote")}</span>
              <button className="pill-btn" onClick={() => amortClose()}>{t("assets.runModal.cancel")}</button>
              <button
                className="btn-dark"
                disabled={runMut.isPending}
                style={runMut.isPending ? { opacity: 0.5 } : undefined}
                onClick={() => runMut.mutate()}
              >
                <Ic name="check" />
                {runMut.isPending ? t("assets.runModal.running") : t("assets.runModal.run")}
              </button>
            </div>
          </div>
        </div>
      )}

      {modal && companyId && (
        <AssetModal
          companyId={companyId}
          asset={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            setModal(null);
            void qc.invalidateQueries({ queryKey: ["assets", companyId] });
          }}
        />
      )}

      {disposing && (
        <DisposeModal
          asset={disposing}
          defaultDate={period.to}
          busy={dispose.isPending}
          onClose={() => setDisposing(null)}
          onConfirm={(date) => dispose.mutate({ id: disposing.id, date })}
        />
      )}
    </div>
  );
}

/** Scoatere din funcțiune — alegerea datei, înlocuiește window.prompt (no-op în WebView-ul Tauri). */
function DisposeModal({
  asset, defaultDate, busy, onClose, onConfirm,
}: {
  asset: FixedAsset;
  defaultDate: string;
  busy: boolean;
  onClose: () => void;
  onConfirm: (date: string) => void;
}) {
  const { t } = useTranslation();
  const [date, setDate] = useState(defaultDate);
  const valid = /^\d{4}-\d{2}-\d{2}$/.test(date.trim());
  const { closing, close } = useAnimatedClose(onClose);

  return (
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) close(); }}
    >
      <div className="modal" style={{ width: 420 }}>
        <div className="modal-head">
          <div>
            <div className="mt">{t("assets.disposeModal.title")}</div>
            <div className="ms">
              {t("assets.disposeModal.subtitle", { desc: asset.description, code: asset.assetCode })}
            </div>
          </div>
          <button className="modal-x" onClick={close}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="field">
            <label>{t("assets.disposeModal.dateLabel")} <span className="req">*</span></label>
            <input
              className="input num"
              placeholder="2026-06-30"
              value={date}
              onChange={(e) => setDate(e.target.value)}
              autoFocus
            />
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={close} disabled={busy}>{t("assets.disposeModal.cancel")}</button>
          <button
            className="btn-dark"
            disabled={busy || !valid}
            style={busy || !valid ? { opacity: 0.5 } : undefined}
            onClick={() => { if (valid) onConfirm(date.trim()); }}
          >
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M18.364 18.364A9 9 0 0 0 5.636 5.636m12.728 12.728A9 9 0 0 1 5.636 5.636m12.728 12.728L5.636 5.636"/>' }} />
            {busy ? t("assets.disposeModal.processing") : t("assets.disposeModal.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}

function AssetModal({
  companyId, asset, onClose, onSaved,
}: {
  companyId: string;
  asset: FixedAsset | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const isEdit = asset !== null;
  const [form, setForm] = useState({
    assetCode: asset?.assetCode ?? "",
    description: asset?.description ?? "",
    accountId: asset?.accountId ?? "213",
    dateOfAcquisition: asset?.dateOfAcquisition ?? "",
    startUpDate: asset?.startUpDate ?? "",
    acquisitionCost: asset?.acquisitionCost ?? "",
    lifeMonths: asset ? String(asset.lifeMonths) : "",
    depreciationMethod: (asset?.depreciationMethod ?? "liniara") as DepreciationMethod,
    subgroup: asset?.subgroup ?? "",
    isNew: asset?.isNew ?? true,
  });
  const [error, setError] = useState<string | null>(null);

  const isSuperAccelerated = form.depreciationMethod === "super_accelerata";

  const save = useMutation({
    mutationFn: () => {
      if (!form.description.trim()) throw new Error(t("assets.modal.descRequired"));
      const input: FixedAssetInput = {
        assetCode: form.assetCode.trim() || "MF",
        description: form.description.trim(),
        accountId: form.accountId.trim() || "213",
        dateOfAcquisition: form.dateOfAcquisition || form.startUpDate,
        startUpDate: form.startUpDate || form.dateOfAcquisition,
        acquisitionCost: form.acquisitionCost.trim() || "0",
        lifeMonths: Number(form.lifeMonths) || 0,
        depreciationMethod: form.depreciationMethod,
        isNew: form.isNew,
        subgroup: isSuperAccelerated ? (form.subgroup.trim() || null) : null,
      };
      return isEdit ? api.assets.update(asset!.id, companyId, input) : api.assets.create(companyId, input);
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, t("assets.notify.saveError"))),
  });

  const field = (k: keyof typeof form) => ({
    value: form[k] as string,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, [k]: e.target.value })),
  });

  const { closing, close } = useAnimatedClose(onClose);

  const DEPR_METHODS: { value: DepreciationMethod; labelKey: string; hint?: string }[] = [
    { value: "liniara", labelKey: "assets.modal.methodLiniara" },
    { value: "degresiva", labelKey: "assets.modal.methodDegresiva", hint: t("assets.modal.methodDegresivaHint") },
    { value: "accelerata", labelKey: "assets.modal.methodAccelerata", hint: t("assets.modal.methodAccelerataHint") },
    { value: "super_accelerata", labelKey: "assets.modal.methodSuperAccelerata", hint: t("assets.modal.methodSuperAccelerataHint") },
  ];

  return (
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget && !save.isPending) close(); }}
    >
      <div className="modal" style={{ width: 560 }}>
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? t("assets.modal.editTitle", { desc: asset.description }) : t("assets.modal.newTitle")}</div>
            <div className="ms">{t("assets.modal.subtitle")}</div>
          </div>
          <button className="modal-x" onClick={close}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body">
          <div className="fgrid">
            <div className="field span2">
              <label>{t("assets.modal.descLabel")} <span className="req">*</span></label>
              <input className="input" placeholder="Laptop Dell" {...field("description")} autoFocus />
            </div>
            <div className="field">
              <label>{t("assets.modal.codeLabel")}</label>
              <input className="input num" placeholder="MF-001" {...field("assetCode")} />
            </div>
            <div className="field">
              <label>{t("assets.modal.accountLabel")}</label>
              <input className="input num" placeholder="213" {...field("accountId")} />
            </div>
            <div className="field">
              <label>{t("assets.modal.acqDateLabel")}</label>
              <input className="input num" type="date" {...field("dateOfAcquisition")} />
            </div>
            <div className="field">
              <label>{t("assets.modal.pifLabel")}</label>
              <input className="input num" type="date" {...field("startUpDate")} />
            </div>
            <div className="field">
              <label>{t("assets.modal.costLabel")}</label>
              <input className="input num" inputMode="decimal" placeholder="5000" {...field("acquisitionCost")} />
            </div>
            <div className="field">
              <label>{t("assets.modal.lifeLabel")}</label>
              <input className="input num" inputMode="numeric" placeholder="36" {...field("lifeMonths")} />
            </div>
            {/* Depreciation method selector */}
            <div className="field span2">
              <label>{t("assets.modal.methodLabel")}</label>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {DEPR_METHODS.map(({ value, labelKey, hint }) => (
                  <label key={value} style={{ display: "flex", alignItems: "flex-start", gap: 8, cursor: "pointer", fontSize: 13 }}>
                    <input
                      type="radio"
                      name="depreciationMethod"
                      value={value}
                      checked={form.depreciationMethod === value}
                      onChange={() => setForm((f) => ({ ...f, depreciationMethod: value }))}
                      style={{ marginTop: 2 }}
                    />
                    <span>
                      <span style={{ fontWeight: 500 }}>{t(labelKey)}</span>
                      {hint && <span style={{ color: "var(--text-sub)", marginLeft: 6 }}>{hint}</span>}
                    </span>
                  </label>
                ))}
              </div>
            </div>
            {/* Super-accelerată eligibility fields */}
            {isSuperAccelerated && (
              <>
                <div className="field">
                  <label>{t("assets.modal.subgroupLabel")}</label>
                  <input className="input num" placeholder="2.1" {...field("subgroup")} />
                </div>
                <div className="field" style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                    <input
                      type="checkbox"
                      checked={form.isNew}
                      onChange={(e) => setForm((f) => ({ ...f, isNew: e.target.checked }))}
                    />
                    {t("assets.modal.isNewLabel")}
                  </label>
                </div>
              </>
            )}
            {error && (
              <div className="span2" style={{ fontSize: 12.5, color: "var(--red)" }}>{error}</div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <button className="pill-btn" onClick={close} disabled={save.isPending}>{t("assets.modal.cancel")}</button>
          <button
            className="btn-dark"
            disabled={save.isPending}
            style={save.isPending ? { opacity: 0.5 } : undefined}
            onClick={() => save.mutate()}
          >
            <Ic name="check" />
            {save.isPending ? t("assets.modal.saving") : isEdit ? t("assets.modal.save") : t("assets.modal.add")}
          </button>
        </div>
      </div>
    </div>
  );
}
