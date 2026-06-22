/**
 * Factură primită detaliu — ported to the Claude-Design system, mirroring the
 * freshly-ported InvoiceDetail.tsx (canonical layout from "Factura detaliu.html"):
 *   .main-inner.wide → .page-head (.crumb "Facturi primite › nr" · .head-title
 *   h1+chip · sub "furnizor · CUI · emisă" · .head-actions Deschide XML /
 *   Deschide PDF / Recalculează TVA + status actions Aprobă/Respinge/Arhivează/
 *   Reanalizează) → .cols-2: left = Defalcare TVA (.scr-card + .sold-line) +
 *   Achiziție intra-UE (.tabs toggle) + Istoric document (.spv-log) + Fișiere
 *   (.pay-row + Deschide), right = Furnizor (.cli/.kv) + Detalii document (.kv)
 *   + Plăți furnizor (.pay-row + .sold-line + formular .field/.fgrid).
 *
 * ALL wiring preserved: api.received.get, api.received.updateStatus (Aprobă/
 * Respinge/Arhivează/Reanalizează — status intern, nu trimite la ANAF/SPV),
 * api.received.reparseVat (Recalculează TVA), api.received.setIntraEuKind
 * (Bunuri R5/R18 / Servicii R7/R20 pentru D300), openPath pe XML/PDF, plus
 * plăți furnizor (TVA la încasare): api.receivedPayments.summary/add/delete.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { notify } from "@/lib/toasts";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { isDemoMode } from "@/lib/demo";
import { buildStandaloneHtml } from "@/lib/doc-render/doc-html";
import type { ReceivedStatus } from "@/types";
import type { OrdinPlataData } from "@/lib/tauri";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
/** Unix seconds → "30 mai 2026, 10:44" (prototype .sd/.e2 format). */
const fmtRoDateTime = (unixSec: number | null | undefined) => {
  if (!unixSec) return "—";
  const d = new Date(unixSec * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}, ${String(
    d.getHours(),
  ).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
};

/** "Banchero Media SRL" → "BM" (prototype .cli-ava initials). */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "—";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

// Inline SVG paths from the prototype for icons not in Ic.tsx (same as Received.tsx).
const P_CHECK_CIRCLE = "M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z";
const P_TRASH =
  "m20.25 7.5-.625 10.632a2.25 2.25 0 0 1-2.247 2.118H6.622a2.25 2.25 0 0 1-2.247-2.118L3.75 7.5M10 11.25h4M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125Z";

function InlineIc({ d, cls = "ic" }: { d: string; cls?: string }) {
  return <svg className={cls} viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: `<path d="${d}"/>` }} />;
}

// Status → design chip (.chip variants + icon + i18n label key) — consistent with Received.tsx.
const STATUS_CHIP: Record<ReceivedStatus, { cls: string; icon: React.ReactNode; labelKey: string }> = {
  NEW:      { cls: "sent", icon: <Ic name="dot" cls="sic" />,               labelKey: "detail.status.new" },
  REVIEWED: { cls: "wait", icon: <Ic name="clock" cls="sic" />,             labelKey: "detail.status.reviewed" },
  APPROVED: { cls: "paid", icon: <InlineIc d={P_CHECK_CIRCLE} cls="sic" />, labelKey: "detail.status.approved" },
  REJECTED: { cls: "late", icon: <Ic name="xMark" cls="sic" />,             labelKey: "detail.status.rejected" },
  ARCHIVED: { cls: "sent", icon: <InlineIc d={P_TRASH} cls="sic" />,        labelKey: "detail.status.archived" },
};

const STATUS_LABEL_KEYS: Record<ReceivedStatus, string> = {
  NEW: "detail.statusLower.new",
  REVIEWED: "detail.statusLower.reviewed",
  APPROVED: "detail.statusLower.approved",
  REJECTED: "detail.statusLower.rejected",
  ARCHIVED: "detail.statusLower.archived",
};

// ─── Ordin de Plată HTML builder ─────────────────────────────────────────────

/**
 * Builds a standalone printable HTML page for an Ordin de Plată document.
 * Layout follows the statutory form (Reg. BNR 2/2016 art. 3):
 *   Plătitor | Bancă plătitoare | Beneficiar | Sumă | Monedă | Data | Referință | Nr. OP.
 */
function buildOrdinPlataHtml(op: OrdinPlataData, t: (k: string) => string): string {
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const row = (label: string, value: string, cls = "") =>
    `<tr><td class="lbl">${esc(label)}</td><td class="${cls}">${esc(value) || "<span class='muted'>—</span>"}</td></tr>`;

  const html = `
    <div class="docv">
      <div class="docv-title">${esc(t("detail.op.title"))}</div>
      <div style="display:flex;gap:8px;justify-content:center;margin-bottom:16px;font-size:12px;color:var(--text-2)">
        <span>${esc(t("detail.op.nr"))} <b>${esc(op.opNumber)}</b></span>
        <span>·</span>
        <span>${esc(t("detail.op.data"))} <b>${esc(op.issueDate)}</b></span>
      </div>
      <table class="scr-table op-table" style="margin-bottom:16px">
        <tbody>
          <tr class="section-head"><td colspan="2"><b>${esc(t("detail.op.platitor"))}</b></td></tr>
          ${row(t("detail.op.denumire"), op.platitorName, "bold")}
          ${row("CUI", op.platitorCui)}
          ${row("IBAN", op.platitorIban, "num")}
          ${row(t("detail.op.banca"), op.platitorBanca)}
          <tr class="section-head"><td colspan="2"><b>${esc(t("detail.op.beneficiar"))}</b></td></tr>
          ${row(t("detail.op.denumire"), op.beneficiarName, "bold")}
          ${row("CUI", op.beneficiarCui)}
          ${row("IBAN", op.beneficiarIban, "num")}
          ${row(t("detail.op.banca"), op.beneficiarBanca)}
          <tr class="section-head"><td colspan="2"><b>${esc(t("detail.op.suma"))}</b></td></tr>
          ${row(t("detail.op.valoare"), `${op.amount.replace(".", ",")} ${op.currency}`, "num bold")}
          ${op.amountWords ? row(t("detail.op.sumaLitere"), op.amountWords) : ""}
          ${row(t("detail.op.referinta"), op.reference)}
          ${op.notes ? row(t("detail.op.explicatii"), op.notes) : ""}
        </tbody>
      </table>
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:32px;margin-top:40px;font-size:12px">
        <div>
          <div style="border-top:1px solid var(--line);padding-top:4px;text-align:center">${esc(t("detail.op.semnaturaPlat"))}</div>
        </div>
        <div>
          <div style="border-top:1px solid var(--line);padding-top:4px;text-align:center">${esc(t("detail.op.stampila"))}</div>
        </div>
      </div>
    </div>
  `;

  return buildStandaloneHtml(t("detail.op.title"), html);
}

const METHOD_KEYS: Record<string, string> = {
  transfer: "detail.method.transfer",
  cash: "detail.method.cash",
  card: "detail.method.card",
  compensare: "detail.method.compensare",
};

export function ReceivedDetailPage() {
  const { t } = useTranslation();
  const { id } = useParams({ from: "/received/$id" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const { data: inv, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.received.detail(id),
    queryFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompanySelected")));
      return api.received.get(id, activeCompanyId);
    },
    enabled: !!activeCompanyId,
  });

  const { mutate: updateStatus, isPending } = useMutation({
    mutationFn: (status: ReceivedStatus) => {
      if (!activeCompanyId) {
        notify.warn(t("detail.noActiveCompanySelected"));
        return Promise.reject(new Error(t("detail.noActiveCompanySelected")));
      }
      return api.received.updateStatus(id, activeCompanyId, status);
    },
    onSuccess: (_data, status) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      setSuccessMsg(t("detail.banner.markedAs", { status: t(STATUS_LABEL_KEYS[status]) }));
      setTimeout(() => setSuccessMsg(null), 3000);
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.statusUpdateError"))),
  });

  const { mutate: reparseVat, isPending: isReparsing } = useMutation({
    mutationFn: () => api.received.reparseVat(activeCompanyId ?? undefined),
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      notify.success(t("detail.notify.vatRecalced", { count }));
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.vatRecalcError"))),
  });

  const { mutate: setIntraEuKind, isPending: isSettingKind } = useMutation({
    mutationFn: (kind: "goods" | "services") => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompanySelected")));
      return api.received.setIntraEuKind(id, activeCompanyId, kind);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.intraEuError"))),
  });

  async function openFile(path: string | null, label: string) {
    if (!path) { notify.error(t("detail.notify.fileUnavailable", { label })); return; }
    try { await openPath(path); }
    catch (e) { notify.error(formatError(e, t("detail.notify.fileOpenError", { label }))); }
  }

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("detail.receivedTitle")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("detail.selectCompany")}
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>{t("detail.loading")}</div>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("detail.receivedTitle")}</h1></div></div>
        <QueryErrorBanner error={error} label={t("detail.errorLabel")} onRetry={() => void refetch()} />
      </div>
    );
  }

  if (!inv) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("detail.receivedTitle")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("detail.notFound")}
        </div>
      </div>
    );
  }

  const docNo = inv.series && inv.number ? `${inv.series}-${inv.number}` : inv.anafDownloadId;
  const chip = STATUS_CHIP[inv.status] ?? STATUS_CHIP.NEW;
  const hasVatBreakdown = inv.netAmount != null;

  return (
    <div className="main-inner wide">

      {/* page head */}
      <div className="page-head">
        <div>
          <div className="crumb">
            <a onClick={() => void navigate({ to: "/received" })} style={{ cursor: "pointer" }}>{t("detail.crumb.received")}</a>
            <span className="sep">›</span>
            <span className="num">{docNo}</span>
          </div>
          <div className="head-title">
            <h1 className="num">{docNo}</h1>
            <span className={`chip ${chip.cls}`}>{chip.icon}{t(chip.labelKey)}</span>
          </div>
          <p className="sub">
            {inv.issuerName} · CUI <span className="num">{inv.issuerCui}</span> · {t("detail.head.issuedOn", { date: fmtRoDate(inv.issueDate) })}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" disabled={!inv.xmlPath} onClick={() => void openFile(inv.xmlPath, "XML")}>
            <Ic name="code" />{t("detail.actions.openXml")}
          </button>
          {inv.pdfPath && (
            <button className="pill-btn" onClick={() => void openFile(inv.pdfPath, "PDF")}>
              <Ic name="dl" />{t("detail.actions.openPdf")}
            </button>
          )}
          <button
            className="pill-btn"
            title={t("detail.actions.recalcTitle")}
            disabled={isReparsing}
            onClick={() => reparseVat()}
          >
            <Ic name="sync" />{isReparsing ? t("detail.actions.recalcPending") : t("detail.actions.recalcVat")}
          </button>

          {(inv.status === "NEW" || inv.status === "REVIEWED") && (
            <>
              <button
                className="pill-btn"
                style={{ color: "var(--red)" }}
                title={t("detail.actions.internalStatusTitle")}
                disabled={isPending}
                onClick={() => updateStatus("REJECTED")}
              >
                <svg className="ic" viewBox="0 0 24 24" style={{ stroke: "var(--red)" }} aria-hidden="true">
                  <path d="M6 18 18 6M6 6l12 12" />
                </svg>
                {t("detail.actions.rejectLocal")}
              </button>
              <button
                className="btn-dark"
                title={t("detail.actions.internalStatusTitle")}
                disabled={isPending}
                onClick={() => updateStatus("APPROVED")}
              >
                <Ic name="check" />{t("detail.actions.approveLocal")}
              </button>
            </>
          )}
          {inv.status === "APPROVED" && (
            <button
              className="pill-btn"
              title={t("detail.actions.internalStatusTitle")}
              disabled={isPending}
              onClick={() => updateStatus("ARCHIVED")}
            >
              <Ic name="book" />{t("detail.actions.archive")}
            </button>
          )}
          {inv.status === "REJECTED" && (
            <button
              className="pill-btn"
              title={t("detail.actions.internalStatusTitle")}
              disabled={isPending}
              onClick={() => updateStatus("REVIEWED")}
            >
              <Ic name="undo" />{t("detail.actions.reanalyze")}
            </button>
          )}
        </div>
      </div>

      {/* status banner (design .banner ok) */}
      {successMsg && (
        <div className="banner ok">
          <Ic name="check" />
          <div>{successMsg} {t("detail.banner.internalStatusSuffix")}</div>
          <span className="bx" onClick={() => setSuccessMsg(null)}>✕</span>
        </div>
      )}

      {/* Defalcare TVA lipsă — nu contribuie la TVA deductibilă în D300/D394. */}
      {!hasVatBreakdown && (
        <div className="banner warn">
          <Ic name="receipt" />
          <div>
            <b>{t("detail.banner.vatMissingTitle")}</b> {t("detail.banner.vatMissingBody1")}{" "}
            <b>{t("detail.banner.vatMissingBodyEm")}</b> {t("detail.banner.vatMissingBody2")}
          </div>
        </div>
      )}

      <div className="cols-2">
        <div>
          {/* defalcare TVA */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.vat.title")}</div>
              <div className="spacer" />
              {hasVatBreakdown ? (
                inv.vatAmount != null ? (
                  <span className="chip paid"><Ic name="checkC" cls="sic" />{t("detail.vat.parsedChip")}</span>
                ) : (
                  <span className="chip late"><Ic name="xMark" cls="sic" />{t("detail.vat.missingChip")}</span>
                )
              ) : (
                <span className="chip wait"><Ic name="clock" cls="sic" />{t("detail.vat.unavailableChip")}</span>
              )}
            </div>
            <div className="card-pad">
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "12px 24px" }}>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>{t("detail.vat.base")}</div>
                  <div className="num" style={{ fontSize: 13.5, fontWeight: 600 }}>
                    {inv.netAmount != null ? `${fmtRON(inv.netAmount)} ${inv.currency}` : "—"}
                  </div>
                </div>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>{t("detail.vat.vat")}</div>
                  <div className="num" style={{ fontSize: 13.5, fontWeight: 600 }}>
                    {inv.vatAmount != null ? `${fmtRON(inv.vatAmount)} ${inv.currency}` : "—"}
                  </div>
                </div>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>{t("detail.vat.total")}</div>
                  <div className="num" style={{ fontSize: 13.5, fontWeight: 600 }}>
                    {fmtRON(inv.totalAmount)} {inv.currency}
                  </div>
                </div>
              </div>
            </div>
            <div className="sold-line">
              <span>
                {hasVatBreakdown
                  ? inv.vatAmount != null
                    ? t("detail.vat.contributes")
                    : t("detail.vat.missingCheck")
                  : t("detail.vat.notContributes")}
              </span>
              <b className="num">{t("detail.vat.total")} {fmtRON(inv.totalAmount)} {inv.currency}</b>
            </div>
          </div>

          {/* achiziție intra-UE — tip bunuri / servicii pentru D300 */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.intraEu.title")}</div>
              <div className="spacer" />
              <div className="tabs">
                <button
                  className={`tab${inv.intraEuKind === "goods" ? " active" : ""}`}
                  disabled={isSettingKind || inv.intraEuKind === "goods"}
                  onClick={() => setIntraEuKind("goods")}
                >
                  {t("detail.intraEu.goods")}
                </button>
                <button
                  className={`tab${inv.intraEuKind === "services" ? " active" : ""}`}
                  disabled={isSettingKind || inv.intraEuKind === "services"}
                  onClick={() => setIntraEuKind("services")}
                >
                  {t("detail.intraEu.services")}
                </button>
              </div>
            </div>
            <div className="card-pad" style={{ fontSize: 12.5, color: "var(--text-2)", lineHeight: 1.5 }}>
              {t("detail.intraEu.body1")} <b>{t("detail.intraEu.goods")}</b> → R5/R18, <b>{t("detail.intraEu.services")}</b> → R7/R20.{" "}
              {t("detail.intraEu.body2")}
            </div>
          </div>

          {/* istoric document — SPV (design .spv-log) */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.history.title")}</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>{t("detail.history.receiving")}</span>
            </div>
            <div className="spv-log">
              <div className="spv-ev">
                <div className="act-ic"><Ic name="docDown" /></div>
                <div>
                  <div className="e1"><b>{t("detail.history.downloaded")}</b></div>
                  <div className="e2 num">
                    {fmtRoDateTime(inv.downloadedAt)} · {t("detail.history.downloadId")} <span className="doc">{inv.anafDownloadId}</span>
                    {inv.anafIndex && <> · {t("detail.anaf.anafIndex")} <span className="doc">{inv.anafIndex}</span></>}
                  </div>
                </div>
              </div>
              <div className="spv-ev">
                <div className="act-ic"><Ic name="docText" /></div>
                <div>
                  <div className="e1"><b>{t("detail.history.registeredLocal")}</b></div>
                  <div className="e2 num">{fmtRoDateTime(inv.createdAt)}</div>
                </div>
              </div>
            </div>
            <div className="sold-line">
              <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <Ic name="shield" cls="sic" />{t("detail.history.autoDownloaded")}
              </span>
              <span className="muted" style={{ fontSize: 11.5 }}>{inv.pdfPath ? t("detail.history.archiveXmlPdf") : t("detail.history.archiveXml")}</span>
            </div>
          </div>

          {/* fișiere */}
          <div className="scr-card">
            <div className="scr-toolbar"><div className="tt">{t("detail.files.title")}</div></div>
            <div className="pay-row">
              <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="code" /></div>
              <div style={{ minWidth: 0 }}>
                <div className="p1">{t("detail.files.xml")}</div>
                <div
                  className="p2 num"
                  style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  title={inv.xmlPath}
                >
                  {inv.xmlPath || "—"}
                </div>
              </div>
              <button
                className="pill-btn"
                style={{ marginLeft: "auto" }}
                disabled={!inv.xmlPath}
                onClick={() => void openFile(inv.xmlPath, "XML")}
              >
                <Ic name="eye" />{t("detail.actions.open")}
              </button>
            </div>
            {inv.pdfPath && (
              <div className="pay-row">
                <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="docText" /></div>
                <div style={{ minWidth: 0 }}>
                  <div className="p1">{t("detail.files.pdf")}</div>
                  <div
                    className="p2 num"
                    style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                    title={inv.pdfPath}
                  >
                    {inv.pdfPath}
                  </div>
                </div>
                <button
                  className="pill-btn"
                  style={{ marginLeft: "auto" }}
                  onClick={() => void openFile(inv.pdfPath, "PDF")}
                >
                  <Ic name="eye" />{t("detail.actions.open")}
                </button>
              </div>
            )}
          </div>
        </div>

        <div>
          {/* furnizor */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.supplier.title")}</div>
              <div className="spacer" />
              <a
                className="see-all"
                style={{ height: "auto", padding: 0, cursor: "pointer" }}
                onClick={() => void navigate({ to: "/contacts" })}
              >
                {t("detail.supplier.viewContacts")}<Ic name="chevR" />
              </a>
            </div>
            <div className="card-pad">
              <div className="cli" style={{ marginBottom: 12 }}>
                <span className="cli-ava">{initials(inv.issuerName)}</span>
                <b style={{ fontSize: 13.5 }}>{inv.issuerName}</b>
              </div>
              <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                <dt>CUI</dt><dd className="num">{inv.issuerCui}</dd>
                <dt>{t("detail.supplier.relType")}</dt><dd>{t("detail.supplier.relValue")}</dd>
              </dl>
            </div>
          </div>

          {/* detalii document */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("detail.docDetails.title")}</div></div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                <dt>{t("detail.docDetails.docNo")}</dt><dd className="num">{docNo}</dd>
                <dt>{t("detail.docDetails.issueDate")}</dt><dd>{fmtRoDate(inv.issueDate)}</dd>
                <dt>{t("detail.details.currency")}</dt><dd className="num">{inv.currency}</dd>
                {inv.exchangeRate != null && (
                  <><dt>{t("detail.details.fxRate")}</dt><dd className="num">{inv.exchangeRate}</dd></>
                )}
                <dt>{t("detail.docDetails.anafIndex")}</dt><dd className="num">{inv.anafIndex || "—"}</dd>
                <dt>{t("detail.docDetails.downloadId")}</dt><dd className="num">{inv.anafDownloadId}</dd>
                <dt>{t("detail.docDetails.downloadedAt")}</dt><dd className="num">{fmtRoDateTime(inv.downloadedAt)}</dd>
                <dt>{t("detail.docDetails.createdAt")}</dt><dd className="num">{fmtRoDateTime(inv.createdAt)}</dd>
              </dl>
            </div>
          </div>

          {/* plăți furnizor — buyer-side TVA la încasare */}
          <SupplierPaymentsCard
            receivedInvoiceId={id}
            companyId={activeCompanyId}
            currency={inv.currency ?? "RON"}
          />
        </div>
      </div>
    </div>
  );
}

/**
 * Supplier-payment panel (payments-out). Buyer-side TVA la încasare: the deduction is exercised
 * on the payment date, so recording payments here drives the deferred-deduction release in D300
 * (rd.24/25) and the GL transfer 4428 → 4426.
 */
function SupplierPaymentsCard({
  receivedInvoiceId,
  companyId,
  currency,
}: {
  receivedInvoiceId: string;
  companyId: string;
  currency: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const today = new Date().toISOString().slice(0, 10);
  const [amount, setAmount] = useState("");
  const [paidAt, setPaidAt] = useState(today);
  const [method, setMethod] = useState("transfer");
  const [exchangeRate, setExchangeRate] = useState("");

  const summaryKey = ["receivedPayments", receivedInvoiceId];
  const { data: summary, isLoading } = useQuery({
    queryKey: summaryKey,
    queryFn: () => api.receivedPayments.summary(receivedInvoiceId, companyId),
  });

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: summaryKey });
    void queryClient.invalidateQueries({
      queryKey: queryKeys.received.detail(receivedInvoiceId),
    });
  };

  const { mutate: addPayment, isPending: isAdding } = useMutation({
    mutationFn: () => {
      const rate = parseFloat(exchangeRate);
      return api.receivedPayments.add({
        receivedInvoiceId,
        companyId,
        amount: amount.trim(),
        paidAt,
        method,
        exchangeRate: Number.isFinite(rate) && rate > 0 ? rate : undefined,
      });
    },
    onSuccess: () => {
      setAmount("");
      setExchangeRate("");
      invalidate();
      notify.success(t("detail.notify.supplierPaymentAdded"));
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.paymentAddError"))),
  });

  const { mutate: removePayment, isPending: isRemoving } = useMutation({
    mutationFn: (paymentId: string) => api.receivedPayments.delete(paymentId, companyId),
    onSuccess: invalidate,
    onError: (e) => notify.error(formatError(e, t("detail.notify.paymentDeleteError"))),
  });

  /** Prints an Ordin de Plată for the given received-invoice payment id. */
  const handlePrintOP = async (paymentId: string) => {
    try {
      const data = await api.ordinPlata.getData({ paymentId, companyId });
      const html = buildOrdinPlataHtml(data, t);
      const fileName = `ordin-de-plata-${data.opNumber}.html`;
      if (isDemoMode()) {
        const w = window.open("", "_blank");
        if (w) { w.document.write(html); w.document.close(); }
        return;
      }
      await api.declarations.openDocInBrowser(html, fileName);
    } catch (err) {
      notify.error(formatError(err, t("detail.op.printError")));
    }
  };

  const payStatus = summary?.paymentStatus ?? "UNPAID";
  const payChip = payStatus === "PAID"
    ? { cls: "paid", icon: "check", label: t("detail.pay.paidFull") }
    : payStatus === "PARTIAL"
      ? { cls: "wait", icon: "clock", label: t("detail.pay.paidPartial") }
      : { cls: "sent", icon: "dot", label: t("detail.pay.unpaid") };
  const payments = summary?.payments ?? [];
  const total = parseDec(summary?.totalAmount ?? "0");
  const paid = parseDec(summary?.paidAmount ?? "0");
  const remaining = Math.max(0, total - paid);

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("detail.supplierPayments.title")}</div>
        <div className="spacer" />
        <span className={`chip ${payChip.cls}`}><Ic name={payChip.icon} cls="sic" />{payChip.label}</span>
      </div>
      <div className="card-pad" style={{ paddingBottom: 10, fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
        {t("detail.supplierPayments.body1")}{" "}
        <b>{t("detail.supplierPayments.bodyEm")}</b> {t("detail.supplierPayments.body2")}
      </div>
      {isLoading ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          {t("detail.loading")}
        </div>
      ) : (
        <>
          {payments.length === 0 ? (
            <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
              {t("detail.payments.empty")}
            </div>
          ) : (
            payments.map((p) => (
              <div className="pay-row" key={p.id}>
                <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="card" /></div>
                <div>
                  <div className="p1">{METHOD_KEYS[p.method] ? t(METHOD_KEYS[p.method]) : p.method}</div>
                  <div className="p2 num">{fmtRoDate(p.paidAt)}</div>
                </div>
                <span className="amt num">{fmtRON(p.amount)}</span>
                {/* Tipărește OP — only on transfer payments (OP = bank transfer document) */}
                {(p.method === "transfer" || p.method === "OP") && (
                  <button
                    className="mini-btn"
                    title={t("detail.op.tiparesteOp")}
                    onClick={() => void handlePrintOP(p.id)}
                    style={{ fontSize: 11 }}
                  >
                    <Ic name="dl" />
                  </button>
                )}
                <button
                  className="mini-btn"
                  title={t("detail.payments.deleteTitle")}
                  disabled={isRemoving}
                  onClick={() => removePayment(p.id)}
                >
                  <Ic name="xMark" />
                </button>
              </div>
            ))
          )}
          <div className="sold-line">
            <span>
              {t("detail.supplierPayments.paid")} <span className="num">{fmtRON(summary?.paidAmount ?? "0")}</span> {t("detail.supplierPayments.of")}{" "}
              <span className="num">{fmtRON(summary?.totalAmount ?? "0")}</span>
            </span>
            <b className="num">{t("detail.supplierPayments.rest")} {fmtRON(remaining)} {currency}</b>
          </div>
          <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
            <div className="fgrid">
              <div className="field">
                <label>{t("detail.supplierPayments.amountLabel")} <span className="req">*</span></label>
                <input
                  className="input num"
                  type="text"
                  inputMode="decimal"
                  placeholder="0.00"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  style={{ textAlign: "right" }}
                />
              </div>
              <div className="field">
                <label>{t("detail.supplierPayments.dateLabel")}</label>
                <input
                  className="input num"
                  type="date"
                  value={paidAt}
                  onChange={(e) => setPaidAt(e.target.value)}
                />
              </div>
              <div className="field">
                <label>{t("detail.supplierPayments.methodLabel")}</label>
                <select className="select" value={method} onChange={(e) => setMethod(e.target.value)}>
                  {Object.entries(METHOD_KEYS).map(([v, k]) => (
                    <option key={v} value={v}>{t(k)}</option>
                  ))}
                </select>
              </div>
              {currency !== "RON" && (
                <div className="field">
                  <label>{t("detail.supplierPayments.fxLabel")}</label>
                  <input
                    className="input num"
                    type="number"
                    step="0.0001"
                    min="0"
                    placeholder={t("detail.payModal.fxPlaceholder")}
                    value={exchangeRate}
                    onChange={(e) => setExchangeRate(e.target.value)}
                    style={{ textAlign: "right" }}
                  />
                </div>
              )}
              <div className="field span2" style={{ alignItems: "flex-end" }}>
                <button
                  className="btn-dark"
                  disabled={isAdding || !amount.trim()}
                  onClick={() => addPayment()}
                >
                  <Ic name="plus" />{isAdding ? t("detail.payModal.saving") : t("detail.supplierPayments.add")}
                </button>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
