import { useState, useEffect } from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import type { CreateLineInput, VatCategory } from "@/types";

function fmtRON(n: number): string {
  return n.toLocaleString("ro-RO", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}

function fmtDateRO(iso: string): string {
  const [y, m, d] = iso.split("-");
  return `${d}.${m}.${y}`;
}

const DEFAULT_LINE: CreateLineInput = {
  name: "",
  quantity: 1,
  unit: "buc",
  unitPrice: 0,
  vatRate: 19,
  vatCategory: "S" as VatCategory,
};

export function InvoiceEditPage() {
  const navigate = useNavigate();
  const { id } = useParams({ from: "/invoices/$id/edit" });
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const { data: invoiceData, isLoading } = useQuery({
    queryKey: queryKeys.invoices.detail(id),
    queryFn: () => api.invoices.get(id),
  });

  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const [contactId, setContactId] = useState<string>("");
  const [series, setSeries] = useState<string>("");
  const [invoiceNumber, setInvoiceNumber] = useState<number>(1);
  const [issueDate, setIssueDate] = useState<string>("");
  const [dueDate, setDueDate] = useState<string>("");
  const [notes, setNotes] = useState<string>("");
  const [lines, setLines] = useState<CreateLineInput[]>([{ ...DEFAULT_LINE }]);
  const [initialized, setInitialized] = useState(false);

  // Pre-fill form from loaded invoice
  useEffect(() => {
    if (invoiceData && !initialized) {
      const inv = invoiceData.invoice;
      setContactId(inv.contactId);
      setSeries(inv.series);
      setInvoiceNumber(inv.number);
      setIssueDate(inv.issueDate);
      setDueDate(inv.dueDate);
      setNotes(inv.notes ?? "");
      setLines(
        invoiceData.lines.map((l) => ({
          name: l.name,
          description: l.description ?? undefined,
          quantity: l.quantity,
          unit: l.unit,
          unitPrice: l.unitPrice,
          vatRate: l.vatRate,
          vatCategory: l.vatCategory,
          cpvCode: l.cpvCode ?? undefined,
        }))
      );
      setInitialized(true);
    }
  }, [invoiceData, initialized]);

  const selectedContact = contacts.find((c) => c.id === contactId) ?? null;

  const fullNumber = series
    ? `${series}-${String(invoiceNumber).padStart(7, "0")}`
    : "—";

  const net = lines.reduce((s, l) => s + l.quantity * l.unitPrice, 0);
  const vat = lines.reduce((s, l) => s + l.quantity * l.unitPrice * (l.vatRate / 100), 0);
  const total = net + vat;

  const addLine = () => setLines((prev) => [...prev, { ...DEFAULT_LINE }]);

  const removeLine = (idx: number) =>
    setLines((prev) => prev.filter((_, i) => i !== idx));

  const updateLine = <K extends keyof CreateLineInput>(
    idx: number,
    key: K,
    value: CreateLineInput[K],
  ) =>
    setLines((prev) =>
      prev.map((l, i) => (i === idx ? { ...l, [key]: value } : l)),
    );

  const editMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      if (!contactId) throw new Error("Selectați un cumpărător.");
      if (lines.length === 0) throw new Error("Adăugați cel puțin o linie.");
      return api.invoices.updateDraft(id, {
        companyId: activeCompanyId,
        contactId,
        series,
        number: invoiceNumber,
        issueDate,
        dueDate,
        currency: "RON",
        notes: notes || undefined,
        lines,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      navigate({ to: "/invoices/$id", params: { id } });
    },
  });

  if (isLoading || !initialized) {
    return (
      <div className="content">
        <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>Se încarcă…</div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb" onClick={() => navigate({ to: "/invoices" })} style={{ cursor: "pointer" }}>Facturi emise</span>
          Editare factură ·{" "}
          <span className="mono" style={{ fontWeight: 400, color: "var(--text-muted)" }}>
            {fullNumber}
          </span>
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn" onClick={() => navigate({ to: "/invoices/$id", params: { id } })}>
            <Icon name="x" size={12} /> Renunță <span className="kbd" style={{ marginLeft: 6 }}>Esc</span>
          </button>
          <button
            className="btn primary"
            onClick={() => editMutation.mutate()}
            disabled={editMutation.isPending}
          >
            <Icon name="draft" size={12} /> Salvează modificările{" "}
            <span className="kbd" style={{ marginLeft: 6 }}>Ctrl S</span>
          </button>
        </span>
      </div>

      {editMutation.isError && (
        <div style={{ padding: "8px 16px", background: "#FEE2E2", color: "#DC2626", fontSize: 12 }}>
          <Icon name="alert" size={12} />{" "}
          {editMutation.error instanceof Error
            ? editMutation.error.message
            : "Eroare la salvare."}
        </div>
      )}

      <div className="editor-split">
        <div className="editor-main">

          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Antet factură · date generale</span>
              <span style={{ display: "flex", gap: 6 }}>
                <span className="kbd">Tab</span>
                <span style={{ textTransform: "none", letterSpacing: 0, fontWeight: 400, fontSize: 10.5 }}>
                  pentru câmpul următor
                </span>
              </span>
            </div>
            <div className="panel-body">
              <div className="form-grid">
                <div className="form-section-title">Emitent</div>
                <label>Companie emitentă</label>
                <div className="field">
                  <input
                    className="input"
                    value={company?.legalName ?? ""}
                    readOnly
                    style={{ width: 320, background: "var(--bg)" }}
                  />
                  {company && (
                    <span className="mono muted" style={{ fontSize: 11 }}>
                      CUI {company.cui}
                      {company.registryNumber ? ` · ${company.registryNumber}` : ""}
                    </span>
                  )}
                </div>
                <label>Serie / Număr</label>
                <div className="field">
                  <input
                    className="input mono"
                    value={series}
                    onChange={(e) => setSeries(e.target.value)}
                    style={{ width: 90 }}
                  />
                  <input
                    className="input mono"
                    value={String(invoiceNumber).padStart(7, "0")}
                    readOnly
                    style={{ width: 120 }}
                  />
                </div>
                <label>Data emiterii</label>
                <div className="field">
                  <input
                    className="input"
                    type="date"
                    value={issueDate}
                    onChange={(e) => setIssueDate(e.target.value)}
                    style={{ width: 130 }}
                  />
                  <Icon name="calendar" size={14} style={{ color: "var(--text-muted)" }} />
                </div>
                <label>Data scadenței</label>
                <div className="field">
                  <input
                    className="input"
                    type="date"
                    value={dueDate}
                    onChange={(e) => setDueDate(e.target.value)}
                    style={{ width: 130 }}
                  />
                  <span className="muted" style={{ fontSize: 11 }}>
                    {issueDate && dueDate
                      ? `${fmtDateRO(issueDate)} → ${fmtDateRO(dueDate)}`
                      : "30 zile · termen standard"}
                  </span>
                </div>

                <div className="form-section-title">Cumpărător</div>
                <label>Cumpărător</label>
                <div className="field">
                  <select
                    className="select"
                    value={contactId}
                    onChange={(e) => setContactId(e.target.value)}
                    style={{ width: 320 }}
                  >
                    <option value="">— selectați cumpărătorul —</option>
                    {contacts
                      .filter((c) => c.contactType === "CUSTOMER" || c.contactType === "BOTH")
                      .map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.legalName}
                          {c.cui ? ` · ${c.cui}` : ""}
                        </option>
                      ))}
                  </select>
                </div>
                {selectedContact && (
                  <>
                    <label>CUI</label>
                    <div className="field">
                      <span className="mono muted" style={{ fontSize: 12 }}>
                        {selectedContact.cui ?? "—"}
                      </span>
                      {selectedContact.vatPayer ? (
                        <span style={{ fontSize: 11, color: "#16A34A" }}>
                          <Icon name="check" size={12} /> plătitor TVA
                        </span>
                      ) : (
                        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                          neplătitor TVA
                        </span>
                      )}
                    </div>
                    <label>Adresă</label>
                    <div className="field">
                      <span className="muted" style={{ fontSize: 12 }}>
                        {[selectedContact.address, selectedContact.city, selectedContact.county, selectedContact.country]
                          .filter(Boolean)
                          .join(", ")}
                      </span>
                    </div>
                  </>
                )}
              </div>
            </div>
          </div>

          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Linii factură · {lines.length} articole</span>
            </div>
            <div className="line-items">
              <table>
                <thead>
                  <tr>
                    <th style={{ width: 28 }}>#</th>
                    <th style={{ width: 110 }}>Cod</th>
                    <th>Descriere</th>
                    <th style={{ width: 64 }} className="num">Cant.</th>
                    <th style={{ width: 56 }}>UM</th>
                    <th style={{ width: 100 }} className="num">Preț unitar</th>
                    <th style={{ width: 64 }} className="num">TVA %</th>
                    <th style={{ width: 110 }} className="num">Valoare net</th>
                    <th style={{ width: 110 }} className="num">Total cu TVA</th>
                    <th style={{ width: 28 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((l, i) => {
                    const lineNet = l.quantity * l.unitPrice;
                    const lineTotal = lineNet * (1 + l.vatRate / 100);
                    return (
                      <tr key={i}>
                        <td
                          style={{
                            textAlign: "center",
                            color: "var(--text-dim)",
                            fontFamily: "var(--font-mono)",
                          }}
                        >
                          {i + 1}
                        </td>
                        <td>
                          <input
                            value={l.cpvCode ?? ""}
                            onChange={(e) => updateLine(i, "cpvCode", e.target.value || undefined)}
                            className="mono"
                          />
                        </td>
                        <td>
                          <input
                            value={l.name}
                            onChange={(e) => updateLine(i, "name", e.target.value)}
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.quantity}
                            onChange={(e) => updateLine(i, "quantity", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td>
                          <input
                            value={l.unit}
                            onChange={(e) => updateLine(i, "unit", e.target.value)}
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.unitPrice}
                            onChange={(e) => updateLine(i, "unitPrice", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.vatRate}
                            onChange={(e) => updateLine(i, "vatRate", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td className="num">
                          <input
                            value={lineNet.toFixed(2)}
                            className="num"
                            readOnly
                            style={{ color: "var(--text-muted)" }}
                          />
                        </td>
                        <td className="num">
                          <input
                            value={lineTotal.toFixed(2)}
                            className="num"
                            readOnly
                            style={{ fontWeight: 600 }}
                          />
                        </td>
                        <td>
                          <button
                            className="btn-icon"
                            onClick={() => removeLine(i)}
                            disabled={lines.length === 1}
                          >
                            <Icon name="trash" size={12} />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                  <tr className="line-add-row" onClick={addLine} style={{ cursor: "pointer" }}>
                    <td colSpan={10}>
                      <Icon name="plus" size={12} /> Adaugă linie
                    </td>
                  </tr>
                </tbody>
                <tfoot>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>
                      Subtotal net
                    </td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(net)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>
                      TVA
                    </td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(vat)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td
                      colSpan={6}
                      style={{
                        textAlign: "right",
                        textTransform: "uppercase",
                        fontSize: 11,
                        letterSpacing: 0.04,
                      }}
                    >
                      Total de plată
                    </td>
                    <td className="num"></td>
                    <td className="num"></td>
                    <td className="num tnum" style={{ fontSize: 14, color: "var(--accent)" }}>
                      {fmtRON(total)}{" "}
                      <span style={{ fontSize: 10.5, color: "var(--text-muted)" }}>RON</span>
                    </td>
                    <td></td>
                  </tr>
                </tfoot>
              </table>
            </div>
          </div>

          <div className="panel">
            <div className="panel-header">
              <span>Note · clauze · referințe</span>
              <span />
            </div>
            <div className="panel-body">
              <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                <label>Observații</label>
                <div className="field" style={{ alignItems: "flex-start" }}>
                  <textarea
                    className="input"
                    style={{ width: "100%", height: 64, padding: 6, resize: "vertical" }}
                    value={notes}
                    onChange={(e) => setNotes(e.target.value)}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>

        <aside className="editor-validation">
          <div className="validation-summary">
            <h3>Editare schiță</h3>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 8 }}>
              Modificați datele facturii și apăsați „Salvează modificările".
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}
