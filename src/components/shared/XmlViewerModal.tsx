/**
 * XmlViewerModal — in-app, read-only viewer for the declaration / e-Factura XML the app generates,
 * in the design's .modal-back/.modal chrome. Renders the XML as a clean, human-labeled DOCUMENT
 * (XmlDocView) — titled sections + real Romanian labels (a UBL invoice looks like the ANAF
 * visualizer; declarations show a declarant block + labeled tables), never as raw code.
 *
 * Actions: print / save as PDF (the same labeled document) · export the data as an XLSX table ·
 * save the byte-clean submission .xml · copy the XML · "re-validate with DUK" for declaration XML
 * (D300/D394/D406/D112/D205) via the bundled ANAF validators. Fed from the store (useOpenXml).
 */
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { XmlDocView } from "@/components/shared/XmlDocView";
import { xmlToTables } from "@/lib/xml-to-tables";
import { useXmlViewerStore, type XmlViewerPayload } from "@/lib/xml-viewer-store";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api, type XmlDukValidation } from "@/lib/tauri";
import { isDemoMode } from "@/lib/demo";
import { notify } from "@/lib/toasts";
import { buildStandaloneHtml } from "@/lib/doc-render/doc-html";

export function XmlViewerModal() {
  const payload = useXmlViewerStore((s) => s.payload);
  const storeClose = useXmlViewerStore((s) => s.close);
  const { closing, close } = useAnimatedClose(storeClose);

  useEffect(() => {
    if (!payload) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [payload, close]);

  if (!payload) return null;

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="modal xmlv" role="dialog" aria-modal="true" aria-label={payload.name}>
        <XmlViewerBody payload={payload} onClose={close} />
      </div>
    </div>,
    document.body,
  );
}

function XmlViewerBody({ payload, onClose }: { payload: XmlViewerPayload; onClose: () => void }) {
  const { t } = useTranslation();
  const [validating, setValidating] = useState(false);
  const [validation, setValidation] = useState<XmlDukValidation | null>(null);

  const fileName = payload.name.endsWith(".xml") ? payload.name : `${payload.name}.xml`;

  const doCopy = async () => {
    try {
      await navigator.clipboard.writeText(payload.xml);
      notify.success(t("shared.xmlViewer.copied"));
    } catch (e) {
      notify.error(t("shared.xmlViewer.copyError", { error: String(e) }));
    }
  };

  const doSave = async () => {
    try {
      if (isDemoMode()) {
        const url = URL.createObjectURL(new Blob([payload.xml], { type: "application/xml" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = fileName;
        a.click();
        URL.revokeObjectURL(url);
      } else {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const dest = await save({ defaultPath: fileName, filters: [{ name: "XML", extensions: ["xml"] }] });
        if (!dest) return;
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        await writeTextFile(dest, payload.xml);
      }
      notify.success(t("shared.xmlViewer.saved"));
    } catch (e) {
      notify.error(t("shared.xmlViewer.saveError", { error: String(e) }));
    }
  };

  // Export a clean XLSX TABLE of the same declaration (opens as a real grid in Excel/Numbers). The
  // canonical .xml (Salvează) stays untouched for ANAF submission.
  const doExportXlsx = async () => {
    if (isDemoMode()) {
      notify.error(t("shared.xmlViewer.tableNativeOnly"));
      return;
    }
    try {
      const tables = xmlToTables(payload.xml);
      if (tables.length === 0) {
        notify.error(t("shared.xmlViewer.tableError", { error: "XML invalid" }));
        return;
      }
      const xlsxName = `${fileName.replace(/\.xml$/i, "")}.xlsx`;
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({ defaultPath: xlsxName, filters: [{ name: "Excel", extensions: ["xlsx"] }] });
      if (!dest) return;
      const written = await api.declarations.exportDeclarationXlsx(tables, dest);
      notify.success(t("shared.xmlViewer.tableSaved"));
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(written).catch(() => {});
    } catch (e) {
      notify.error(t("shared.xmlViewer.tableError", { error: String(e) }));
    }
  };

  // Print / Save the labeled document as PDF. Tauri's WKWebView can't window.print(), so we wrap the
  // rendered `.docv` document in a self-contained HTML file and open it in the default BROWSER, where
  // it auto-prints and "Save as PDF" works. Demo (browser) mode opens a new tab directly.
  const doPrint = async () => {
    const el = document.querySelector(".docv");
    if (!el) {
      notify.error(t("shared.xmlViewer.printError", { error: "—" }));
      return;
    }
    const html = buildStandaloneHtml(payload.name, el.outerHTML);
    if (isDemoMode()) {
      const w = window.open("", "_blank");
      if (w) {
        w.document.write(html);
        w.document.close();
      }
      return;
    }
    // The Rust command writes the HTML to the app cache dir and opens it in the default browser
    // (opening from Rust bypasses the JS opener's dialog-only path scope).
    try {
      await api.declarations.openDocInBrowser(html, `${payload.name.replace(/\.xml$/i, "")}.html`);
      notify.success(t("shared.xmlViewer.printOpened"));
    } catch (e) {
      notify.error(t("shared.xmlViewer.printError", { error: String(e) }));
    }
  };

  const doRevalidate = async () => {
    if (!payload.declKind) return;
    setValidating(true);
    setValidation(null);
    try {
      const res = await api.declarations.validateDeclarationXml(payload.declKind, payload.xml);
      setValidation(res);
    } catch (e) {
      notify.error(t("shared.xmlViewer.validateError", { error: String(e) }));
    } finally {
      setValidating(false);
    }
  };

  return (
    <>
      <header className="pdfv-bar">
        <div className="pdfv-bar-l">
          <div className="pdfv-title">
            <Ic name="docText" cls="ic" />
            <span>{payload.name}</span>
          </div>
        </div>

        <div className="pdfv-bar-r">
          {payload.declKind && (
            <button type="button" className="pill-btn" onClick={doRevalidate} disabled={validating}>
              <Ic name="shield" cls="ic" />
              {validating ? t("shared.xmlViewer.validating") : t("shared.xmlViewer.revalidate")}
            </button>
          )}
          <button type="button" className="sq-btn" onClick={doCopy} aria-label={t("shared.xmlViewer.copy")}>
            <Ic name="copy" cls="ic" />
          </button>
          <button type="button" className="pill-btn" onClick={doPrint}>
            <Ic name="printer" cls="ic" />
            {t("shared.xmlViewer.printPdf")}
          </button>
          <button type="button" className="pill-btn" onClick={doExportXlsx}>
            <Ic name="grid" cls="ic" />
            {t("shared.xmlViewer.exportTable")}
          </button>
          <button type="button" className="pill-btn" onClick={doSave}>
            <Ic name="dl" cls="ic" />
            {t("shared.xmlViewer.save")}
          </button>
          <button type="button" className="sq-btn" onClick={onClose} aria-label={t("shared.xmlViewer.close")}>
            <Ic name="xMark" cls="ic" />
          </button>
        </div>
      </header>

      {validation && <ValidationStrip declKind={payload.declKind} result={validation} />}

      <XmlDocView payload={payload} />
    </>
  );
}

function ValidationStrip({ declKind, result }: { declKind?: string; result: XmlDukValidation }) {
  const { t } = useTranslation();
  if (!result.available) {
    return <div className="xmlv-valid xmlv-valid--na">{t("shared.xmlViewer.validatorUnavailable")}</div>;
  }
  if (result.passed) {
    return (
      <div className="xmlv-valid xmlv-valid--ok">
        <Ic name="checkC" cls="ic" />
        {t("shared.xmlViewer.validOk", { kind: declKind ?? "" })}
      </div>
    );
  }
  return (
    <div className="xmlv-valid xmlv-valid--err">
      <div className="xmlv-valid-head">
        <Ic name="triangle" cls="ic" />
        {t("shared.xmlViewer.validErrors", { count: result.issues.length })}
      </div>
      <ul className="xmlv-issues">
        {result.issues.slice(0, 30).map((iss, i) => (
          <li key={i} className={`xmlv-issue xmlv-issue--${iss.severity}`}>
            <span className="xmlv-issue-code">{iss.code}</span>
            {iss.message}
          </li>
        ))}
      </ul>
    </div>
  );
}
