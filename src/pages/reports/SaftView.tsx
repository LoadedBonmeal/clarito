/**
 * SaftView — D406 SAF-T export panel.
 * Wave 5 — rf look: SectionCard + Banner + Btn
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn, Banner } from "@/components/rf";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";

// SaftView uses legacy export_saft_d406 (returns XML string) + new export_saft_official (writes file, returns path).

const MONTHS = [
  "Ianuarie", "Februarie", "Martie", "Aprilie", "Mai", "Iunie",
  "Iulie", "August", "Septembrie", "Octombrie", "Noiembrie", "Decembrie",
];

interface Props {
  selectedYear:       number;
  selectedMonth:      number;
  allInvoicesForYear: { issueDate: string }[];
}

export function SaftView({ selectedYear, selectedMonth, allInvoicesForYear }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting,         setExporting]         = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);

  const monthName = MONTHS[selectedMonth - 1] ?? String(selectedMonth);

  // Compute period strings for preflight (first→last day of selected month).
  const mm = String(selectedMonth).padStart(2, "0");
  const lastDay = new Date(selectedYear, selectedMonth, 0).getDate();
  const periodFrom = `${selectedYear}-${mm}-01`;
  const periodTo   = `${selectedYear}-${mm}-${String(lastDay).padStart(2, "0")}`;

  // ── Pre-export validation (preflight) ──────────────────────────────────────
  const { data: preflightIssues = [] } = useQuery({
    queryKey: ["preflight", "d406", activeCompanyId ?? "", periodFrom, periodTo],
    queryFn: () => api.declarations.preflight(activeCompanyId!, "D406", periodFrom, periodTo),
    enabled: !!activeCompanyId,
    staleTime: 30_000,
  });

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (allInvoicesForYear.length === 0) {
      notify.info(`Nu există date pentru anul ${selectedYear}.`);
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează SAF-T D406",
      defaultPath: `saft-d406-${selectedYear}-${mm}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      // D406 is a monthly declaration — always pass the selected month.
      const xml = await api.saft.exportD406(activeCompanyId, selectedYear, selectedMonth);
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

  const handleExportOfficial = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const savePath = await saveDialog({
      title:       "Salvează D406 oficial ANAF",
      defaultPath: `d406-oficial-${selectedYear}-${mm}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExportingOfficial(true);
    try {
      // export_saft_official takes params wrapper: { companyId, year, month, destPath }
      const saved = await api.saft.exportSaftOfficial(
        activeCompanyId,
        selectedYear,
        selectedMonth,
        savePath,
      );
      notify.success(`D406 oficial salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D406 oficial."));
    } finally {
      setExportingOfficial(false);
    }
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", gap: 20, alignItems: "start" }}>
      {/* Info card */}
      <SectionCard icon="declaration" title="D406 — SAF-T (Standard Audit File for Tax)">
        <div style={{ padding: "4px 16px 16px" }}>
          <p style={{ fontSize: 13, color: "var(--rf-text-muted)", lineHeight: 1.6, margin: "0 0 12px" }}>
            Fișierul standard de audit fiscal (SAF-T) conține datele contabile detaliate solicitate
            de ANAF: conturi, jurnale, facturi, stocuri și active. Începând cu 2025, depunerea D406
            este obligatorie lunar pentru contribuabilii mijlocii și mari.
          </p>
          <Banner variant="info">
            Pentru companiile mici, termenul de depunere D406 a fost amânat. Verificați obligația
            specifică firmei dvs.
          </Banner>
          <div style={{ marginTop: 16, fontSize: 12.5, color: "var(--rf-text-muted)" }}>
            Perioadă selectată: <b style={{ color: "var(--rf-text)" }}>{monthName} {selectedYear}</b>
            {allInvoicesForYear.length > 0
              ? ` · ${allInvoicesForYear.length} facturi disponibile pentru ${selectedYear}`
              : ` · nicio factură disponibilă pentru ${selectedYear}`}
          </div>
        </div>
      </SectionCard>

      {/* Export card */}
      <SectionCard icon="download" title="Generează SAF-T">
        <div style={{ padding: "4px 16px 16px", display: "flex", flexDirection: "column", gap: 16 }}>
          {/* Preflight validation panel */}
          <PreflightPanel issues={preflightIssues} />

          <div style={{ fontSize: 13, color: "var(--rf-text-muted)", lineHeight: 1.5 }}>
            Exportă SAF-T D406 pentru <b>{monthName} {selectedYear}</b>.
          </div>
          {/* Legacy preview */}
          <Btn
            variant="secondary"
            icon="xml"
            block
            disabled={exporting || !activeCompanyId}
            onClick={() => void handleExport()}
            title={`SAF-T D406 preview (facturi emise) pentru ${monthName} ${selectedYear}`}
          >
            {exporting ? "Export în curs…" : `Extract SAF-T (preview) ${monthName} ${selectedYear}`}
          </Btn>
          {/* Official D406 */}
          <Btn
            variant="primary"
            icon="anaf"
            block
            disabled={exportingOfficial || !activeCompanyId}
            onClick={() => void handleExportOfficial()}
            title={`Export D406 oficial ANAF (schema completă + GL) pentru ${monthName} ${selectedYear}`}
          >
            {exportingOfficial ? "Export D406 în curs…" : `Export oficial D406 ${monthName} ${selectedYear}`}
          </Btn>
        </div>
      </SectionCard>
    </div>
  );
}
