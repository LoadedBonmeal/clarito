/**
 * Documente — verbatim port of the design "Documente.html":
 *   .page-head (title + sub + pill-btn "Exportă arhiva (ZIP)") · .banner.warn
 *   verificare integritate arhivă · .scr-card → .scr-toolbar (.tabs tip document ·
 *   .scr-search · period pill) → .scr-table (tip document / document asociat /
 *   dată / dimensiune / .row-acts eye+dl) → .pager · .sec-h "Șabloane" →
 *   .cols-2-even (șablon factură PDF + șabloane recurente).
 *
 * REAL wiring: archive entries aggregated on FE from api.invoices.list
 * (xmlPath / pdfPath / signatureXmlPath) + api.received.list (xmlPath / pdfPath);
 * deschide via @tauri-apps/plugin-opener openPath; descarcă via plugin-dialog
 * save + plugin-fs readFile/writeFile; dimensiuni via plugin-fs stat (best-effort);
 * api.archive.exportZip (head button), api.archive.verifyIntegrity (banner),
 * api.settings.get invoice_template_preset/accent/show_words (șablon card),
 * api.recurring.list (șabloane recurente), navigate /settings + /recurring.
 */

import { useMemo, useState, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";

type DocKind = "invoice" | "declaration" | "receipt";
type KindFilter = DocKind | "all";
type PeriodFilter = string | "all";

interface DocEntry {
  id: string;
  kind: DocKind;
  /** Litera din .cli-ava (F / R / D / P). */
  ava: string;
  typeLabel: string;
  assoc: string;
  /** ISO yyyy-mm-dd — pentru sortare + filtrul de perioadă. */
  dateIso: string;
  path: string;
}

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

function fmtMonth(ym: string, lng: string): string {
  const [year, month] = ym.split("-");
  const d = new Date(Number(year), Number(month) - 1, 1);
  return d.toLocaleDateString(lng, { month: "long", year: "numeric" });
}

/** "14 KB" / "4,2 MB" — virgulă zecimală românească. */
function fmtSize(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

const PAGE_SIZE = 50;

/** Preset keys with dedicated i18n labels (documents.preset.*). */
const PRESET_KEYS = ["clasic", "modern", "minimal"];
/** Frequency keys with dedicated i18n labels (documents.freq.*). */
const FREQ_KEYS = ["monthly", "quarterly", "annual"];

/** Numerele de pagină afișate în .pager (cu elipse), ca în prototip. */
function pageNumbers(current: number, total: number): Array<number | "…"> {
  if (total <= 7) return Array.from({ length: total }, (_, i) => i + 1);
  const set = new Set<number>([1, total, current - 1, current, current + 1]);
  const nums = Array.from(set).filter((n) => n >= 1 && n <= total).sort((a, b) => a - b);
  const out: Array<number | "…"> = [];
  let prev = 0;
  for (const n of nums) {
    if (prev && n - prev > 1) out.push("…");
    out.push(n);
    prev = n;
  }
  return out;
}

export function DocumentsPage() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [kindFilter, setKindFilter] = useState<KindFilter>("all");
  const [query, setQuery] = useState("");
  const [period, setPeriod] = useState<PeriodFilter>("all");
  const [page, setPage] = useState(1);
  const [exportingZip, setExportingZip] = useState(false);
  const [openPop, setOpenPop] = useState<"" | "period">("");
  const [sizes, setSizes] = useState<Map<string, number | null>>(new Map());

  // Close toolbar pops on outside click
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  // ── Date reale: facturi emise + primite ─────────────────────────────────
  const { data: paged, isLoading, isError: pagedError, error: pagedErr, refetch: refetchPaged } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.invoices.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  const { data: receivedPaged } = useQuery({
    queryKey: queryKeys.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    queryFn: () => api.received.list({ companyId: activeCompanyId ?? undefined, page: { offset: 0, limit: 10000 } }),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });
  const contactNames = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of contacts) m.set(c.id, c.legalName);
    return m;
  }, [contacts]);

  // ── Verificare integritate arhivă ───────────────────────────────────────
  const { data: integrity, refetch: refetchIntegrity } = useQuery({
    queryKey: ["archive-integrity", activeCompanyId ?? ""],
    queryFn: () => api.archive.verifyIntegrity(activeCompanyId!),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  // ── Șablon factură (preset/accent/sumă în litere din setări) ────────────
  const { data: tplSettings } = useQuery({
    queryKey: ["settings", "invoice_template_card"],
    queryFn: async () => {
      const [preset, accent, showWords] = await Promise.all([
        api.settings.get("invoice_template_preset"),
        api.settings.get("invoice_template_accent"),
        api.settings.get("invoice_template_show_words"),
      ]);
      return { preset: preset || "clasic", accent: accent || "#000000", showWords: showWords == null || showWords !== "0" };
    },
  });

  // ── Șabloane recurente ────────────────────────────────────────────────────
  const { data: recurring = [] } = useQuery({
    queryKey: queryKeys.recurring.list(activeCompanyId ?? ""),
    queryFn: () => api.recurring.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  // ── Agregare intrări de arhivă pe FE ─────────────────────────────────────
  const allEntries = useMemo<DocEntry[]>(() => {
    const out: DocEntry[] = [];
    for (const inv of paged?.items ?? []) {
      if (inv.xmlPath) {
        out.push({ id: `${inv.id}:xml`, kind: "invoice", ava: "F", typeLabel: t("documents.type.invoiceXml"), assoc: inv.fullNumber, dateIso: inv.issueDate, path: inv.xmlPath });
      }
      if (inv.pdfPath) {
        out.push({ id: `${inv.id}:pdf`, kind: "invoice", ava: "F", typeLabel: t("documents.type.invoicePdf"), assoc: inv.fullNumber, dateIso: inv.issueDate, path: inv.pdfPath });
      }
      if (inv.signatureXmlPath) {
        out.push({ id: `${inv.id}:sig`, kind: "receipt", ava: "R", typeLabel: t("documents.type.signedReceipt"), assoc: inv.fullNumber, dateIso: inv.issueDate, path: inv.signatureXmlPath });
      }
    }
    for (const ri of receivedPaged?.items ?? []) {
      const num = ri.number ? `${ri.series ? `${ri.series}-` : ""}${ri.number}` : ri.anafDownloadId;
      const assoc = `${num} · ${ri.issuerName}`;
      if (ri.xmlPath) {
        out.push({ id: `${ri.id}:xml`, kind: "invoice", ava: "P", typeLabel: t("documents.type.receivedXml"), assoc, dateIso: ri.issueDate, path: ri.xmlPath });
      }
      if (ri.pdfPath) {
        out.push({ id: `${ri.id}:pdf`, kind: "invoice", ava: "P", typeLabel: t("documents.type.receivedPdf"), assoc, dateIso: ri.issueDate, path: ri.pdfPath });
      }
    }
    out.sort((a, b) => b.dateIso.localeCompare(a.dateIso) || a.assoc.localeCompare(b.assoc));
    return out;
  }, [paged, receivedPaged, t]);

  const counts = useMemo(
    () => ({
      all: allEntries.length,
      invoice: allEntries.filter((e) => e.kind === "invoice").length,
      // propunere — neimplementat: declarațiile (D300/D406/bilanț) nu sunt încă
      // arhivate ca fișiere pe disc, deci tab-ul are mereu 0 intrări.
      declaration: allEntries.filter((e) => e.kind === "declaration").length,
      receipt: allEntries.filter((e) => e.kind === "receipt").length,
    }),
    [allEntries],
  );

  const availableMonths = useMemo(() => {
    const months = new Set<string>();
    for (const e of allEntries) months.add(e.dateIso.slice(0, 7));
    return Array.from(months).filter(Boolean).sort((a, b) => b.localeCompare(a));
  }, [allEntries]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return allEntries
      .filter((e) => kindFilter === "all" || e.kind === kindFilter)
      .filter((e) => period === "all" || e.dateIso.slice(0, 7) === period)
      .filter((e) => !q || e.typeLabel.toLowerCase().includes(q) || e.assoc.toLowerCase().includes(q));
  }, [allEntries, kindFilter, period, query]);

  // Reset la pagina 1 când se schimbă filtrele
  useEffect(() => { setPage(1); }, [kindFilter, period, query]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const safePage = Math.min(page, totalPages);
  const pageRows = filtered.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE);

  // Dimensiuni fișiere (best-effort, doar pentru rândurile vizibile)
  useEffect(() => {
    const missing = pageRows.filter((e) => !sizes.has(e.path));
    if (missing.length === 0) return;
    let cancelled = false;
    void (async () => {
      const { stat } = await import("@tauri-apps/plugin-fs");
      const results = await Promise.all(
        missing.map(async (e): Promise<[string, number | null]> => {
          try {
            const s = await stat(e.path);
            return [e.path, s.size];
          } catch {
            return [e.path, null];
          }
        }),
      );
      if (cancelled) return;
      setSizes((prev) => {
        const next = new Map(prev);
        for (const [p, s] of results) next.set(p, s);
        return next;
      });
    })();
    return () => { cancelled = true; };
  }, [pageRows, sizes]);

  // ── Acțiuni ───────────────────────────────────────────────────────────────
  async function handleOpen(entry: DocEntry) {
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(entry.path);
    } catch (e) {
      notify.error(formatError(e, t("documents.notify.openError")));
    }
  }

  async function handleDownload(entry: DocEntry) {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const fileName = entry.path.split(/[\\/]/).pop() ?? "document";
      const dest = await save({ defaultPath: fileName });
      if (!dest) return;
      const { readFile, writeFile } = await import("@tauri-apps/plugin-fs");
      const data = await readFile(entry.path);
      await writeFile(dest, data);
      notify.success(t("documents.notify.saved", { path: dest }));
    } catch (e) {
      notify.error(formatError(e, t("documents.notify.downloadError")));
    }
  }

  async function handleArchiveZip() {
    if (!activeCompanyId) { notify.warn(t("documents.notify.noActiveCompany")); return; }
    setExportingZip(true);
    try {
      const path = await api.archive.exportZip(activeCompanyId);
      notify.success(t("documents.notify.zipExported", { path }));
      try {
        const { openPath } = await import("@tauri-apps/plugin-opener");
        await openPath(path);
      } catch { /* reveal best-effort */ }
    } catch (e) {
      notify.error(formatError(e, t("documents.notify.zipError")));
    } finally {
      setExportingZip(false);
    }
  }

  const tabs: Array<{ value: KindFilter; label: string; count: number }> = [
    { value: "all",         label: t("documents.tabs.all"),          count: counts.all },
    { value: "invoice",     label: t("documents.tabs.invoices"),     count: counts.invoice },
    { value: "declaration", label: t("documents.tabs.declarations"), count: counts.declaration },
    { value: "receipt",     label: t("documents.tabs.receipts"),     count: counts.receipt },
  ];

  const missingCount = integrity?.missing.length ?? 0;
  const integrityOk = integrity ? integrity.ok && missingCount === 0 : true;

  const activeRecurring = recurring.filter((r) => r.active).length;
  const visibleRecurring = recurring.slice(0, 3);

  const presetKey = tplSettings?.preset ?? "clasic";

  // ── Empty state — nicio companie ─────────────────────────────────────────
  if (!activeCompanyId) {
    return (
      <div className="main-inner page-documents">
        <div className="page-head"><div><h1>{t("documents.title")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("documents.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner page-documents">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("documents.title")}</h1>
          <p className="sub">
            {t("documents.sub")}{activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" disabled={exportingZip} onClick={() => void handleArchiveZip()}>
            <Ic name="dl" />{exportingZip ? t("documents.head.exporting") : t("documents.head.exportZip")}
          </button>
        </div>
      </div>

      {/* verificare integritate arhivă */}
      {integrity && !integrityOk && (
        <div className="banner warn">
          <svg className="ic" viewBox="0 0 24 24"><path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z" /></svg>
          <span>
            <b>{t("documents.integrityWarn.missing", { count: missingCount })}</b>
            {" "}{t("documents.integrityWarn.checked", { n: integrity.checked.toLocaleString(i18n.language) })}
            {integrity.missingUnderRetention > 0 && (
              <>
                {t("documents.integrityWarn.ofWhich")} <b>{integrity.missingUnderRetention}</b> {t("documents.integrityWarn.underRetention")} <b>{t("documents.integrityWarn.fiveYears")}</b> {t("documents.integrityWarn.redownload")}
              </>
            )}
          </span>
          <button className="pill-btn" style={{ marginLeft: "auto", flex: "none" }} onClick={() => void refetchIntegrity()}>
            {t("documents.reverify")}
          </button>
        </div>
      )}
      {integrity && integrityOk && (
        <div className="banner">
          <Ic name="checkC" />
          <span>
            <b>{t("documents.integrityOk.title")}</b> {t("documents.integrityOk.body", { n: integrity.checked.toLocaleString(i18n.language) })}
          </span>
          <button className="pill-btn" style={{ marginLeft: "auto", flex: "none" }} onClick={() => void refetchIntegrity()}>
            {t("documents.reverify")}
          </button>
        </div>
      )}

      {/* arhivă documente */}
      <div className="scr-card" style={{ marginBottom: 16 }}>
        <div className="scr-toolbar">
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${kindFilter === t.value ? " active" : ""}`}
                onClick={() => setKindFilter(t.value)}
              >
                {t.label}<span className="cnt num">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder={t("documents.search")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>

          {/* period pill */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="pill-btn"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "period" ? "" : "period")}
            >
              <Ic name="calendar" />
              {period === "all" ? t("documents.period.allMonths") : fmtMonth(period, i18n.language)}
              <Ic name="chevD" cls="ic" />
            </button>
            {openPop === "period" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 210, maxHeight: 300, overflowY: "auto" }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("documents.period.title")}</div>
                <button className="pop-item" onClick={() => { setPeriod("all"); setOpenPop(""); }}>
                  <span style={{ flex: 1 }}>{t("documents.period.allMonths")}</span>
                  {period === "all" && <Ic name="check" cls="co-check" />}
                </button>
                {availableMonths.map((ym) => (
                  <button key={ym} className="pop-item" onClick={() => { setPeriod(ym); setOpenPop(""); }}>
                    <span style={{ flex: 1 }}>{fmtMonth(ym, i18n.language)}</span>
                    {period === ym && <Ic name="check" cls="co-check" />}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("documents.states.loading")}</div>
        ) : pagedError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={pagedErr} label={t("documents.states.errorLabel")} onRetry={() => void refetchPaged()} />
          </div>
        ) : filtered.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {allEntries.length === 0
              ? t("documents.states.empty")
              : t("documents.states.emptyFiltered")}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("documents.table.type")}</th>
                  <th>{t("documents.table.assoc")}</th>
                  <th>{t("documents.table.date")}</th>
                  <th className="r">{t("documents.table.size")}</th>
                  <th className="r" style={{ width: 90 }}></th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((e) => (
                  <tr key={e.id}>
                    <td><div className="cli"><span className="cli-ava">{e.ava}</span>{e.typeLabel}</div></td>
                    <td className="muted">{e.assoc}</td>
                    <td className="num">{fmtRoDate(e.dateIso)}</td>
                    <td className="r num">{fmtSize(sizes.get(e.path))}</td>
                    <td>
                      <div className="row-acts">
                        <button className="mini-btn" title={t("documents.row.open")} onClick={() => void handleOpen(e)}>
                          <Ic name="eye" />
                        </button>
                        <button className="mini-btn" title={t("documents.row.download")} onClick={() => void handleDownload(e)}>
                          <Ic name="dl" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>

            {/* pager */}
            <div className="pager">
              <span>
                {t("documents.pager.showing")} <b>{((safePage - 1) * PAGE_SIZE + 1).toLocaleString(i18n.language)}–{Math.min(safePage * PAGE_SIZE, filtered.length).toLocaleString(i18n.language)}</b> {t("documents.pager.of")}{" "}
                <b>{filtered.length.toLocaleString(i18n.language)}</b> {t("documents.pager.documentsRetention")}
              </span>
              <div className="pg-btns">
                <button className="pg-btn" disabled={safePage <= 1} onClick={() => setPage(safePage - 1)}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>' }} />
                </button>
                {pageNumbers(safePage, totalPages).map((n, i) =>
                  n === "…" ? (
                    <button key={`e${i}`} className="pg-btn" disabled>…</button>
                  ) : (
                    <button key={n} className={`pg-btn${n === safePage ? " cur" : ""}`} onClick={() => setPage(n)}>
                      {n}
                    </button>
                  ),
                )}
                <button className="pg-btn" disabled={safePage >= totalPages} onClick={() => setPage(safePage + 1)}>
                  <Ic name="chevR" />
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* șabloane */}
      <div className="sec-h">{t("documents.templates.secTitle")}</div>
      <div className="cols-2-even">
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("documents.templates.invoiceTpl")}</div>
            <div className="spacer" />
            <span className="chip sent">{PRESET_KEYS.includes(presetKey) ? t(`documents.preset.${presetKey}`) : presetKey}</span>
          </div>
          <div className="set-row">
            <div>
              <div className="s1">{t("documents.templates.presetAccent")}</div>
              <div className="s2">
                {PRESET_KEYS.includes(presetKey) ? t(`documents.preset.${presetKey}Desc`) : presetKey} · {t("documents.templates.accentWord")} {tplSettings?.accent ?? "#000000"} · {t("documents.templates.amountInWords")} {tplSettings?.showWords === false ? "OFF" : "ON"}
              </div>
            </div>
            <div className="end">
              <button className="pill-btn" onClick={() => void navigate({ to: "/settings" })}>
                {t("documents.templates.editInSettings")}
              </button>
            </div>
          </div>
        </div>

        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("documents.templates.recurringTpl")}</div>
            <div className="spacer" />
            <button
              className="see-all"
              style={{ height: "auto", padding: 0, border: 0, background: "transparent" }}
              onClick={() => void navigate({ to: "/recurring" })}
            >
              {t("documents.templates.seeAll")}<Ic name="chevR" />
            </button>
          </div>
          {recurring.length === 0 ? (
            <div style={{ padding: "20px 16px", fontSize: 12.5, color: "var(--text-2)" }}>
              {t("documents.templates.noRecurring")}
            </div>
          ) : (
            <>
              {visibleRecurring.map((r) => (
                <div className="set-row" key={r.id}>
                  <div>
                    <div className="s1">{r.templateName}</div>
                    <div className="s2">
                      {contactNames.get(r.clientId) ?? "—"} · {FREQ_KEYS.includes(r.frequency) ? t(`documents.freq.${r.frequency}`) : r.frequency}
                    </div>
                  </div>
                  <div className="end">
                    {r.active ? (
                      <span className="chip paid">
                        <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>' }} />
                        {t("documents.templates.active")}
                      </span>
                    ) : (
                      <span className="chip sent"><Ic name="clock" cls="sic" />{t("documents.templates.inactive")}</span>
                    )}
                  </div>
                </div>
              ))}
              {recurring.length > 3 && (
                <div className="set-row">
                  <div className="s2">
                    {t("documents.templates.more", { count: recurring.length - 3 })} · {t("documents.templates.moreActive", { n: activeRecurring })}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
