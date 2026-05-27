/**
 * Modal pentru import CSV — facturi sau contacte.
 */

import { useRef, useState } from "react";
import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";

interface CsvImportModalProps {
  type: "invoices" | "contacts";
  companyId: string;
  onClose: () => void;
  onSuccess: (count: number) => void;
}

const TEMPLATES: Record<"invoices" | "contacts", string> = {
  invoices:
    "company_cui;customer_cui;customer_name;series;number;issue_date;due_date;item_name;qty;unit;unit_price;vat_rate",
  contacts:
    "type;cui;name;address;city;county;email;phone",
};

const TYPE_LABELS: Record<"invoices" | "contacts", string> = {
  invoices: "Facturi",
  contacts: "Contacte",
};

export function CsvImportModal({
  type,
  companyId,
  onClose,
  onSuccess,
}: CsvImportModalProps) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [preview, setPreview] = useState<string[]>([]);
  const [content, setContent] = useState<string>("");
  const [fileName, setFileName] = useState<string>("");
  const [importing, setImporting] = useState(false);
  const [result, setResult] = useState<{ imported: number; errors: string[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewResult, setPreviewResult] = useState<{ imported: number; errors: string[] } | null>(null);
  const [previewing, setPreviewing] = useState(false);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setFileName(file.name);
    setResult(null);
    setError(null);
    setPreviewResult(null);

    const reader = new FileReader();
    reader.onload = (ev) => {
      const text = (ev.target?.result as string) ?? "";
      setContent(text);
      const lines = text.split(/\r?\n/).filter(Boolean);
      setPreview(lines.slice(0, 5));
    };
    reader.readAsText(file, "UTF-8");
  };

  const handleImport = async () => {
    if (!content) { setError("Selectați un fișier CSV."); return; }
    setImporting(true);
    setError(null);
    try {
      const res =
        type === "invoices"
          ? await api.importData.invoicesCsv(content, companyId)
          : await api.importData.contactsCsv(content, companyId);
      setResult(res);
      if (res.imported > 0) onSuccess(res.imported);
    } catch (e) {
      setError("Eroare la import: " + String(e));
    } finally {
      setImporting(false);
    }
  };

  const handleDownloadTemplate = async () => {
    try {
      const template =
        type === "invoices"
          ? await api.importData.invoicesCsvTemplate()
          : await api.importData.contactsCsvTemplate();
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        filters: [{ name: "CSV", extensions: ["csv"] }],
        defaultPath: type === "invoices" ? "template-facturi.csv" : "template-contacte.csv",
      });
      if (path) {
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        await writeTextFile(path, template);
      }
    } catch {
      // Fallback to browser download if Tauri save dialog fails
      const blob = new Blob([TEMPLATES[type] + "\n"], { type: "text/csv;charset=utf-8;" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `template_${type}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    }
  };

  const handleDryRun = async () => {
    if (!content) { setError("Selectați un fișier CSV."); return; }
    setPreviewing(true);
    setError(null);
    try {
      const res =
        type === "invoices"
          ? await api.importData.invoicesCsvDryRun(content, companyId)
          : await api.importData.contactsCsvDryRun(content, companyId);
      setPreviewResult(res);
    } catch (e) {
      setError("Eroare la validare: " + String(e));
    } finally {
      setPreviewing(false);
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          width: 520,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.18)",
          padding: "20px 24px 18px",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
          <h3 style={{ fontSize: 14, fontWeight: 700, margin: 0 }}>
            Import {TYPE_LABELS[type]} din CSV
          </h3>
          <button type="button" className="btn-icon" aria-label="Închide" onClick={onClose}>
            <Icon name="x" size={14} />
          </button>
        </div>

        {/* Template download */}
        <div style={{ marginBottom: 12 }}>
          <button type="button" className="btn" onClick={handleDownloadTemplate}>
            <Icon name="download" size={12} /> Descarcă template CSV
          </button>
          <span style={{ fontSize: 10.5, color: "var(--text-muted)", marginLeft: 10 }}>
            Folosiți template-ul ca punct de pornire.
          </span>
        </div>

        {/* File picker */}
        <div style={{ marginBottom: 12 }}>
          <input
            ref={fileRef}
            type="file"
            accept=".csv,.txt"
            style={{ display: "none" }}
            onChange={handleFileChange}
          />
          <button
            type="button"
            className="btn"
            onClick={() => fileRef.current?.click()}
          >
            <Icon name="upload" size={12} /> Selectează fișier CSV
          </button>
          {fileName && (
            <span style={{ fontSize: 11, marginLeft: 10, color: "var(--text-muted)" }}>
              {fileName}
            </span>
          )}
        </div>

        {/* Preview */}
        {preview.length > 0 && (
          <div style={{ marginBottom: 12 }}>
            <div style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)", marginBottom: 4 }}>
              Preview (primele {preview.length} linii):
            </div>
            <div
              style={{
                background: "var(--bg)",
                border: "1px solid var(--border)",
                padding: "6px 10px",
                fontFamily: "var(--font-mono)",
                fontSize: 10.5,
                overflowX: "auto",
                maxHeight: 120,
                overflowY: "auto",
              }}
            >
              {preview.map((line, i) => (
                <div key={i}>{line}</div>
              ))}
            </div>
          </div>
        )}

        {/* Dry-run validation */}
        {content && !result && (
          <div style={{ marginBottom: 10 }}>
            <button
              type="button"
              className="btn"
              disabled={previewing}
              onClick={() => void handleDryRun()}
            >
              {previewing ? "Se validează…" : "Validează înainte de import →"}
            </button>
          </div>
        )}
        {previewResult && !result && (
          <div
            style={{
              padding: "8px 10px",
              background: "var(--bg)",
              border: "1px solid var(--border)",
              fontSize: 11,
              marginBottom: 10,
            }}
          >
            <div style={{ fontWeight: 600, marginBottom: 4 }}>
              Previzualizare: {previewResult.imported} înregistrări valide
            </div>
            {previewResult.errors.length > 0 && (
              <div style={{ color: "#DC2626", marginTop: 4 }}>
                {previewResult.errors.slice(0, 5).map((e, i) => (
                  <div key={i}>• {e}</div>
                ))}
                {previewResult.errors.length > 5 && (
                  <div>…și încă {previewResult.errors.length - 5} erori.</div>
                )}
              </div>
            )}
          </div>
        )}

        {/* Result */}
        {result && (
          <div
            style={{
              padding: "8px 12px",
              background: result.errors.length === 0 ? "#F0FDF4" : "#FFF7ED",
              border: `1px solid ${result.errors.length === 0 ? "#BBF7D0" : "#FED7AA"}`,
              fontSize: 11,
              marginBottom: 12,
              color: result.errors.length === 0 ? "#15803D" : "#C2410C",
            }}
          >
            <b>{result.imported} înregistrări importate.</b>
            {result.errors.length > 0 && (
              <div style={{ marginTop: 4 }}>
                {result.errors.slice(0, 5).map((e, i) => (
                  <div key={i}>{e}</div>
                ))}
                {result.errors.length > 5 && (
                  <div>…și {result.errors.length - 5} erori suplimentare.</div>
                )}
              </div>
            )}
          </div>
        )}

        {/* Error */}
        {error && (
          <div
            style={{
              padding: "6px 10px",
              background: "#FEE2E2",
              border: "1px solid #FECACA",
              fontSize: 11,
              color: "#991B1B",
              marginBottom: 12,
            }}
          >
            {error}
          </div>
        )}

        {/* Actions */}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button type="button" className="btn" onClick={onClose}>
            Închide
          </button>
          <button
            type="button"
            className="btn primary"
            disabled={!content || importing}
            onClick={handleImport}
          >
            {importing ? "Se importă…" : "Importă"}
          </button>
        </div>
      </div>
    </div>
  );
}
