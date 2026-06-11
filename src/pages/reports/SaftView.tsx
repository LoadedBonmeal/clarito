/**
 * SaftView — D406 SAF-T export panel (embedded in Rapoarte).
 * Claude-Design classes: .scr-card + .scr-toolbar .tt + .banner + .pill-btn/.btn-dark.
 * ALL wiring preserved: declarations.preflight, saft.exportD406 (preview),
 * saft.exportSaftOfficial (+ DUK override), PreflightPanel.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { api } from "@/lib/tauri";
import type { PreflightIssue } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { MONTHS_RO } from "@/lib/utils";

// SaftView uses legacy export_saft_d406 (returns XML string) + new export_saft_official (writes file, returns path).

const MONTHS = MONTHS_RO;

// Info icon absent from the Ic set — inlined verbatim (design banner pattern).
const SVG_INFO_CIRCLE = '<path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>';

interface Props {
  selectedYear:       number;
  selectedMonth:      number;
  allInvoicesForYear: { issueDate: string }[];
}

export function SaftView({ selectedYear, selectedMonth, allInvoicesForYear }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting,         setExporting]         = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);
  const [dukBlock,          setDukBlock]          = useState<PreflightIssue[] | null>(null);

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

  const handleExportOfficial = async (override = false) => {
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
      const res = await api.saft.exportSaftOfficial(
        activeCompanyId,
        selectedYear,
        selectedMonth,
        savePath,
        override,
      );
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error("DUKIntegrator a găsit erori. Corectați-le sau exportați oricum.");
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? `D406 oficial salvat (DUK: valid): ${res.path}`
          : `D406 oficial salvat: ${res.path} (validare DUK indisponibilă local)`,
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D406 oficial."));
    } finally {
      setExportingOfficial(false);
    }
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", gap: 16, alignItems: "start" }}>
      {/* Info card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">D406 — SAF-T (Standard Audit File for Tax)</div>
        </div>
        <div className="card-pad">
          <p style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.6, margin: "0 0 12px" }}>
            Fișierul standard de audit fiscal (SAF-T) conține datele contabile detaliate solicitate
            de ANAF: conturi, jurnale, facturi, stocuri și active. Începând cu 2025, depunerea D406
            este obligatorie lunar pentru contribuabilii mijlocii și mari.
          </p>
          <div className="banner" style={{ marginBottom: 0 }}>
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_INFO_CIRCLE }} />
            <span>
              Pentru companiile mici, termenul de depunere D406 a fost amânat. Verificați obligația
              specifică firmei dvs.
            </span>
          </div>
          <div style={{ marginTop: 16, fontSize: 12.5, color: "var(--text-2)" }}>
            Perioadă selectată: <b style={{ color: "var(--text)" }}>{monthName} {selectedYear}</b>
            {allInvoicesForYear.length > 0
              ? ` · ${allInvoicesForYear.length} facturi disponibile pentru ${selectedYear}`
              : ` · nicio factură disponibilă pentru ${selectedYear}`}
          </div>
        </div>
      </div>

      {/* Export card */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">Generează SAF-T</div>
        </div>
        <div className="card-pad" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          {/* Preflight validation panel */}
          <PreflightPanel issues={preflightIssues} />

          {/* DUK block panel */}
          {dukBlock && (
            <div>
              <PreflightPanel issues={dukBlock} />
              <button
                className="pill-btn"
                style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.35)" }}
                onClick={() => void handleExportOfficial(true)}
              >
                Exportă oricum (ignoră DUK)
              </button>
            </div>
          )}

          <div style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.5 }}>
            Exportă SAF-T D406 pentru <b style={{ color: "var(--text)" }}>{monthName} {selectedYear}</b>.
          </div>
          {/* Legacy preview */}
          <button
            className="pill-btn"
            style={{ width: "100%", justifyContent: "center" }}
            disabled={exporting || !activeCompanyId}
            onClick={() => void handleExport()}
            title={`SAF-T D406 preview (facturi emise) pentru ${monthName} ${selectedYear}`}
          >
            <Ic name="code" />
            {exporting ? "Export în curs…" : `Extract SAF-T (preview) ${monthName} ${selectedYear}`}
          </button>
          {/* Official D406 */}
          <button
            className="btn-dark"
            style={{ width: "100%", justifyContent: "center", opacity: exportingOfficial || !activeCompanyId ? 0.6 : 1 }}
            disabled={exportingOfficial || !activeCompanyId}
            onClick={() => void handleExportOfficial()}
            title={`Export D406 oficial ANAF (schema completă + GL) pentru ${monthName} ${selectedYear}`}
          >
            <Ic name="shield" />
            {exportingOfficial ? "Export D406 în curs…" : `Export oficial D406 ${monthName} ${selectedYear}`}
          </button>
        </div>
      </div>
    </div>
  );
}
