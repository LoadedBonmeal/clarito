/**
 * Bonuri fiscale / Raport Z — P2 Wave 6
 *
 * Lista bonurilor Z cu:
 *   - Header: serie casă, nr Z, dată, nr bonuri, numerar/card/tichete, total, status + butoane
 *   - Modal creare/editare: toate câmpurile + defalcare per cotă TVA
 *   - Panou de-dup: facturi din aceeași zi → checkbox "încasată prin acest bon" + CASH/CARD
 *   - Lifecycle: DRAFT → POSTED (GL) → STORNAT
 *
 * Printout Raport Z: per HG 479/2003 art.64(2) — deschis în fereastra PDF viewer.
 */

import { useState, useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { fmtRON, parseDec } from "@/lib/utils";
import type {
  FiscalReceipt,
  FiscalReceiptDetail,
  FiscalReceiptInput,
  Invoice,
  VatLineInput,
  InvoiceLinkInput,
  Paginated,
} from "@/types";

// ─── Helpers ──────────────────────────────────────────────────────────────────

const RO_MON = [
  "ian", "feb", "mar", "apr", "mai", "iun",
  "iul", "aug", "sep", "oct", "nov", "dec",
];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

const STATUS_LABEL: Record<string, string> = {
  DRAFT: "Ciornă",
  POSTED: "Contabilizat",
  STORNAT: "Stornat",
};

const STATUS_CLASS: Record<string, string> = {
  DRAFT: "badge-draft",
  POSTED: "badge-posted",
  STORNAT: "badge-cancelled",
};

const emptyInput = (): FiscalReceiptInput => ({
  serieCasa: "CASA1",
  nrZ: 1,
  reportDate: new Date().toISOString().slice(0, 10),
  nrBonuri: 0,
  total: "0.00",
  numerar: "0.00",
  card: "0.00",
  tichete: "0.00",
  notes: "",
});

// ─── Raport Z Printout (HG 479/2003 art.64(2)) ───────────────────────────────

function printRaportZ(detail: FiscalReceiptDetail, companyName: string) {
  const { receipt, vatLines } = detail;
  const html = `<!DOCTYPE html>
<html><head><meta charset="utf-8"/><title>Raport Z ${receipt.nrZ}</title>
<style>
  body { font-family: monospace; font-size: 12px; margin: 20px; max-width: 400px; }
  h1 { font-size: 14px; text-align: center; }
  .sub { text-align: center; font-size: 11px; color: #555; margin-bottom: 8px; }
  table { width: 100%; border-collapse: collapse; }
  td, th { padding: 2px 4px; }
  th { text-align: left; border-bottom: 1px solid #ccc; }
  .r { text-align: right; }
  .sep { border-top: 1px dashed #999; margin: 6px 0; }
  .bold { font-weight: bold; }
</style></head><body>
<h1>${companyName}</h1>
<div class="sub">RAPORT Z DE ÎNCHIDERE ZILNICĂ</div>
<div class="sub">Seria: ${receipt.serieCasa} | Nr. Z: ${receipt.nrZ}</div>
<div class="sub">Data: ${fmtRoDate(receipt.reportDate)}</div>
<div class="sub">Nr. bonuri: ${receipt.nrBonuri}</div>
<hr class="sep"/>
<table>
  <thead><tr><th>Cotă TVA</th><th class="r">Bază</th><th class="r">TVA</th><th class="r">Total</th></tr></thead>
  <tbody>
    ${vatLines
      .map(
        (l) =>
          `<tr><td>${l.rate}%</td>
           <td class="r">${l.baza}</td>
           <td class="r">${l.tva}</td>
           <td class="r">${fmtRON(parseDec(l.baza) + parseDec(l.tva))}</td></tr>`
      )
      .join("")}
  </tbody>
</table>
<hr class="sep"/>
<table>
  <tr><td>TOTAL</td><td class="r bold">${fmtRON(parseDec(receipt.total))} RON</td></tr>
  <tr><td>Numerar</td><td class="r">${fmtRON(parseDec(receipt.numerar))} RON</td></tr>
  <tr><td>Card</td><td class="r">${fmtRON(parseDec(receipt.card))} RON</td></tr>
  ${parseDec(receipt.tichete) > 0 ? `<tr><td>Tichete</td><td class="r">${fmtRON(parseDec(receipt.tichete))} RON</td></tr>` : ""}
  <tr><td colspan="2" class="sep"></td></tr>
  <tr><td>TVA TOTAL</td><td class="r">${fmtRON(vatLines.reduce((s, l) => s + parseDec(l.tva), 0))} RON</td></tr>
</table>
<hr class="sep"/>
<div class="sub" style="margin-top:8px">Document emis conform HG 479/2003 art.64(2)</div>
</body></html>`;

  const w = window.open("", "_blank", "width=450,height=600");
  if (!w) return;
  w.document.write(html);
  w.document.close();
  w.focus();
  w.print();
}

// ─── Form component ───────────────────────────────────────────────────────────

interface ReceiptFormProps {
  companyId: string;
  initial?: FiscalReceipt;
  onSuccess: () => void;
  onCancel: () => void;
}

function ReceiptForm({ companyId, initial, onSuccess, onCancel }: ReceiptFormProps) {
  const queryClient = useQueryClient();

  const [form, setForm] = useState<FiscalReceiptInput>(
    initial
      ? {
          serieCasa: initial.serieCasa,
          nrZ: initial.nrZ,
          reportDate: initial.reportDate,
          nrBonuri: initial.nrBonuri,
          total: initial.total,
          numerar: initial.numerar,
          card: initial.card,
          tichete: initial.tichete,
          notes: initial.notes ?? "",
        }
      : emptyInput()
  );

  const computedTotal = useMemo(
    () =>
      fmtRON(
        parseDec(form.numerar) +
          parseDec(form.card) +
          parseDec(form.tichete ?? "0")
      ),
    [form.numerar, form.card, form.tichete]
  );

  const saveMutation = useMutation({
    mutationFn: () => {
      const payload: FiscalReceiptInput = {
        ...form,
        total: fmtRON(
          parseDec(form.numerar) +
            parseDec(form.card) +
            parseDec(form.tichete ?? "0")
        ),
      };
      if (initial) {
        return api.fiscalReceipts.update(initial.id, companyId, payload);
      } else {
        return api.fiscalReceipts.create(companyId, payload);
      }
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["fiscalReceipts", companyId] });
      notify.success(initial ? "Bon actualizat." : "Bon creat.");
      onSuccess();
    },
    onError: (e) => notify.error(formatError(e, "Eroare la salvare bon.")),
  });

  const f =
    (field: keyof FiscalReceiptInput) =>
    (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
      setForm((prev) => ({ ...prev, [field]: e.target.value }));

  return (
    <>
      <div className="modal-head">
        <div>
          <div className="mt">{initial ? "Editare Raport Z" : "Raport Z nou"}</div>
        </div>
        <button className="modal-x" onClick={onCancel} aria-label="Inchide">
          <Ic name="xMark" />
        </button>
      </div>
      <div className="modal-body">
        <div className="fgrid">
          <div className="field">
            <label>Serie casă</label>
            <input value={form.serieCasa} onChange={f("serieCasa")} className="input" />
          </div>
          <div className="field">
            <label>Nr. Z</label>
            <input
              type="number"
              min={1}
              value={form.nrZ}
              onChange={(e) =>
                setForm((p) => ({ ...p, nrZ: parseInt(e.target.value) || 1 }))
              }
              className="input"
            />
          </div>
          <div className="field">
            <label>Data raportului</label>
            <input
              type="date"
              value={form.reportDate}
              onChange={f("reportDate")}
              className="input"
            />
          </div>
          <div className="field">
            <label>Nr. bonuri</label>
            <input
              type="number"
              min={0}
              value={form.nrBonuri ?? 0}
              onChange={(e) =>
                setForm((p) => ({ ...p, nrBonuri: parseInt(e.target.value) || 0 }))
              }
              className="input"
            />
          </div>
          <div className="field">
            <label>Numerar (RON)</label>
            <input value={form.numerar} onChange={f("numerar")} className="input" />
          </div>
          <div className="field">
            <label>Card (RON)</label>
            <input value={form.card} onChange={f("card")} className="input" />
          </div>
          <div className="field">
            <label>Tichete (RON)</label>
            <input
              value={form.tichete ?? "0.00"}
              onChange={f("tichete")}
              className="input"
            />
          </div>
          <div className="field span2">
            <label>Total Z (calculat)</label>
            <input
              value={computedTotal}
              readOnly
              className="input"
              style={{ color: "var(--accent)", fontWeight: 600 }}
            />
          </div>
          <div className="field span2">
            <label>Observații</label>
            <textarea
              value={form.notes ?? ""}
              onChange={f("notes")}
              className="input"
              rows={2}
            />
          </div>
        </div>
      </div>
      <div className="modal-foot">
        <button type="button" className="pill-btn" onClick={onCancel}>
          Anulare
        </button>
        <button
          className="btn-dark"
          onClick={() => saveMutation.mutate()}
          disabled={saveMutation.isPending}
        >
          {saveMutation.isPending
            ? "Se salvează…"
            : initial
            ? "Actualizează"
            : "Creează bon"}
        </button>
      </div>
    </>
  );
}

// ─── VAT Lines Editor ─────────────────────────────────────────────────────────

interface VatLinesEditorProps {
  companyId: string;
  detail: FiscalReceiptDetail;
  onRefresh: () => void;
}

function VatLinesEditor({ companyId, detail, onRefresh }: VatLinesEditorProps) {
  const queryClient = useQueryClient();
  const { data: vatRates = [] } = useQuery({
    queryKey: ["vatRates", "active"],
    queryFn: () => api.vatRates.list(true),
  });

  const [lines, setLines] = useState<VatLineInput[]>(() =>
    detail.vatLines.map((l) => ({
      vatCategory: l.vatCategory,
      rate: l.rate,
      baza: l.baza,
      tva: l.tva,
    }))
  );

  const isReadonly = detail.receipt.status !== "DRAFT";

  const total = parseDec(detail.receipt.total);
  const sumLines = lines.reduce((s, l) => s + parseDec(l.baza) + parseDec(l.tva), 0);
  const diff = Math.abs(total - sumLines);

  const saveMutation = useMutation({
    mutationFn: () =>
      api.fiscalReceipts.setVatLines(detail.receipt.id, companyId, lines),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["fiscalReceipts", companyId] });
      notify.success("Linii TVA salvate.");
      onRefresh();
    },
    onError: (e) => notify.error(formatError(e, "Eroare la salvare linii TVA.")),
  });

  const addLine = () =>
    setLines((prev) => [
      ...prev,
      {
        vatCategory: "S",
        rate: vatRates[0]?.rate ?? "21",
        baza: "0.00",
        tva: "0.00",
      },
    ]);

  const removeLine = (i: number) =>
    setLines((prev) => prev.filter((_, idx) => idx !== i));

  const updateLine = (i: number, field: keyof VatLineInput, val: string) =>
    setLines((prev) =>
      prev.map((l, idx) => (idx === i ? { ...l, [field]: val } : l))
    );

  return (
    <div className="section-panel">
      <div className="section-head">
        <span>Defalcare pe cote TVA</span>
        {!isReadonly && (
          <button className="sq-btn" onClick={addLine} title="Adaugă cotă">
            <Ic name="plus" />
          </button>
        )}
      </div>
      {lines.length === 0 && (
        <p className="empty-msg">Nicio cotă TVA. Adăugați cel puțin una.</p>
      )}
      {lines.map((l, i) => (
        <div key={i} className="vat-line-row">
          <select
            value={l.rate}
            onChange={(e) => updateLine(i, "rate", e.target.value)}
            className="inp inp-sm"
            disabled={isReadonly}
          >
            {vatRates.map((r) => (
              <option key={r.id} value={r.rate}>
                {r.rate}% — {r.label}
              </option>
            ))}
          </select>
          <label className="inp-grp">
            <span>Bază</span>
            <input
              value={l.baza}
              onChange={(e) => updateLine(i, "baza", e.target.value)}
              className="inp inp-sm"
              disabled={isReadonly}
            />
          </label>
          <label className="inp-grp">
            <span>TVA</span>
            <input
              value={l.tva}
              onChange={(e) => updateLine(i, "tva", e.target.value)}
              className="inp inp-sm"
              disabled={isReadonly}
            />
          </label>
          <span className="vat-line-gross">
            {fmtRON(parseDec(l.baza) + parseDec(l.tva))}
          </span>
          {!isReadonly && (
            <button className="sq-btn sq-sm" onClick={() => removeLine(i)}>
              <Ic name="xMark" />
            </button>
          )}
        </div>
      ))}
      {lines.length > 0 && (
        <div className="vat-line-footer">
          <span
            style={{
              color: diff > 0.01 ? "var(--danger)" : "var(--success)",
              fontWeight: 600,
            }}
          >
            Σ(bază+TVA) = {fmtRON(sumLines)} RON
            {diff > 0.01 ? ` ≠ Total Z (${fmtRON(total)})` : " ✓"}
          </span>
        </div>
      )}
      {!isReadonly && (
        <button
          className="btn-dark btn-sm"
          onClick={() => saveMutation.mutate()}
          disabled={saveMutation.isPending || diff > 0.01}
        >
          {saveMutation.isPending ? "Se salvează…" : "Salvează linii TVA"}
        </button>
      )}
    </div>
  );
}

// ─── Invoice De-dup Panel ─────────────────────────────────────────────────────

interface DedupPanelProps {
  companyId: string;
  detail: FiscalReceiptDetail;
  onRefresh: () => void;
}

function DedupPanel({ companyId, detail, onRefresh }: DedupPanelProps) {
  const queryClient = useQueryClient();
  const isReadonly = detail.receipt.status !== "DRAFT";

  // Fetch invoices from the same day (VALIDATED status)
  const { data: dayInvoicesResult } = useQuery<Paginated<Invoice>>({
    queryKey: ["invoices", companyId, detail.receipt.reportDate],
    queryFn: () =>
      api.invoices.list({
        companyId,
        dateFrom: detail.receipt.reportDate,
        dateTo: detail.receipt.reportDate,
        statuses: ["VALIDATED"],
      }),
    enabled: !!companyId,
  });
  const dayInvoices: Invoice[] = dayInvoicesResult?.items ?? [];

  const linkedIds = new Set(detail.invoiceLinks.map((l) => l.invoiceId));

  const addMutation = useMutation({
    mutationFn: (input: InvoiceLinkInput) =>
      api.fiscalReceipts.addInvoiceLink(detail.receipt.id, companyId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["fiscalReceipts", companyId] });
      onRefresh();
    },
    onError: (e) => notify.error(formatError(e, "Eroare la adăugare legătură.")),
  });

  const removeMutation = useMutation({
    mutationFn: (linkId: string) =>
      api.fiscalReceipts.removeInvoiceLink(linkId, detail.receipt.id, companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["fiscalReceipts", companyId] });
      onRefresh();
    },
    onError: (e) => notify.error(formatError(e, "Eroare la eliminare legătură.")),
  });

  const [payMeans, setPayMeans] = useState<Record<string, "CASH" | "CARD">>({});

  const toggleLink = (inv: Invoice) => {
    if (linkedIds.has(inv.id)) {
      const lnk = detail.invoiceLinks.find((l) => l.invoiceId === inv.id);
      if (lnk) removeMutation.mutate(lnk.id);
    } else {
      addMutation.mutate({
        invoiceId: inv.id,
        amount: inv.totalAmount,
        payMeans: payMeans[inv.id] ?? "CASH",
      });
    }
  };

  // Venit din Z = Total − Σ facturi legate
  const linkedTotal = detail.invoiceLinks.reduce(
    (s, l) => s + parseDec(l.amount),
    0
  );
  const remainder = parseDec(detail.receipt.total) - linkedTotal;

  return (
    <div className="section-panel">
      <div className="section-head">
        <span>Facturi legate (de-dup)</span>
      </div>
      <div className="dedup-summary">
        <span>
          Venit direct din Z:{" "}
          <strong style={{ color: remainder < 0 ? "var(--danger)" : undefined }}>
            {fmtRON(remainder)} RON
          </strong>
          {remainder < 0 && " ⚠ facturi depășesc totalul Z!"}
        </span>
      </div>

      {dayInvoices.length === 0 && (
        <p className="empty-msg">
          Nicio factură emisă în data {fmtRoDate(detail.receipt.reportDate)}.
        </p>
      )}

      {dayInvoices.map((inv) => {
        const linked = linkedIds.has(inv.id);
        return (
          <div
            key={inv.id}
            className={"dedup-row" + (linked ? " dedup-linked" : "")}
          >
            <label className="dedup-chk">
              <input
                type="checkbox"
                checked={linked}
                onChange={() => toggleLink(inv)}
                disabled={isReadonly}
              />
              <span className="dedup-inv-num">{inv.fullNumber}</span>
              <span className="dedup-inv-amt">
                {fmtRON(parseDec(inv.totalAmount))} RON
              </span>
            </label>
            {!linked && !isReadonly && (
              <div className="dedup-means">
                <label>
                  <input
                    type="radio"
                    name={`means-${inv.id}`}
                    value="CASH"
                    checked={(payMeans[inv.id] ?? "CASH") === "CASH"}
                    onChange={() =>
                      setPayMeans((p) => ({ ...p, [inv.id]: "CASH" }))
                    }
                  />
                  Numerar
                </label>
                <label>
                  <input
                    type="radio"
                    name={`means-${inv.id}`}
                    value="CARD"
                    checked={payMeans[inv.id] === "CARD"}
                    onChange={() =>
                      setPayMeans((p) => ({ ...p, [inv.id]: "CARD" }))
                    }
                  />
                  Card
                </label>
              </div>
            )}
            {linked && (
              <span className="badge-dedup">
                {detail.invoiceLinks.find((l) => l.invoiceId === inv.id)?.payMeans ??
                  "CASH"}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ─── Detail Drawer ────────────────────────────────────────────────────────────

interface DetailDrawerProps {
  receiptId: string;
  companyId: string;
  onClose: () => void;
}

function DetailDrawer({ receiptId, companyId, onClose }: DetailDrawerProps) {
  const queryClient = useQueryClient();
  const { data: detail, isLoading, refetch } = useQuery({
    queryKey: ["fiscalReceipts", companyId, receiptId],
    queryFn: () => api.fiscalReceipts.get(receiptId, companyId),
  });

  const { data: company } = useQuery({
    queryKey: ["companies", companyId],
    queryFn: () => api.companies.get(companyId),
    enabled: !!companyId,
  });

  const [editMode, setEditMode] = useState(false);

  const statusMutation = useMutation({
    mutationFn: (status: string) =>
      api.fiscalReceipts.setStatus(receiptId, companyId, status),
    onSuccess: (updated) => {
      void queryClient.invalidateQueries({ queryKey: ["fiscalReceipts", companyId] });
      notify.success(
        updated.status === "POSTED"
          ? "Bon contabilizat în GL."
          : updated.status === "STORNAT"
          ? "Bon stornat — jurnalul GL a fost șters."
          : "Status actualizat."
      );
      void refetch();
    },
    onError: (e) => notify.error(formatError(e, "Eroare la schimbarea statusului.")),
  });

  if (isLoading || !detail) {
    return (
      <div className="drawer-overlay" onClick={onClose}>
        <div className="drawer" onClick={(e) => e.stopPropagation()}>
          <div className="drawer-head">
            <span>Se încarcă…</span>
            <button className="sq-btn" onClick={onClose}>
              <Ic name="xMark" />
            </button>
          </div>
        </div>
      </div>
    );
  }

  const receipt = detail.receipt;
  const isDraft = receipt.status === "DRAFT";
  const isPosted = receipt.status === "POSTED";

  return (
    <div className="drawer-overlay" onClick={onClose}>
      <div className="drawer drawer-lg" onClick={(e) => e.stopPropagation()}>
        <div className="drawer-head">
          <div>
            <span className="drawer-title">
              Raport Z {receipt.nrZ} / {receipt.serieCasa}
            </span>
            <span className="drawer-sub">{fmtRoDate(receipt.reportDate)}</span>
          </div>
          <div className="drawer-head-acts">
            <span className={"badge " + STATUS_CLASS[receipt.status]}>
              {STATUS_LABEL[receipt.status]}
            </span>
            {isDraft && (
              <button
                className="sq-btn"
                title="Editează"
                onClick={() => setEditMode(!editMode)}
              >
                <Ic name="pen" />
              </button>
            )}
            <button
              className="sq-btn"
              title="Print Raport Z"
              onClick={() => printRaportZ(detail, company?.legalName ?? "")}
            >
              <Ic name="printer" />
            </button>
            <button className="sq-btn" onClick={onClose}>
              <Ic name="xMark" />
            </button>
          </div>
        </div>

        {editMode && isDraft ? (
          <ReceiptForm
            companyId={companyId}
            initial={receipt}
            onSuccess={() => {
              setEditMode(false);
              void refetch();
            }}
            onCancel={() => setEditMode(false)}
          />
        ) : (
          <div className="drawer-body">
            {/* Summary */}
            <div className="detail-grid">
              <div className="detail-item">
                <span className="detail-lbl">Total Z</span>
                <span className="detail-val">
                  {fmtRON(parseDec(receipt.total))} RON
                </span>
              </div>
              <div className="detail-item">
                <span className="detail-lbl">Numerar</span>
                <span className="detail-val">
                  {fmtRON(parseDec(receipt.numerar))} RON
                </span>
              </div>
              <div className="detail-item">
                <span className="detail-lbl">Card</span>
                <span className="detail-val">
                  {fmtRON(parseDec(receipt.card))} RON
                </span>
              </div>
              {parseDec(receipt.tichete) > 0 && (
                <div className="detail-item">
                  <span className="detail-lbl">Tichete</span>
                  <span className="detail-val">
                    {fmtRON(parseDec(receipt.tichete))} RON
                  </span>
                </div>
              )}
              <div className="detail-item">
                <span className="detail-lbl">Nr. bonuri</span>
                <span className="detail-val">{receipt.nrBonuri}</span>
              </div>
            </div>

            {/* VAT lines */}
            <VatLinesEditor
              companyId={companyId}
              detail={detail}
              onRefresh={() => void refetch()}
            />

            {/* De-dup panel */}
            <DedupPanel
              companyId={companyId}
              detail={detail}
              onRefresh={() => void refetch()}
            />

            {/* Status actions */}
            <div className="drawer-acts">
              {isDraft && (
                <button
                  className="btn-dark"
                  onClick={async () => {
                    const ok = await confirm(
                      "Contabilizați acest bon? Se vor genera înregistrări GL.",
                      { title: "Confirmare postare" }
                    );
                    if (ok) statusMutation.mutate("POSTED");
                  }}
                  disabled={statusMutation.isPending}
                >
                  Contabilizează (→ POSTED)
                </button>
              )}
              {isPosted && (
                <button
                  className="btn-ghost btn-danger"
                  onClick={async () => {
                    const ok = await confirm(
                      "Stornați bonul? Înregistrările GL vor fi șterse.",
                      { title: "Confirmare storno" }
                    );
                    if (ok) statusMutation.mutate("STORNAT");
                  }}
                  disabled={statusMutation.isPending}
                >
                  Stornare (→ STORNAT)
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Main Page ────────────────────────────────────────────────────────────────

export function FiscalReceiptsPage() {
  const queryClient = useQueryClient();
  const { activeCompanyId } = useAppStore();

  const [showCreate, setShowCreate] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const {
    data: receipts = [],
    isLoading,
    isError,
    error,
  } = useQuery({
    queryKey: ["fiscalReceipts", activeCompanyId],
    queryFn: () =>
      activeCompanyId
        ? api.fiscalReceipts.list(activeCompanyId)
        : Promise.resolve([] as FiscalReceipt[]),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: ["companies"],
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);
  const companyName = activeCompany?.legalName ?? "";

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.fiscalReceipts.delete(id, activeCompanyId!),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: ["fiscalReceipts", activeCompanyId],
      });
      notify.success("Bon șters.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere bon.")),
  });

  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="state-row muted">
          Selectați o companie activă pentru a vedea bonurile fiscale.
        </div>
      </div>
    );
  }

  const count = receipts.length;

  return (
    <div className="main-inner wide">
      {/* Header */}
      <div className="page-head">
        <div>
          <h1>Bonuri fiscale / Raport Z</h1>
          <p className="sub">
            {count} {count === 1 ? "bon înregistrat" : "bonuri inregistrate"} · {companyName}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => setShowCreate(true)}>
            <Ic name="plus" /> Raport Z nou
          </button>
        </div>
      </div>

      {/* Create modal */}
      {showCreate && (
        <div className="modal-back show" onClick={() => setShowCreate(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <ReceiptForm
              companyId={activeCompanyId}
              onSuccess={() => setShowCreate(false)}
              onCancel={() => setShowCreate(false)}
            />
          </div>
        </div>
      )}

      {/* Content */}
      <div className="scr-card">
        {isLoading && <div className="state-row">Se încarcă…</div>}
        {isError && <QueryErrorBanner error={error} label="bonurile fiscale" />}

        {!isLoading && !isError && (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: "130px" }}>Data</th>
                <th className="r" style={{ width: "120px" }}>Total</th>
                <th className="r" style={{ width: "110px" }}>TVA 21%</th>
                <th className="r" style={{ width: "110px" }}>TVA 11%</th>
                <th className="r" style={{ width: "110px" }}>TVA 9%</th>
                <th className="r" style={{ width: "120px" }}>Numerar</th>
                <th className="r" style={{ width: "120px" }}>Card</th>
                <th style={{ width: "110px" }}>Status</th>
                <th></th>
              </tr>
            </thead>
            {receipts.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={9} style={{ padding: 0 }}>
                    <div className="empty">
                      <div className="ei"><Ic name="receipt" /></div>
                      <b>Niciun Raport Z.</b>
                      Apasati Raport Z nou.
                    </div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <tbody>
                {receipts.map((r) => {
                  // Extract per-rate TVA from vatLines if available on the list item,
                  // otherwise show em-dash (detail drawer has full vatLines breakdown).
                  const rv = r as { tva21?: string | null; tva11?: string | null; tva9?: string | null };
                  const tva21 = rv.tva21 != null ? fmtRON(parseDec(rv.tva21)) : "—";
                  const tva11 = rv.tva11 != null ? fmtRON(parseDec(rv.tva11)) : "—";
                  const tva9 = rv.tva9 != null ? fmtRON(parseDec(rv.tva9)) : "—";

                  return (
                    <tr
                      key={r.id}
                      className="trow-link"
                      onClick={() => setSelectedId(r.id)}
                    >
                      <td>{fmtRoDate(r.reportDate)}</td>
                      <td className="r">
                        <strong>{fmtRON(parseDec(r.total))}</strong>
                      </td>
                      <td className="r">{tva21}</td>
                      <td className="r">{tva11}</td>
                      <td className="r">{tva9}</td>
                      <td className="r">{fmtRON(parseDec(r.numerar))}</td>
                      <td className="r">{fmtRON(parseDec(r.card))}</td>
                      <td>
                        <span className={"badge " + STATUS_CLASS[r.status]}>
                          {STATUS_LABEL[r.status]}
                        </span>
                      </td>
                      <td className="row-acts">
                        {r.status === "DRAFT" && (
                          <button
                            className="sq-btn sq-sm"
                            title="Șterge"
                            onClick={async (e) => {
                              e.stopPropagation();
                              const ok = await confirm(
                                "Ștergeți acest bon fiscal?",
                                { title: "Confirmare" }
                              );
                              if (ok) deleteMutation.mutate(r.id);
                            }}
                          >
                            <Ic name="trash" />
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            )}
          </table>
        )}
      </div>

      {/* Detail drawer */}
      {selectedId && (
        <DetailDrawer
          receiptId={selectedId}
          companyId={activeCompanyId}
          onClose={() => setSelectedId(null)}
        />
      )}
    </div>
  );
}
