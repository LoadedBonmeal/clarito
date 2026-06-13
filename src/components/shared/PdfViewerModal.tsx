/**
 * PdfViewerModal — in-app PDF viewer in the design's .modal-back/.modal chrome,
 * powered by EmbedPDF (PDFium-via-WASM). Light monochrome, matching the app.
 *
 * Full viewer chrome (all via headless EmbedPDF plugins — no shadcn):
 *   thumbnail rail · page nav (‹ Page X of N ›) · rotate L/R · zoom −/%/+ ·
 *   find-in-document · save-as · open-externally · close.
 *
 * Desktop-tuned engine:
 *   - wasmUrl → LOCALLY BUNDLED pdfium.wasm (Vite ?url) — offline, no CDN.
 *   - worker:false → main thread (no worker-src CSP surface).
 *   - fontFallback:null → never fetches fallback fonts over the network.
 * CSP allows it via 'wasm-unsafe-eval' (compile) + connect-src 'self' (wasm
 * fetch) + img-src blob: (rendered page/thumbnail images).
 *
 * Documents load from in-memory bytes (openDocumentBuffer) supplied by
 * useOpenPdf — identical path for the real app (plugin-fs) and the demo harness.
 */
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { createPluginRegistration } from "@embedpdf/core";
import { EmbedPDF } from "@embedpdf/core/react";
import { usePdfiumEngine } from "@embedpdf/engines/react";
import { Viewport, ViewportPluginPackage } from "@embedpdf/plugin-viewport/react";
import { Scroller, ScrollPluginPackage, useScroll } from "@embedpdf/plugin-scroll/react";
import {
  DocumentContent,
  DocumentManagerPluginPackage,
  useActiveDocument,
  useDocumentManagerCapability,
} from "@embedpdf/plugin-document-manager/react";
import { RenderLayer, RenderPluginPackage } from "@embedpdf/plugin-render/react";
import { ZoomMode, ZoomPluginPackage, useZoom } from "@embedpdf/plugin-zoom/react";
import { ThumbnailPluginPackage, ThumbnailsPane, ThumbImg } from "@embedpdf/plugin-thumbnail/react";
import { RotatePluginPackage, useRotate } from "@embedpdf/plugin-rotate/react";
import { SearchLayer, SearchPluginPackage, useSearch } from "@embedpdf/plugin-search/react";
// Locally bundled PDFium WASM — Vite serves it same-origin (no CDN).
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
  createPluginRegistration(ThumbnailPluginPackage),
  createPluginRegistration(RotatePluginPackage),
  createPluginRegistration(SearchPluginPackage),
];

const ZOOM_PRESETS = [50, 75, 100, 125, 150, 200];

export function PdfViewerModal() {
  const payload = usePdfViewerStore((s) => s.payload);
  const close = usePdfViewerStore((s) => s.close);

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
        <MiniBar name={payload.name} onClose={onClose} />
        <div className="pdfv-body pdfv-status pdfv-status--err">{t("shared.pdfViewer.engineError")}</div>
      </>
    );
  }
  if (isLoading || !engine) {
    return (
      <>
        <MiniBar name={payload.name} onClose={onClose} />
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

/** Minimal bar shown during engine boot / on error (no document yet). */
function MiniBar({ name, onClose }: { name: string; onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <header className="pdfv-bar">
      <div className="pdfv-bar-l">
        <div className="pdfv-title">
          <Ic name="docText" cls="ic" />
          <span>{name}</span>
        </div>
      </div>
      <div className="pdfv-bar-r">
        <button type="button" className="sq-btn" onClick={onClose} aria-label={t("shared.pdfViewer.close")}>
          <Ic name="xMark" cls="ic" />
        </button>
      </div>
    </header>
  );
}

/** Inside the EmbedPDF provider: loads the buffer, renders the full chrome. */
function ViewerShell({ payload, onClose }: { payload: PdfViewerPayload; onClose: () => void }) {
  const { t } = useTranslation();
  const dm = useDocumentManagerCapability();
  const { activeDocumentId } = useActiveDocument();
  const [showThumbs, setShowThumbs] = useState(true);
  const [showSearch, setShowSearch] = useState(false);

  useEffect(() => {
    const cap = dm.provides;
    if (!cap) return;
    const buffer = payload.bytes.slice().buffer;
    const task = cap.openDocumentBuffer({ buffer, name: payload.name, autoActivate: true });
    void task.toPromise().catch(() => {});
  }, [dm.provides, payload]);

  return (
    <>
      <header className="pdfv-bar">
        <div className="pdfv-bar-l">
          <button
            type="button"
            className={`sq-btn${showThumbs ? " is-on" : ""}`}
            onClick={() => setShowThumbs((v) => !v)}
            aria-label={t("shared.pdfViewer.thumbnails")}
            aria-pressed={showThumbs}
          >
            <Ic name="collapse" cls="ic" />
          </button>
          <div className="pdfv-title">
            <Ic name="docText" cls="ic" />
            <span>{payload.name}</span>
          </div>
        </div>

        {activeDocumentId && (
          <div className="pdfv-bar-c">
            <PageNav documentId={activeDocumentId} />
            <span className="pdfv-sep" />
            <RotateControls documentId={activeDocumentId} />
            <span className="pdfv-sep" />
            <ZoomControls documentId={activeDocumentId} />
            <button
              type="button"
              className={`sq-btn${showSearch ? " is-on" : ""}`}
              onClick={() => setShowSearch((v) => !v)}
              aria-label={t("shared.pdfViewer.search")}
              aria-pressed={showSearch}
            >
              <Ic name="lens" cls="ic" />
            </button>
          </div>
        )}

        <div className="pdfv-bar-r">
          <SaveButton name={payload.name} bytes={payload.bytes} />
          {payload.externalPath && !isDemoMode() && (
            <OpenExternalButton path={payload.externalPath} />
          )}
          <button type="button" className="sq-btn" onClick={onClose} aria-label={t("shared.pdfViewer.close")}>
            <Ic name="xMark" cls="ic" />
          </button>
        </div>
      </header>

      {activeDocumentId && showSearch && (
        <SearchBar documentId={activeDocumentId} onClose={() => setShowSearch(false)} />
      )}

      <div className="pdfv-main">
        {activeDocumentId && showThumbs && (
          <aside className="pdfv-thumbs">
            <ThumbSidebar documentId={activeDocumentId} />
          </aside>
        )}
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
                          <SearchLayer documentId={activeDocumentId} pageIndex={pageIndex} />
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
      </div>
    </>
  );
}

// ── Toolbar controls (each inside the EmbedPDF provider) ─────────────────────

function PageNav({ documentId }: { documentId: string }) {
  const { t } = useTranslation();
  const { state, provides } = useScroll(documentId);
  const cur = state?.currentPage ?? 1;
  const total = state?.totalPages ?? 1;
  return (
    <div className="pdfv-pagenav">
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.scrollToPreviousPage()}
        disabled={cur <= 1}
        aria-label={t("shared.pdfViewer.prevPage")}
      >
        <Ic name="chevL" cls="ic" />
      </button>
      <span className="pdfv-pageind">{t("shared.pdfViewer.pageOf", { cur, total })}</span>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.scrollToNextPage()}
        disabled={cur >= total}
        aria-label={t("shared.pdfViewer.nextPage")}
      >
        <Ic name="chevR" cls="ic" />
      </button>
    </div>
  );
}

function RotateControls({ documentId }: { documentId: string }) {
  const { t } = useTranslation();
  const { provides } = useRotate(documentId);
  return (
    <>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.rotateBackward()}
        aria-label={t("shared.pdfViewer.rotateLeft")}
      >
        <Ic name="rotL" cls="ic" />
      </button>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.rotateForward()}
        aria-label={t("shared.pdfViewer.rotateRight")}
      >
        <Ic name="rotR" cls="ic" />
      </button>
    </>
  );
}

function ZoomControls({ documentId }: { documentId: string }) {
  const { t } = useTranslation();
  const { state, provides } = useZoom(documentId);
  const pct = Math.round((state?.currentZoomLevel ?? 1) * 100);
  return (
    <div className="pdfv-zoom">
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.zoomOut()}
        aria-label={t("shared.pdfViewer.zoomOut")}
      >
        <Ic name="minus" cls="ic" />
      </button>
      <select
        className="pdfv-zoomsel"
        value={ZOOM_PRESETS.includes(pct) ? String(pct) : "custom"}
        onChange={(e) => provides?.requestZoom(Number(e.target.value) / 100)}
        aria-label={t("shared.pdfViewer.zoomLevel")}
      >
        {!ZOOM_PRESETS.includes(pct) && <option value="custom">{pct}%</option>}
        {ZOOM_PRESETS.map((p) => (
          <option key={p} value={String(p)}>{p}%</option>
        ))}
      </select>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.zoomIn()}
        aria-label={t("shared.pdfViewer.zoomIn")}
      >
        <Ic name="plus" cls="ic" />
      </button>
    </div>
  );
}

function SearchBar({ documentId, onClose }: { documentId: string; onClose: () => void }) {
  const { t } = useTranslation();
  const { state, provides } = useSearch(documentId);
  const [q, setQ] = useState("");
  const total = state?.results?.length ?? 0;
  const active = state?.activeResultIndex ?? -1;

  return (
    <div className="pdfv-search">
      <Ic name="lens" cls="ic" />
      <input
        className="pdfv-searchinput"
        autoFocus
        value={q}
        placeholder={t("shared.pdfViewer.searchPlaceholder")}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void provides?.searchAllPages(q);
          if (e.key === "Escape") onClose();
        }}
      />
      <span className="pdfv-searchcount">
        {total > 0 ? `${active + 1} / ${total}` : q ? t("shared.pdfViewer.noMatches") : ""}
      </span>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.previousResult()}
        disabled={total === 0}
        aria-label={t("shared.pdfViewer.prevMatch")}
      >
        <Ic name="chevL" cls="ic" />
      </button>
      <button
        type="button"
        className="sq-btn"
        onClick={() => provides?.nextResult()}
        disabled={total === 0}
        aria-label={t("shared.pdfViewer.nextMatch")}
      >
        <Ic name="chevR" cls="ic" />
      </button>
      <button type="button" className="sq-btn" onClick={onClose} aria-label={t("shared.pdfViewer.close")}>
        <Ic name="xMark" cls="ic" />
      </button>
    </div>
  );
}

function ThumbSidebar({ documentId }: { documentId: string }) {
  const { provides: scroll } = useScroll(documentId);
  return (
    <ThumbnailsPane documentId={documentId}>
      {(m) => (
        <div
          key={m.pageIndex}
          className="pdfv-thumb"
          style={{ position: "absolute", top: m.top, height: m.wrapperHeight, width: "100%" }}
          onClick={() => scroll?.scrollToPage({ pageNumber: m.pageIndex + 1 })}
        >
          <div className="pdfv-thumb-img" style={{ width: m.width, height: m.height }}>
            <ThumbImg documentId={documentId} meta={m} />
          </div>
          <span className="pdfv-thumb-n">{m.pageIndex + 1}</span>
        </div>
      )}
    </ThumbnailsPane>
  );
}

// ── Right-side actions ───────────────────────────────────────────────────────

function SaveButton({ name, bytes }: { name: string; bytes: Uint8Array }) {
  const { t } = useTranslation();
  async function handleSave() {
    try {
      if (isDemoMode()) {
        const blob = new Blob([bytes.slice().buffer], { type: "application/pdf" });
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
      await writeFile(dest, bytes);
      notify.success(t("shared.pdfViewer.saved"));
    } catch (e) {
      notify.error(t("shared.pdfViewer.saveError", { error: String(e) }));
    }
  }
  return (
    <button type="button" className="pill-btn" onClick={handleSave}>
      <Ic name="dl" cls="ic" />
      {t("shared.pdfViewer.save")}
    </button>
  );
}

function OpenExternalButton({ path }: { path: string }) {
  const { t } = useTranslation();
  async function handle() {
    try {
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(path);
    } catch (e) {
      notify.error(t("shared.pdfViewer.openExternalError", { error: String(e) }));
    }
  }
  return (
    <button type="button" className="pill-btn" onClick={handle}>
      <Ic name="eye" cls="ic" />
      {t("shared.pdfViewer.openExternal")}
    </button>
  );
}
