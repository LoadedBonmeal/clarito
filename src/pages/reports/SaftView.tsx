/**
 * SaftView — D406 SAF-T export panel.
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

interface Props {
  selectedYear: number;
  allInvoicesForYear: { issueDate: string }[];
}

export function SaftView({ selectedYear, allInvoicesForYear }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (allInvoicesForYear.length === 0) {
      notify.info(`Nu există date pentru anul ${selectedYear}.`);
      return;
    }
    const savePath = await saveDialog({
      title: "Salvează SAF-T D406",
      defaultPath: `saft-d406-${selectedYear}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const xml = await api.saft.exportD406(activeCompanyId, selectedYear, undefined);
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      await writeTextFile(savePath, xml);
      notify.success(`SAF-T D406 salvat: ${savePath}`);
      try { await openPath(savePath); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta SAF-T D406."));
    } finally {
      setExporting(false);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <h2 style={{ fontSize: 12, fontWeight: 600, color: "var(--text)", letterSpacing: "0.04em", textTransform: "uppercase", margin: 0 }}>
          D406 — SAF-T (Standard Audit File for Tax)
        </h2>
        <button
          type="button"
          className="btn"
          disabled={exporting || !activeCompanyId}
          onClick={handleExport}
          title={`SAF-T D406 — standard ANAF de audit fiscal pentru ${selectedYear}`}
        >
          <Icon name="file" size={12} /> {exporting ? "Export…" : `Exportă SAF-T D406 (XML) ${selectedYear}`}
        </button>
      </div>

      <div
        style={{
          border: "1px solid var(--border)",
          background: "var(--bg)",
          padding: "16px 18px",
          maxWidth: 580,
        }}
      >
        <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: "var(--text)" }}>
          Despre D406 SAF-T
        </div>
        <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.6, margin: "0 0 8px" }}>
          SAF-T (Standard Audit File for Tax) este standardul ANAF de audit fiscal electronic
          introdus prin OPANAF 1056/2021. Fișierul D406 conține informații structurate despre
          operațiunile economice ale companiei și se depune la solicitarea ANAF.
        </p>
        <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.6, margin: "0 0 8px" }}>
          Această versiune acoperă <strong>jurnalul de vânzări</strong> (facturi emise) pentru
          anul selectat — <strong>versiune beta</strong>. Selectorul de an se află deasupra, în bara de perioadă.
        </p>
        <div style={{ fontSize: 11, color: "var(--text-muted)", borderTop: "1px solid var(--border)", paddingTop: 8, marginTop: 8 }}>
          An curent selectat: <strong>{selectedYear}</strong>
          {allInvoicesForYear.length > 0
            ? ` · ${allInvoicesForYear.length} facturi disponibile`
            : " · nicio factură disponibilă pentru acest an"}
        </div>
      </div>
    </div>
  );
}
