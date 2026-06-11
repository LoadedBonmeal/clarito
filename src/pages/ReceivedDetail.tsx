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
import type { ReceivedStatus } from "@/types";

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

// Status → design chip (.chip variants + icon + label) — consistent with Received.tsx.
const STATUS_CHIP: Record<ReceivedStatus, { cls: string; icon: React.ReactNode; label: string }> = {
  NEW:      { cls: "sent", icon: <Ic name="dot" cls="sic" />,               label: "Nouă" },
  REVIEWED: { cls: "wait", icon: <Ic name="clock" cls="sic" />,             label: "Revizuită" },
  APPROVED: { cls: "paid", icon: <InlineIc d={P_CHECK_CIRCLE} cls="sic" />, label: "Aprobată" },
  REJECTED: { cls: "late", icon: <Ic name="xMark" cls="sic" />,             label: "Respinsă" },
  ARCHIVED: { cls: "sent", icon: <InlineIc d={P_TRASH} cls="sic" />,        label: "Arhivată" },
};

const STATUS_LABELS: Record<ReceivedStatus, string> = {
  NEW: "nouă",
  REVIEWED: "revizuită",
  APPROVED: "aprobată",
  REJECTED: "respinsă",
  ARCHIVED: "arhivată",
};

const METHOD_LABELS: Record<string, string> = {
  transfer: "Transfer bancar",
  cash: "Numerar",
  card: "Card",
  compensare: "Compensare",
};

const INTERNAL_STATUS_TITLE =
  "Status intern în evidența locală. Nu trimite niciun răspuns la ANAF/SPV.";

export function ReceivedDetailPage() {
  const { id } = useParams({ from: "/received/$id" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const { data: inv, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.received.detail(id),
    queryFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă selectată."));
      return api.received.get(id, activeCompanyId);
    },
    enabled: !!activeCompanyId,
  });

  const { mutate: updateStatus, isPending } = useMutation({
    mutationFn: (status: ReceivedStatus) => {
      if (!activeCompanyId) {
        notify.warn("Nicio companie activă selectată.");
        return Promise.reject(new Error("Nicio companie activă selectată."));
      }
      return api.received.updateStatus(id, activeCompanyId, status);
    },
    onSuccess: (_data, status) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      setSuccessMsg(`Factura a fost marcată ca ${STATUS_LABELS[status]}.`);
      setTimeout(() => setSuccessMsg(null), 3000);
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza statusul.")),
  });

  const { mutate: reparseVat, isPending: isReparsing } = useMutation({
    mutationFn: () => api.received.reparseVat(activeCompanyId ?? undefined),
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      notify.success(`TVA recalculat pentru ${count} facturi.`);
    },
    onError: (e) => notify.error(formatError(e, "Eroare recalculare TVA.")),
  });

  const { mutate: setIntraEuKind, isPending: isSettingKind } = useMutation({
    mutationFn: (kind: "goods" | "services") => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă selectată."));
      return api.received.setIntraEuKind(id, activeCompanyId, kind);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza tipul achiziției.")),
  });

  async function openFile(path: string | null, label: string) {
    if (!path) { notify.error(`${label} indisponibil`); return; }
    try { await openPath(path); }
    catch (e) { notify.error(formatError(e, `Eroare deschidere ${label}.`)); }
  }

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Factură primită</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea factura.
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>Se încarcă…</div>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Factură primită</h1></div></div>
        <QueryErrorBanner error={error} label="factura primită" onRetry={() => void refetch()} />
      </div>
    );
  }

  if (!inv) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Factură primită</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Factura nu a fost găsită.
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
            <a onClick={() => void navigate({ to: "/received" })} style={{ cursor: "pointer" }}>Facturi primite</a>
            <span className="sep">›</span>
            <span className="num">{docNo}</span>
          </div>
          <div className="head-title">
            <h1 className="num">{docNo}</h1>
            <span className={`chip ${chip.cls}`}>{chip.icon}{chip.label}</span>
          </div>
          <p className="sub">
            {inv.issuerName} · CUI <span className="num">{inv.issuerCui}</span> · emisă {fmtRoDate(inv.issueDate)}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" disabled={!inv.xmlPath} onClick={() => void openFile(inv.xmlPath, "XML")}>
            <Ic name="code" />Deschide XML
          </button>
          {inv.pdfPath && (
            <button className="pill-btn" onClick={() => void openFile(inv.pdfPath, "PDF")}>
              <Ic name="dl" />Deschide PDF
            </button>
          )}
          <button
            className="pill-btn"
            title="Re-parsează baza impozabilă și TVA din fișierele XML descărcate."
            disabled={isReparsing}
            onClick={() => reparseVat()}
          >
            <Ic name="sync" />{isReparsing ? "Recalculare…" : "Recalculează TVA"}
          </button>

          {(inv.status === "NEW" || inv.status === "REVIEWED") && (
            <>
              <button
                className="pill-btn"
                style={{ color: "var(--red)" }}
                title={INTERNAL_STATUS_TITLE}
                disabled={isPending}
                onClick={() => updateStatus("REJECTED")}
              >
                <svg className="ic" viewBox="0 0 24 24" style={{ stroke: "var(--red)" }} aria-hidden="true">
                  <path d="M6 18 18 6M6 6l12 12" />
                </svg>
                Respinge local
              </button>
              <button
                className="btn-dark"
                title={INTERNAL_STATUS_TITLE}
                disabled={isPending}
                onClick={() => updateStatus("APPROVED")}
              >
                <Ic name="check" />Aprobă local
              </button>
            </>
          )}
          {inv.status === "APPROVED" && (
            <button
              className="pill-btn"
              title={INTERNAL_STATUS_TITLE}
              disabled={isPending}
              onClick={() => updateStatus("ARCHIVED")}
            >
              <Ic name="book" />Arhivează
            </button>
          )}
          {inv.status === "REJECTED" && (
            <button
              className="pill-btn"
              title={INTERNAL_STATUS_TITLE}
              disabled={isPending}
              onClick={() => updateStatus("REVIEWED")}
            >
              <Ic name="undo" />Reanalizează
            </button>
          )}
        </div>
      </div>

      {/* status banner (design .banner ok) */}
      {successMsg && (
        <div className="banner ok">
          <Ic name="check" />
          <div>{successMsg} Status intern — nu se trimite răspuns la ANAF/SPV.</div>
          <span className="bx" onClick={() => setSuccessMsg(null)}>✕</span>
        </div>
      )}

      {/* Defalcare TVA lipsă — nu contribuie la TVA deductibilă în D300/D394. */}
      {!hasVatBreakdown && (
        <div className="banner warn">
          <Ic name="receipt" />
          <div>
            <b>Defalcare TVA indisponibilă.</b> Această factură nu are baza și TVA extrase din XML,
            deci <b>nu contribuie la TVA deductibilă</b> în D300/D394. Apăsați «Recalculează TVA»
            (în antet) pentru a re-parsa din fișierul XML.
          </div>
        </div>
      )}

      <div className="cols-2">
        <div>
          {/* defalcare TVA */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Defalcare TVA</div>
              <div className="spacer" />
              {hasVatBreakdown ? (
                inv.vatAmount != null ? (
                  <span className="chip paid"><Ic name="checkC" cls="sic" />Parsată din XML</span>
                ) : (
                  <span className="chip late"><Ic name="xMark" cls="sic" />TVA lipsă din XML</span>
                )
              ) : (
                <span className="chip wait"><Ic name="clock" cls="sic" />Indisponibilă</span>
              )}
            </div>
            <div className="card-pad">
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "12px 24px" }}>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>Bază impozabilă</div>
                  <div className="num" style={{ fontSize: 13.5, fontWeight: 600 }}>
                    {inv.netAmount != null ? `${fmtRON(inv.netAmount)} ${inv.currency}` : "—"}
                  </div>
                </div>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>TVA</div>
                  <div className="num" style={{ fontSize: 13.5, fontWeight: 600 }}>
                    {inv.vatAmount != null ? `${fmtRON(inv.vatAmount)} ${inv.currency}` : "—"}
                  </div>
                </div>
                <div>
                  <div style={{ fontSize: 11.5, color: "var(--dim)", marginBottom: 3 }}>Total</div>
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
                    ? "Contribuie la TVA deductibilă în D300/D394"
                    : "TVA lipsă din XML — verificați factura"
                  : "Nu contribuie la TVA deductibilă în D300/D394"}
              </span>
              <b className="num">Total {fmtRON(inv.totalAmount)} {inv.currency}</b>
            </div>
          </div>

          {/* achiziție intra-UE — tip bunuri / servicii pentru D300 */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Achiziție intra-UE</div>
              <div className="spacer" />
              <div className="tabs">
                <button
                  className={`tab${inv.intraEuKind === "goods" ? " active" : ""}`}
                  disabled={isSettingKind || inv.intraEuKind === "goods"}
                  onClick={() => setIntraEuKind("goods")}
                >
                  Bunuri
                </button>
                <button
                  className={`tab${inv.intraEuKind === "services" ? " active" : ""}`}
                  disabled={isSettingKind || inv.intraEuKind === "services"}
                  onClick={() => setIntraEuKind("services")}
                >
                  Servicii
                </button>
              </div>
            </div>
            <div className="card-pad" style={{ fontSize: 12.5, color: "var(--text-2)", lineHeight: 1.5 }}>
              Determină rândul D300: <b>Bunuri</b> → R5/R18, <b>Servicii</b> → R7/R20.
              Relevant numai pentru facturile cu categoria K (achiziții intracomunitare).
            </div>
          </div>

          {/* istoric document — SPV (design .spv-log) */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Istoric document · SPV</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>e-Factura · primire</span>
            </div>
            <div className="spv-log">
              <div className="spv-ev">
                <div className="act-ic"><Ic name="docDown" /></div>
                <div>
                  <div className="e1"><b>Descărcată din SPV</b></div>
                  <div className="e2 num">
                    {fmtRoDateTime(inv.downloadedAt)} · ID descărcare <span className="doc">{inv.anafDownloadId}</span>
                    {inv.anafIndex && <> · index ANAF <span className="doc">{inv.anafIndex}</span></>}
                  </div>
                </div>
              </div>
              <div className="spv-ev">
                <div className="act-ic"><Ic name="docText" /></div>
                <div>
                  <div className="e1"><b>Înregistrată local</b></div>
                  <div className="e2 num">{fmtRoDateTime(inv.createdAt)}</div>
                </div>
              </div>
            </div>
            <div className="sold-line">
              <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <Ic name="shield" cls="sic" />Descărcată automat din SPV
              </span>
              <span className="muted" style={{ fontSize: 11.5 }}>arhivă XML{inv.pdfPath ? " + PDF" : ""} · păstrare legală</span>
            </div>
          </div>

          {/* fișiere */}
          <div className="scr-card">
            <div className="scr-toolbar"><div className="tt">Fișiere</div></div>
            <div className="pay-row">
              <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="code" /></div>
              <div style={{ minWidth: 0 }}>
                <div className="p1">XML e-Factura · UBL 2.1</div>
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
                <Ic name="eye" />Deschide
              </button>
            </div>
            {inv.pdfPath && (
              <div className="pay-row">
                <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="docText" /></div>
                <div style={{ minWidth: 0 }}>
                  <div className="p1">PDF factură</div>
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
                  <Ic name="eye" />Deschide
                </button>
              </div>
            )}
          </div>
        </div>

        <div>
          {/* furnizor */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Furnizor</div>
              <div className="spacer" />
              <a
                className="see-all"
                style={{ height: "auto", padding: 0, cursor: "pointer" }}
                onClick={() => void navigate({ to: "/contacts" })}
              >
                Vezi contacte<Ic name="chevR" />
              </a>
            </div>
            <div className="card-pad">
              <div className="cli" style={{ marginBottom: 12 }}>
                <span className="cli-ava">{initials(inv.issuerName)}</span>
                <b style={{ fontSize: 13.5 }}>{inv.issuerName}</b>
              </div>
              <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                <dt>CUI</dt><dd className="num">{inv.issuerCui}</dd>
                <dt>Tip relație</dt><dd>Furnizor · factură primită prin SPV</dd>
              </dl>
            </div>
          </div>

          {/* detalii document */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Detalii document</div></div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                <dt>Nr. document</dt><dd className="num">{docNo}</dd>
                <dt>Dată emitere</dt><dd>{fmtRoDate(inv.issueDate)}</dd>
                <dt>Monedă</dt><dd className="num">{inv.currency}</dd>
                {inv.exchangeRate != null && (
                  <><dt>Curs valutar</dt><dd className="num">{inv.exchangeRate}</dd></>
                )}
                <dt>Index ANAF</dt><dd className="num">{inv.anafIndex || "—"}</dd>
                <dt>ID descărcare</dt><dd className="num">{inv.anafDownloadId}</dd>
                <dt>Descărcat la</dt><dd className="num">{fmtRoDateTime(inv.downloadedAt)}</dd>
                <dt>Creat la</dt><dd className="num">{fmtRoDateTime(inv.createdAt)}</dd>
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
      notify.success("Plată furnizor înregistrată.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut înregistra plata.")),
  });

  const { mutate: removePayment, isPending: isRemoving } = useMutation({
    mutationFn: (paymentId: string) => api.receivedPayments.delete(paymentId, companyId),
    onSuccess: invalidate,
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge plata.")),
  });

  const payStatus = summary?.paymentStatus ?? "UNPAID";
  const payChip = payStatus === "PAID"
    ? { cls: "paid", icon: "check", label: "Plătită integral" }
    : payStatus === "PARTIAL"
      ? { cls: "wait", icon: "clock", label: "Parțial plătită" }
      : { cls: "sent", icon: "dot", label: "Neplătită" };
  const payments = summary?.payments ?? [];
  const total = parseDec(summary?.totalAmount ?? "0");
  const paid = parseDec(summary?.paidAmount ?? "0");
  const remaining = Math.max(0, total - paid);

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">Plăți furnizor</div>
        <div className="spacer" />
        <span className={`chip ${payChip.cls}`}><Ic name={payChip.icon} cls="sic" />{payChip.label}</span>
      </div>
      <div className="card-pad" style={{ paddingBottom: 10, fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
        Pentru achiziții cu «TVA la încasare», dreptul de deducere se exercită la{" "}
        <b>data plății</b> — plățile de aici deduc TVA în perioada plății (D300) și transferă
        4428 → 4426 în contabilitate.
      </div>
      {isLoading ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          Se încarcă…
        </div>
      ) : (
        <>
          {payments.length === 0 ? (
            <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
              Nicio plată înregistrată.
            </div>
          ) : (
            payments.map((p) => (
              <div className="pay-row" key={p.id}>
                <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="card" /></div>
                <div>
                  <div className="p1">{METHOD_LABELS[p.method] ?? p.method}</div>
                  <div className="p2 num">{fmtRoDate(p.paidAt)}</div>
                </div>
                <span className="amt num">{fmtRON(p.amount)}</span>
                <button
                  className="mini-btn"
                  title="Șterge plata"
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
              Plătit <span className="num">{fmtRON(summary?.paidAmount ?? "0")}</span> din{" "}
              <span className="num">{fmtRON(summary?.totalAmount ?? "0")}</span>
            </span>
            <b className="num">Rest {fmtRON(remaining)} {currency}</b>
          </div>
          <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
            <div className="fgrid">
              <div className="field">
                <label>Sumă (RON) <span className="req">*</span></label>
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
                <label>Data plății</label>
                <input
                  className="input num"
                  type="date"
                  value={paidAt}
                  onChange={(e) => setPaidAt(e.target.value)}
                />
              </div>
              <div className="field">
                <label>Metodă</label>
                <select className="select" value={method} onChange={(e) => setMethod(e.target.value)}>
                  {Object.entries(METHOD_LABELS).map(([v, l]) => (
                    <option key={v} value={v}>{l}</option>
                  ))}
                </select>
              </div>
              {currency !== "RON" && (
                <div className="field">
                  <label>Curs BNR la plată</label>
                  <input
                    className="input num"
                    type="number"
                    step="0.0001"
                    min="0"
                    placeholder="ex. 4.9750"
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
                  <Ic name="plus" />{isAdding ? "Se salvează…" : "Adaugă plată"}
                </button>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
