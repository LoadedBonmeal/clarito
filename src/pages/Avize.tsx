/**
 * Avize page — Aviz de însoțire a mărfii (formular 14-3-6A).
 * List + create form + issue + convertToInvoice + printable view.
 * GL + stock OUT fires on issue (backend). No e-Factura, no ANAF upload.
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "@tanstack/react-router";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type {
  Aviz,
  AvizWithLines,
  CreateAvizInput,
  CreateAvizLineInput,
} from "@/types";

// ─── helpers ──────────────────────────────────────────────────────────────────

const RO_MON = ["ian","feb","mar","apr","mai","iun","iul","aug","sep","oct","nov","dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
const localDateISO = (d = new Date()) => {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
};
const newRowId = () => Math.random().toString(36).slice(2);

function makeEmptyRow(): LineRow {
  return {
    rowId: newRowId(),
    name: "",
    description: undefined,
    quantity: 1,
    unit: "buc",
    unitPrice: 0,
    vatRate: 19,
    vatCategory: "S",
    cpvCode: undefined,
    art331Code: undefined,
    revenueKind: "goods",
  };
}

// "all" | "draft" | "facturate" — "facturate" covers both ISSUED and INVOICED
type TabFilter = "all" | "draft" | "facturate";

const STATUS_CHIP: Record<string, { cls: string }> = {
  DRAFT:    { cls: "sent" },
  ISSUED:   { cls: "paid" },
  INVOICED: { cls: "paid" },
};

// ─── Create modal ─────────────────────────────────────────────────────────────

interface CreateModalProps {
  companyId: string;
  onClose: () => void;
}

function CreateModal({ companyId, onClose }: CreateModalProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { closing, close: animClose } = useAnimatedClose(onClose);

  const [contactId, setContactId] = useState("");
  const [series, setSeries] = useState("AVZ");
  const [avizDate, setAvizDate] = useState(localDateISO());
  const [currency, setCurrency] = useState("RON");
  const [gestiuneId, setGestiuneId] = useState("");
  const [transportMeans, setTransportMeans] = useState("");
  const [driverName, setDriverName] = useState("");
  const [vehiclePlate, setVehiclePlate] = useState("");
  const [destination, setDestination] = useState("");
  const [notes, setNotes] = useState("");
  const [lines, setLines] = useState<LineRow[]>([makeEmptyRow()]);

  const { data: contacts = [] } = useQuery({
    queryKey: ["contacts", "list", { companyId }],
    queryFn: () => api.contacts.list({ companyId }),
    staleTime: 60_000,
  });

  const { data: gestiuni = [] } = useQuery({
    queryKey: ["gestiuni", "list", companyId],
    queryFn: () => api.gestiuni.list(companyId),
    staleTime: 60_000,
  });

  const createMut = useMutation({
    mutationFn: (input: CreateAvizInput) => api.avize.create(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["avize", "list", companyId] });
      notify.success(t("avize.notify.created"));
      animClose();
    },
    onError: (e: unknown) => notify.error(t("avize.notify.createError") + " " + formatError(e)),
  });

  const handleSubmit = () => {
    if (!contactId) { notify.error(t("avize.validate.contact")); return; }
    if (!avizDate) { notify.error(t("avize.validate.date")); return; }
    if (lines.length === 0) { notify.error(t("avize.validate.lines")); return; }

    const mappedLines: CreateAvizLineInput[] = lines.map((r) => ({
      name: r.name,
      description: r.description || null,
      quantity: r.quantity,
      unit: r.unit || "buc",
      unitPrice: r.unitPrice,
      vatRate: r.vatRate,
      vatCategory: r.vatCategory,
      revenueKind: r.revenueKind || null,
    }));

    createMut.mutate({
      companyId,
      contactId,
      series: series || "AVZ",
      avizDate,
      gestiuneId: gestiuneId || null,
      transportMeans: transportMeans || null,
      driverName: driverName || null,
      vehiclePlate: vehiclePlate || null,
      destination: destination || null,
      currency: currency || "RON",
      notes: notes || null,
      lines: mappedLines,
    });
  };

  return (
    <div className={`modal-back ${closing ? "closing" : "show"}`} onClick={animClose}>
      <div
        className="modal lg"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div>
            <div className="mt">{t("avize.modal.createTitle")}</div>
          </div>
          <button className="modal-x" onClick={animClose} aria-label={t("avize.modal.close")}>
            <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
              <path d="M6 18 18 6M6 6l12 12"/>
            </svg>
          </button>
        </div>

        <div className="modal-body">
          {/* Client */}
          <div className="field">
            <label>{t("avize.modal.contact")}</label>
            <select className="select" value={contactId} onChange={(e) => setContactId(e.target.value)}>
              <option value="">{t("avize.modal.contactPick")}</option>
              {contacts.map((c) => (
                <option key={c.id} value={c.id}>{c.legalName}</option>
              ))}
            </select>
          </div>

          {/* Date + Series + Currency */}
          <div className="fgrid">
            <div className="field" style={{ flex: 2 }}>
              <label>{t("avize.modal.date")}</label>
              <input
                type="date"
                className="input"
                value={avizDate}
                onChange={(e) => setAvizDate(e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t("avize.modal.series")}</label>
              <input
                type="text"
                className="input"
                placeholder={t("avize.modal.seriesPlaceholder")}
                value={series}
                onChange={(e) => setSeries(e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t("avize.modal.currency")}</label>
              <select className="select" value={currency} onChange={(e) => setCurrency(e.target.value)}>
                <option value="RON">RON</option>
                <option value="EUR">EUR</option>
                <option value="USD">USD</option>
              </select>
            </div>
          </div>

          {/* Gestiune */}
          <div className="field">
            <label>{t("avize.modal.gestiune")}</label>
            <select className="select" value={gestiuneId} onChange={(e) => setGestiuneId(e.target.value)}>
              <option value="">{t("avize.modal.gestiunePick")}</option>
              {gestiuni.map((g) => (
                <option key={g.id} value={g.id}>{g.denumire}</option>
              ))}
            </select>
          </div>

          {/* Transport */}
          <div className="fgrid">
            <div className="field">
              <label>{t("avize.modal.transportMeans")}</label>
              <input
                type="text"
                className="input"
                placeholder={t("avize.modal.transportMeansPlaceholder")}
                value={transportMeans}
                onChange={(e) => setTransportMeans(e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t("avize.modal.driverName")}</label>
              <input
                type="text"
                className="input"
                placeholder={t("avize.modal.driverNamePlaceholder")}
                value={driverName}
                onChange={(e) => setDriverName(e.target.value)}
              />
            </div>
            <div className="field">
              <label>{t("avize.modal.vehiclePlate")}</label>
              <input
                type="text"
                className="input"
                placeholder={t("avize.modal.vehiclePlatePlaceholder")}
                value={vehiclePlate}
                onChange={(e) => setVehiclePlate(e.target.value)}
              />
            </div>
          </div>

          {/* Destination */}
          <div className="field">
            <label>{t("avize.modal.destination")}</label>
            <input
              type="text"
              className="input"
              placeholder={t("avize.modal.destinationPlaceholder")}
              value={destination}
              onChange={(e) => setDestination(e.target.value)}
            />
          </div>

          {/* Lines */}
          <div>
            <div style={{ marginBottom: 8 }}>{t("avize.modal.lines")}</div>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              companyId={companyId}
              currency={currency}
              issueDate={avizDate}
              showTotals
            />
          </div>

          {/* Notes */}
          <div className="field">
            <label>{t("avize.modal.notes")}</label>
            <textarea
              className="input"
              rows={2}
              placeholder={t("avize.modal.notesPlaceholder")}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              style={{ resize: "vertical" }}
            />
          </div>
        </div>

        <div className="modal-foot">
          <button type="button" className="pill-btn" onClick={animClose}>{t("avize.modal.close")}</button>
          <button className="btn-dark" onClick={handleSubmit} disabled={createMut.isPending}>
            {createMut.isPending ? t("avize.modal.saving") : t("avize.modal.create")}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Print view ───────────────────────────────────────────────────────────────

interface PrintViewProps {
  awl: AvizWithLines;
  companyName: string;
  contactName: string;
  onClose: () => void;
}

function PrintView({ awl, companyName, contactName, onClose }: PrintViewProps) {
  const { t } = useTranslation();
  const { aviz, lines } = awl;

  return (
    <div className="modal-back show" onClick={onClose}>
      <div
        className="modal lg"
        style={{ maxHeight: "90vh", overflowY: "auto" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div>
            <div className="mt">{t("avize.print.title", { number: aviz.fullNumber })}</div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn ghost" onClick={() => window.print()}>
              <Ic name="print" />
              {t("avize.actions.print")}
            </button>
            <button className="sq-btn ghost" onClick={onClose} aria-label={t("avize.modal.close")}>
              <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
                <path d="M6 18 18 6M6 6l12 12"/>
              </svg>
            </button>
          </div>
        </div>

        <div className="modal-body" style={{ padding: "24px 32px" }}>
          {/* Header */}
          <div style={{ textAlign: "center", marginBottom: 16 }}>
            <div style={{ fontSize: 14, color: "var(--fg-muted)" }}>{t("avize.print.formCode")}</div>
            <h2 style={{ margin: "4px 0", fontSize: 18, fontWeight: 700 }}>
              {t("avize.print.title", { number: aviz.fullNumber })}
            </h2>
            <div style={{ fontSize: 13, color: "var(--fg-muted)" }}>{fmtRoDate(aviz.avizDate)}</div>
          </div>

          {/* Parties */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, marginBottom: 16 }}>
            <div style={{ border: "1px solid var(--border)", borderRadius: 6, padding: 12 }}>
              <div style={{ fontSize: 11, textTransform: "uppercase", color: "var(--fg-muted)", marginBottom: 4 }}>{t("avize.print.company")}</div>
              <div style={{ fontWeight: 600 }}>{companyName}</div>
            </div>
            <div style={{ border: "1px solid var(--border)", borderRadius: 6, padding: 12 }}>
              <div style={{ fontSize: 11, textTransform: "uppercase", color: "var(--fg-muted)", marginBottom: 4 }}>{t("avize.print.contact")}</div>
              <div style={{ fontWeight: 600 }}>{contactName}</div>
              {aviz.destination && <div style={{ fontSize: 13, color: "var(--fg-muted)", marginTop: 2 }}>{aviz.destination}</div>}
            </div>
          </div>

          {/* Transport info */}
          {(aviz.transportMeans || aviz.driverName || aviz.vehiclePlate) && (
            <div style={{ display: "flex", gap: 12, marginBottom: 16, flexWrap: "wrap" }}>
              {aviz.transportMeans && (
                <div><span style={{ fontSize: 11, color: "var(--fg-muted)" }}>{t("avize.print.transportMeans")}: </span><span style={{ fontWeight: 500 }}>{aviz.transportMeans}</span></div>
              )}
              {aviz.driverName && (
                <div><span style={{ fontSize: 11, color: "var(--fg-muted)" }}>{t("avize.print.driver")}: </span><span style={{ fontWeight: 500 }}>{aviz.driverName}</span></div>
              )}
              {aviz.vehiclePlate && (
                <div><span style={{ fontSize: 11, color: "var(--fg-muted)" }}>{t("avize.print.plate")}: </span><span style={{ fontWeight: 500 }}>{aviz.vehiclePlate}</span></div>
              )}
            </div>
          )}

          {/* Lines table */}
          <table className="scr-table" style={{ marginBottom: 16 }}>
            <thead>
              <tr>
                <th style={{ width: 32 }}>{t("avize.print.tablePos")}</th>
                <th>{t("avize.print.tableName")}</th>
                <th style={{ width: 60 }}>{t("avize.print.tableUnit")}</th>
                <th className="num" style={{ width: 80 }}>{t("avize.print.tableQty")}</th>
                <th className="num" style={{ width: 100 }}>{t("avize.print.tablePrice")}</th>
                <th className="num" style={{ width: 60 }}>{t("avize.print.tableVat")}</th>
                <th className="num" style={{ width: 100 }}>{t("avize.print.tableTotal")}</th>
              </tr>
            </thead>
            <tbody>
              {lines.map((l, i) => (
                <tr key={l.id}>
                  <td>{i + 1}</td>
                  <td>
                    <div style={{ fontWeight: 500 }}>{l.name}</div>
                    {l.description && <div style={{ fontSize: 12, color: "var(--fg-muted)" }}>{l.description}</div>}
                  </td>
                  <td>{l.unit}</td>
                  <td className="num">{l.quantity}</td>
                  <td className="num">{fmtRON(l.unitPrice)}</td>
                  <td className="num">{l.vatRate}%</td>
                  <td className="num">{fmtRON(l.totalAmount)}</td>
                </tr>
              ))}
            </tbody>
          </table>

          {/* Totals */}
          <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 24 }}>
            <table style={{ borderCollapse: "collapse", minWidth: 240 }}>
              <tbody>
                <tr>
                  <td style={{ padding: "3px 12px", color: "var(--fg-muted)", fontSize: 13 }}>{t("avize.print.subtotal")}</td>
                  <td className="num" style={{ padding: "3px 12px", fontVariantNumeric: "tabular-nums" }}>{fmtRON(aviz.subtotalAmount)} {aviz.currency !== "RON" ? aviz.currency : ""}</td>
                </tr>
                <tr>
                  <td style={{ padding: "3px 12px", color: "var(--fg-muted)", fontSize: 13 }}>{t("avize.print.vat")}</td>
                  <td className="num" style={{ padding: "3px 12px", fontVariantNumeric: "tabular-nums" }}>{fmtRON(aviz.vatAmount)} {aviz.currency !== "RON" ? aviz.currency : ""}</td>
                </tr>
                <tr style={{ borderTop: "2px solid var(--border)" }}>
                  <td style={{ padding: "6px 12px", fontWeight: 700 }}>{t("avize.print.total")}</td>
                  <td className="num" style={{ padding: "6px 12px", fontWeight: 700, fontVariantNumeric: "tabular-nums" }}>{fmtRON(aviz.totalAmount)} {aviz.currency !== "RON" ? aviz.currency : ""}</td>
                </tr>
              </tbody>
            </table>
          </div>

          {/* Signatures */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16, marginTop: 32 }}>
            {[t("avize.print.signExpeditor"), t("avize.print.signTransportator"), t("avize.print.signDestinatar")].map((label) => (
              <div key={label} style={{ borderTop: "1px solid var(--border)", paddingTop: 8, textAlign: "center", fontSize: 12, color: "var(--fg-muted)" }}>
                {label}
              </div>
            ))}
          </div>

          {aviz.notes && (
            <div style={{ marginTop: 16, fontSize: 13, color: "var(--fg-muted)" }}>{aviz.notes}</div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Row actions ──────────────────────────────────────────────────────────────

interface RowActionsProps {
  aviz: Aviz;
  companyId: string;
  onPrint: () => void;
  onClose: () => void;
  anchor: DOMRect | null;
}

function RowActions({ aviz, companyId, onPrint, onClose, anchor }: RowActionsProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [deleteConfirm, setDeleteConfirm] = useState(false);

  const issueMut = useMutation({
    mutationFn: () => api.avize.issue(companyId, aviz.id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["avize", "list", companyId] });
      notify.success(t("avize.notify.issued"));
      onClose();
    },
    onError: (e: unknown) => { notify.error(t("avize.notify.issueError") + " " + formatError(e)); onClose(); },
  });

  const convertMut = useMutation({
    mutationFn: () => api.avize.convertToInvoice(companyId, aviz.id, aviz.invoiceId ?? ""),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["avize", "list", companyId] });
      notify.success(t("avize.notify.converted"));
      onClose();
      void navigate({ to: "/invoices" });
    },
    onError: (e: unknown) => { notify.error(t("avize.notify.convertError") + " " + formatError(e)); onClose(); },
  });

  const style: React.CSSProperties = anchor
    ? { position: "fixed", top: anchor.bottom + 4, right: window.innerWidth - anchor.right, zIndex: 9999 }
    : { position: "fixed", top: 0, right: 0, zIndex: 9999 };

  const s = aviz.status;

  return (
    <div className="pop" style={style}>
      {s === "DRAFT" && (
        <button className="pop-item" onClick={() => issueMut.mutate()}>
          {t("avize.actions.issue")}
        </button>
      )}
      {s === "ISSUED" && (
        <button className="pop-item" onClick={() => convertMut.mutate()}>
          {t("avize.actions.convertToInvoice")}
        </button>
      )}
      <button className="pop-item" onClick={() => { onPrint(); onClose(); }}>
        {t("avize.actions.print")}
      </button>
      {s === "DRAFT" && (
        deleteConfirm
          ? <button className="pop-item danger" onClick={() => onClose()}>{t("avize.actions.confirmDelete")}</button>
          : <button className="pop-item danger" onClick={() => setDeleteConfirm(true)}>{t("avize.actions.delete")}</button>
      )}
    </div>
  );
}

// ─── Tab definitions ──────────────────────────────────────────────────────────

const TABS: { id: TabFilter; label: string }[] = [
  { id: "all",       label: "Toate" },
  { id: "draft",     label: "Draft" },
  { id: "facturate", label: "Facturate" },
];

// ─── Page ─────────────────────────────────────────────────────────────────────

export function AvizePage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<TabFilter>("all");
  const [search, setSearch] = useState("");
  const [modalOpen, setModalOpen] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<DOMRect | null>(null);
  const [menuAviz, setMenuAviz] = useState<Aviz | undefined>();
  const [printData, setPrintData] = useState<{ awl: AvizWithLines; contactName: string; companyName: string } | null>(null);

  const { data: avize = [], isLoading, error } = useQuery({
    queryKey: ["avize", "list", activeCompanyId ?? ""],
    queryFn: () => api.avize.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: ["contacts", "list", { companyId: activeCompanyId }],
    queryFn: () => api.contacts.list({ companyId: activeCompanyId! }),
    enabled: !!activeCompanyId,
    staleTime: 60_000,
  });

  const { data: companies = [] } = useQuery({
    queryKey: ["companies", "list"],
    queryFn: () => api.companies.list(),
    staleTime: 300_000,
  });

  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const contactName = (contactId: string) =>
    contacts.find((c) => c.id === contactId)?.legalName ?? contactId;

  // Count per tab (used for <span className="cnt">)
  const countAll       = avize.length;
  const countDraft     = avize.filter((a) => a.status === "DRAFT").length;
  const countFacturate = avize.filter((a) => a.status === "ISSUED" || a.status === "INVOICED").length;

  const tabCount = (id: TabFilter): number => {
    if (id === "all")       return countAll;
    if (id === "draft")     return countDraft;
    if (id === "facturate") return countFacturate;
    return 0;
  };

  const filtered = useMemo(() => {
    let items = avize;
    if (tab === "draft")     items = items.filter((a) => a.status === "DRAFT");
    if (tab === "facturate") items = items.filter((a) => a.status === "ISSUED" || a.status === "INVOICED");
    if (search.trim()) {
      const s = search.toLowerCase();
      items = items.filter((a) =>
        (a.fullNumber ?? "").toLowerCase().includes(s) ||
        contactName(a.contactId).toLowerCase().includes(s) ||
        (a.destination ?? "").toLowerCase().includes(s)
      );
    }
    return items;
  }, [avize, tab, search, contacts]);

  const handleOpenMenu = useCallback((e: React.MouseEvent, a: Aviz) => {
    e.stopPropagation();
    setMenuAnchor((e.currentTarget as HTMLElement).getBoundingClientRect());
    setMenuAviz(a);
  }, []);

  const handlePrint = useCallback(async (a: Aviz) => {
    if (!activeCompanyId) return;
    try {
      const awl = await queryClient.fetchQuery({
        queryKey: ["avize", "detail", a.id],
        queryFn: () => api.avize.get(activeCompanyId, a.id),
        staleTime: 30_000,
      });
      setPrintData({
        awl,
        contactName: contactName(a.contactId),
        companyName: activeCompany?.legalName ?? "",
      });
    } catch (err) {
      notify.error(formatError(err));
    }
  }, [activeCompanyId, queryClient, contacts, activeCompany]);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="banner info">{t("avize.selectCompany")}</div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">

      <div className="page-head">
        <div>
          <h1>Avize de insotire a marfii</h1>
          <p className="sub">
            Aviz cod 14-3-6A · {activeCompany?.legalName ?? ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => setModalOpen(true)}>
            <Ic name="plus" />
            {t("avize.head.new")}
          </button>
        </div>
      </div>

      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tabs">
            {TABS.map(({ id, label }) => (
              <div
                key={id}
                className={"tab" + (tab === id ? " active" : "")}
                onClick={() => setTab(id)}
              >
                {label}<span className="cnt">{tabCount(id)}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Cauta dupa numar sau client..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>

        {isLoading && <div className="state-row">{t("avize.states.loading")}</div>}
        {error && <QueryErrorBanner label={t("avize.states.errorLabel")} error={error} />}

        {!isLoading && !error && (
          <table className="scr-table">
            <thead>
              <tr>
                <th style={{ width: 150 }}>Numar</th>
                <th style={{ width: 130 }}>Data</th>
                <th>Client</th>
                <th className="r" style={{ width: 150 }}>Valoare</th>
                <th style={{ width: 120 }}>Status</th>
                <th style={{ width: 40 }}></th>
              </tr>
            </thead>
            {filtered.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={6} style={{ padding: 0 }}>
                    <div className="empty">
                      <div className="ei"><Ic name="truck" /></div>
                      <b>Niciun aviz.</b>
                      Emiteti un aviz de insotire a marfii.
                    </div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <tbody>
                {filtered.map((a) => {
                  const chip = STATUS_CHIP[a.status] ?? { cls: "sent" };
                  return (
                    <tr key={a.id}>
                      <td className="num">{a.fullNumber ?? `${a.series}-${String(a.number).padStart(4, "0")}`}</td>
                      <td>{fmtRoDate(a.avizDate)}</td>
                      <td>{contactName(a.contactId)}</td>
                      <td className="r">{fmtRON(a.totalAmount)}{a.currency !== "RON" ? ` ${a.currency}` : ""}</td>
                      <td><span className={`chip ${chip.cls}`}>{t(`avize.status.${a.status}`)}</span></td>
                      <td>
                        <button
                          className="sq-btn ghost"
                          onClick={(e) => handleOpenMenu(e, a)}
                          aria-label="Acțiuni"
                        >
                          <svg width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" viewBox="0 0 24 24">
                            <path d="M6.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0ZM12.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0ZM18.75 12a.75.75 0 1 1-1.5 0 .75.75 0 0 1 1.5 0Z"/>
                          </svg>
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            )}
          </table>
        )}
      </div>

      {menuAviz && (
        <RowActions
          aviz={menuAviz}
          companyId={activeCompanyId}
          anchor={menuAnchor}
          onPrint={() => void handlePrint(menuAviz)}
          onClose={() => { setMenuAviz(undefined); setMenuAnchor(null); }}
        />
      )}

      {modalOpen && (
        <CreateModal
          companyId={activeCompanyId}
          onClose={() => setModalOpen(false)}
        />
      )}

      {printData && (
        <PrintView
          awl={printData.awl}
          companyName={printData.companyName}
          contactName={printData.contactName}
          onClose={() => setPrintData(null)}
        />
      )}
    </div>
  );
}
