/**
 * PdfViewerModal — in-app PDF viewer rendered in the design's .modal-back/.modal
 * chrome. Powered by EmbedPDF (PDFium-via-WASM, the same engine as Chrome).
 *
 * Desktop-tuned:
 *   - wasmUrl points at the LOCALLY BUNDLED pdfium.wasm (Vite ?url asset) — no
 *     CDN dependency, works fully offline. CSP allows it via 'wasm-unsafe-eval'
 *     (compile) + connect-src 'self' (fetch) + img-src blob: (rendered pages).
 *   - worker:false → runs on the main thread, so no worker-src CSP surface.
 *   - fontFallback:null → never reaches out to the network for fallback fonts.
 *
 * The document is loaded from in-memory bytes (openDocumentBuffer), supplied by
 * useOpenPdf via the pdf-viewer store — so it works the same whether the bytes
 * came off disk (real app) or from the bundled sample (demo harness).
 */
import { useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { createPluginRegistration } from "@embedpdf/core";
import { EmbedPDF } from "@embedpdf/core/react";
import { usePdfiumEngine } from "@embedpdf/engines/react";
import { Viewport, ViewportPluginPackage } from "@embedpdf/plugin-viewport/react";
import { Scroller, ScrollPluginPackage } from "@embedpdf/plugin-scroll/react";
import {
  DocumentContent,
  DocumentManagerPluginPackage,
  useActiveDocument,
  useDocumentManagerCapability,
} from "@embedpdf/plugin-document-manager/react";
import { RenderLayer, RenderPluginPackage } from "@embedpdf/plugin-render/react";
import { ZoomMode, ZoomPluginPackage, useZoomCapability } from "@embedpdf/plugin-zoom/react";
// Locally bundled PDFium WASM — Vite copies it into the build and serves it
// same-origin (no CDN). The export is "@embedpdf/pdfium/pdfium.wasm".
import pdfiumWasmUrl from "@embedpdf/pdfium/pdfium.wasm?url";

import { Ic } from "@/components/shared/Ic";
import { usePdfViewerStore, type PdfViewerPayload } from "@/lib/pdf-viewer-store";
import { isDemoMode } from "@/lib/demo";
import { notify } from "@/lib/toasts";

const plugins = [
  createPluginRegistration(DocumentManagerPluginPackage),
  createPluginRegistration(ViewportPluginPackage),
  createPluginRegistration(ScrollPluginPackage),
  createPluginRegistration(RenderPluginPackage),
  createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitWidth }),
];

export function PdfViewerModal() {
  const payload = usePdfViewerStore((s) => s.payload);
  const close = usePdfViewerStore((s) => s.close);

  // Escape closes — registered only while open.
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
    <div className="modal-back show" onMouseDown={close}>
      <div
        className="modal pdfv"
        role="dialog"
        aria-modal="true"
        aria-label={payload.name}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <ViewerEngine payload={payload} onClose={close} />
      </div>
    </div>,
    document.body,
  );
}

/** Boots the PDFium engine, then mounts the EmbedPDF provider once ready. */
function ViewerEngine({ payload, onClose }: { payload: PdfViewerPayload; onClose: () => void }) {
  const { t } = useTranslation();
  const { engine, isLoading, error } = usePdfiumEngine({
    wasmUrl: pdfiumWasmUrl,
    worker: false,
    fontFallback: null,
  });

  if (error) {
    return (
      <>
        <Header name={payload.name} payload={payload} onClose={onClose} />
        <div className="pdfv-body pdfv-status pdfv-status--err">
          {t("shared.pdfViewer.engineError")}
        </div>
      </>
    );
  }

  if (isLoading || !engine) {
    return (
      <>
        <Header name={payload.name} payload={payload} onClose={onClose} />
        <div className="pdfv-body pdfv-status">{t("shared.pdfViewer.loading")}</div>
      </>
    );
  }

  return (
    <EmbedPDF engine={engine} plugins={plugins}>
      {() => <ViewerShell payload={payload} onClose={onClose} />}
    </EmbedPDF>
  );
}

/** Inside the EmbedPDF provider: loads the buffer, renders toolbar + pages. */
function ViewerShell({ payload, onClose }: { payload: PdfViewerPayload; onClose: () => void }) {
  const { t } = useTranslation();
  const dm = useDocumentManagerCapability();
  const { activeDocumentId } = useActiveDocument();

  // Load the PDF bytes exactly once when the manager capability is ready.
  useEffect(() => {
    const cap = dm.provides;
    if (!cap) return;
    // .slice() yields a tight ArrayBuffer (exact byteLength) regardless of how
    // the source Uint8Array was created.
    const buffer = payload.bytes.slice().buffer;
    const task = cap.openDocumentBuffer({ buffer, name: payload.name, autoActivate: true });
    void task.toPromise().catch(() => {
      /* surfaced by the empty-state below */
    });
  }, [dm.provides, payload]);

  return (
    <>
      <Header name={payload.name} payload={payload} onClose={onClose} withZoom />
      <div className="pdfv-body">
        {activeDocumentId ? (
          <DocumentContent documentId={activeDocumentId}>
            {({ isLoaded }) =>
              isLoaded ? (
                <Viewport documentId={activeDocumentId} className="pdfv-viewport">
                  <Scroller
                    documentId={activeDocumentId}
                    renderPage={({ width, height, pageIndex }) => (
                      <div className="pdfv-page" style={{ width, height }}>
                        <RenderLayer documentId={activeDocumentId} pageIndex={pageIndex} />
                      </div>
                    )}
                  />
                </Viewport>
              ) : (
                <div className="pdfv-status">{t("shared.pdfViewer.loading")}</div>
              )
            }
          </DocumentContent>
        ) : (
          <div className="pdfv-status">{t("shared.pdfViewer.loading")}</div>
        )}
      </div>
    </>
  );
}

/** Toolbar — title, optional zoom controls, save/open-external, close. */
function Header({
  name,
  payload,
  onClose,
  withZoom = false,
}: {
  name: string;
  payload: PdfViewerPayload;
  onClose: () => void;
  withZoom?: boolean;
}) {
  const { t } = useTranslation();

  async function handleSave() {
    try {
      if (isDemoMode()) {
        const blob = new Blob([payload.bytes.slice().buffer], { type: "application/pdf" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = name.endsWith(".pdf") ? name : `${name}.pdf`;
        a.click();
        URL.revokeObjectURL(url);
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: name.endsWith(".pdf") ? name : `${name}.pdf`,
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (!dest) return;
      const { writeFile } = await import("@tauri-apps/plugin-fs");
      await writeFile(dest, payload.bytes);
      notify.success(t("shared.pdfViewer.saved"));
    } catch (e) {
      notify.error(t("shared.pdfViewer.saveError", { error: String(e) }));
    }
  }

  async function handleOpenExternal() {
    if (!payload.externalPath) return;
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(payload.externalPath);
    } catch (e) {
      notify.error(t("shared.pdfViewer.openExternalError", { error: String(e) }));
    }
  }

  return (
    <header className="pdfv-bar">
      <div className="pdfv-title">
        <Ic name="docText" cls="ic" />
        <span>{name}</span>
      </div>
      <div className="pdfv-actions">
        {withZoom && <ZoomControls />}
        {payload.externalPath && !isDemoMode() && (
          <button type="button" className="pill-btn" onClick={handleOpenExternal}>
            <Ic name="eye" cls="ic" />
            {t("shared.pdfViewer.openExternal")}
          </button>
        )}
        <button type="button" className="pill-btn" onClick={handleSave}>
          <Ic name="dl" cls="ic" />
          {t("shared.pdfViewer.save")}
        </button>
        <button
          type="button"
          className="sq-btn"
          onClick={onClose}
          aria-label={t("shared.pdfViewer.close")}
        >
          <Ic name="xMark" cls="ic" />
        </button>
      </div>
    </header>
  );
}

/** Zoom −/+ — must live inside the EmbedPDF provider (uses the zoom plugin). */
function ZoomControls() {
  const { t } = useTranslation();
  const zoom = useZoomCapability();
  return (
    <div className="pdfv-zoom">
      <button
        type="button"
        className="sq-btn"
        onClick={() => zoom.provides?.zoomOut()}
        aria-label={t("shared.pdfViewer.zoomOut")}
      >
        <Ic name="minus" cls="ic" />
      </button>
      <button
        type="button"
        className="sq-btn"
        onClick={() => zoom.provides?.zoomIn()}
        aria-label={t("shared.pdfViewer.zoomIn")}
      >
        <Ic name="plus" cls="ic" />
      </button>
    </div>
  );
}
