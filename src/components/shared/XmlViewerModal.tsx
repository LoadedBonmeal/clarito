/**
 * XmlViewerModal — in-app XML viewer + editor in the design's .modal-back/.modal chrome, powered by
 * CodeMirror 6 (@codemirror/lang-xml). Monochrome, matching the app.
 *
 * Viewer  : syntax highlight · code folding (per-element + fold/unfold-all) · find-in-document
 *           (Cmd/Ctrl+F) · copy · save-as.
 * Editor  : toggle to edit · pretty-print (Format) · "re-validate with DUK" for declaration XML
 *           (D300/D394/D406/D112/D205) via the bundled ANAF validators.
 *
 * Fed from the store (useXmlViewerStore / useOpenXml) with the declaration / e-Factura XML the app
 * generates — same path for the real app and the demo harness (?demo=1, validate/preview stubbed).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { EditorView, basicSetup } from "codemirror";
import { Compartment, EditorState } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { openSearchPanel } from "@codemirror/search";
import { HighlightStyle, foldAll, syntaxHighlighting, unfoldAll } from "@codemirror/language";
import { xml } from "@codemirror/lang-xml";
import { tags as ht } from "@lezer/highlight";

import { Ic } from "@/components/shared/Ic";
import { XmlTableView } from "@/components/shared/XmlTableView";
import { useXmlViewerStore, type XmlViewerPayload } from "@/lib/xml-viewer-store";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { formatXml } from "@/lib/xml-format";
import { api, type XmlDukValidation } from "@/lib/tauri";
import { isDemoMode } from "@/lib/demo";
import { notify } from "@/lib/toasts";

// ── Monochrome CodeMirror theme (resolves the app's CSS vars; safe fallbacks) ───────────────────
const monoTheme = EditorView.theme(
  {
    "&": { color: "var(--text, #1d1d1f)", backgroundColor: "#fff", height: "100%", fontSize: "12.5px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": {
      fontFamily: "'SF Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      lineHeight: "1.55",
    },
    ".cm-content": { padding: "10px 0" },
    ".cm-gutters": {
      backgroundColor: "var(--fill, #f6f6f7)",
      color: "var(--text-3, #b8b8be)",
      border: "none",
      borderRight: "1px solid var(--line, #ececee)",
    },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-2, #6e6e73)" },
    ".cm-activeLine": { backgroundColor: "rgba(0,0,0,0.025)" },
    ".cm-cursor": { borderLeftColor: "var(--text, #1d1d1f)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "rgba(0,0,0,0.10)",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "var(--fill, #f6f6f7)",
      border: "1px solid var(--line, #ececee)",
      color: "var(--text-2, #6e6e73)",
      padding: "0 6px",
      margin: "0 2px",
      borderRadius: "5px",
    },
    ".cm-panels": {
      backgroundColor: "var(--fill, #f6f6f7)",
      color: "var(--text, #1d1d1f)",
      borderTop: "1px solid var(--line, #ececee)",
    },
    ".cm-panel.cm-search input, .cm-panel.cm-search button, .cm-textfield": {
      fontFamily: "inherit",
      fontSize: "12.5px",
    },
    ".cm-searchMatch": { backgroundColor: "rgba(0,0,0,0.12)", outline: "1px solid var(--text-3, #b8b8be)" },
    ".cm-searchMatch-selected": { backgroundColor: "rgba(0,0,0,0.24)" },
  },
  { dark: false },
);

// Monochrome syntax highlight — defined tags win; basicSetup's default style fills the rest (fallback).
const monoHighlight = HighlightStyle.define([
  { tag: [ht.tagName, ht.angleBracket], color: "var(--text, #1d1d1f)", fontWeight: "600" },
  { tag: ht.attributeName, color: "var(--text-2, #6e6e73)" },
  { tag: [ht.attributeValue, ht.string], color: "#3f7a52" },
  { tag: ht.comment, color: "var(--text-3, #a0a0a6)", fontStyle: "italic" },
  { tag: [ht.processingInstruction, ht.meta], color: "var(--text-3, #a0a0a6)" },
  { tag: ht.content, color: "var(--text, #1d1d1f)" },
]);

const READ_ONLY = [EditorView.editable.of(false), EditorState.readOnly.of(true)];

export function XmlViewerModal() {
  const payload = useXmlViewerStore((s) => s.payload);
  const storeClose = useXmlViewerStore((s) => s.close);
  const { closing, close } = useAnimatedClose(storeClose);

  useEffect(() => {
    if (!payload) return;
    const h = (e: KeyboardEvent) => {
      // Don't steal Escape from the CodeMirror search panel (it closes the panel itself).
      if (e.key === "Escape" && !(e.target as HTMLElement)?.closest?.(".cm-panel")) close();
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
        <XmlEditor payload={payload} onClose={close} />
      </div>
    </div>,
    document.body,
  );
}

function XmlEditor({ payload, onClose }: { payload: XmlViewerPayload; onClose: () => void }) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const editComp = useRef(new Compartment());
  const [editable, setEditable] = useState(!!payload.editable);
  const [dirty, setDirty] = useState(false);
  const [validating, setValidating] = useState(false);
  const [validation, setValidation] = useState<XmlDukValidation | null>(null);
  // Default to the human-readable TABLE view; "code" reveals the CodeMirror editor (for editing).
  const [viewMode, setViewMode] = useState<"table" | "code">(payload.editable ? "code" : "table");
  const [tableXml, setTableXml] = useState(payload.xml);

  // Build the editor once for this modal's lifetime (open→close→open re-mounts it fresh).
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: payload.xml,
        extensions: [
          basicSetup,
          xml(),
          syntaxHighlighting(monoHighlight),
          monoTheme,
          EditorView.lineWrapping,
          keymap.of([indentWithTab]),
          editComp.current.of(payload.editable ? [] : READ_ONLY),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              setDirty(true);
              setValidation(null); // edits invalidate the previous DUK verdict
            }
          }),
        ],
      }),
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const doc = () => viewRef.current?.state.doc.toString() ?? payload.xml;

  const toggleEditable = useCallback(() => {
    const view = viewRef.current;
    if (!view) return;
    const next = !editable;
    setEditable(next);
    view.dispatch({ effects: editComp.current.reconfigure(next ? [] : READ_ONLY) });
    if (next) view.focus();
  }, [editable]);

  const doFind = () => viewRef.current && openSearchPanel(viewRef.current);
  const doFold = () => viewRef.current && foldAll(viewRef.current);
  const doUnfold = () => viewRef.current && unfoldAll(viewRef.current);

  const doFormat = () => {
    const view = viewRef.current;
    if (!view) return;
    const formatted = formatXml(view.state.doc.toString());
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: formatted } });
    notify.success(t("shared.xmlViewer.formatted"));
  };

  const doCopy = async () => {
    try {
      await navigator.clipboard.writeText(doc());
      notify.success(t("shared.xmlViewer.copied"));
    } catch (e) {
      notify.error(t("shared.xmlViewer.copyError", { error: String(e) }));
    }
  };

  const fileName = payload.name.endsWith(".xml") ? payload.name : `${payload.name}.xml`;

  const doSave = async () => {
    const text = doc();
    try {
      if (isDemoMode()) {
        const url = URL.createObjectURL(new Blob([text], { type: "application/xml" }));
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
        await writeTextFile(dest, text);
      }
      setDirty(false);
      notify.success(t("shared.xmlViewer.saved"));
    } catch (e) {
      notify.error(t("shared.xmlViewer.saveError", { error: String(e) }));
    }
  };

  const doRevalidate = async () => {
    if (!payload.declKind) return;
    setValidating(true);
    setValidation(null);
    try {
      const res = await api.declarations.validateDeclarationXml(payload.declKind, doc());
      setValidation(res);
    } catch (e) {
      notify.error(t("shared.xmlViewer.validateError", { error: String(e) }));
    } finally {
      setValidating(false);
    }
  };

  // The table view always reflects the current document; snapshot the editor doc when switching to it.
  const showTable = () => {
    setTableXml(doc());
    setViewMode("table");
  };
  const showCode = () => setViewMode("code");
  const isCode = viewMode === "code";

  return (
    <>
      <header className="pdfv-bar">
        <div className="pdfv-bar-l">
          <div className="pdfv-title">
            <Ic name="code" cls="ic" />
            <span>{payload.name}</span>
            {dirty && (
              <span className="xmlv-dirty" title={t("shared.xmlViewer.unsaved")}>
                ●
              </span>
            )}
          </div>
        </div>

        <div className="pdfv-bar-c">
          <div className="xmlv-toggle" role="tablist" aria-label={t("shared.xmlViewer.viewSwitch")}>
            <button
              type="button"
              role="tab"
              aria-selected={!isCode}
              className={!isCode ? "is-on" : ""}
              onClick={showTable}
            >
              <Ic name="grid" cls="ic" />
              {t("shared.xmlViewer.tableView")}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={isCode}
              className={isCode ? "is-on" : ""}
              onClick={showCode}
            >
              <Ic name="code" cls="ic" />
              {t("shared.xmlViewer.codeView")}
            </button>
          </div>
          {isCode && (
            <>
              <span className="pdfv-sep" />
              <button type="button" className="sq-btn" onClick={doFind} aria-label={t("shared.xmlViewer.find")}>
                <Ic name="lens" cls="ic" />
              </button>
              <button type="button" className="sq-btn" onClick={doFold} aria-label={t("shared.xmlViewer.foldAll")}>
                <Ic name="collapse" cls="ic" />
              </button>
              <button
                type="button"
                className="sq-btn"
                onClick={doUnfold}
                aria-label={t("shared.xmlViewer.unfoldAll")}
              >
                <Ic name="chevUD" cls="ic" />
              </button>
            </>
          )}
        </div>

        <div className="pdfv-bar-r">
          {isCode && (
            <>
              <button
                type="button"
                className={`sq-btn${editable ? " is-on" : ""}`}
                onClick={toggleEditable}
                aria-label={editable ? t("shared.xmlViewer.viewMode") : t("shared.xmlViewer.editMode")}
                aria-pressed={editable}
                title={editable ? t("shared.xmlViewer.viewMode") : t("shared.xmlViewer.editMode")}
              >
                <Ic name={editable ? "eye" : "pen"} cls="ic" />
              </button>
              <button type="button" className="pill-btn" onClick={doFormat}>
                <Ic name="code" cls="ic" />
                {t("shared.xmlViewer.format")}
              </button>
            </>
          )}
          {payload.declKind && (
            <button type="button" className="pill-btn" onClick={doRevalidate} disabled={validating}>
              <Ic name="shield" cls="ic" />
              {validating ? t("shared.xmlViewer.validating") : t("shared.xmlViewer.revalidate")}
            </button>
          )}
          <button type="button" className="sq-btn" onClick={doCopy} aria-label={t("shared.xmlViewer.copy")}>
            <Ic name="copy" cls="ic" />
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

      {/* CodeMirror stays mounted (hidden in table mode) so edits + the doc are preserved. */}
      <div className="xmlv-editor" ref={hostRef} style={{ display: isCode ? "block" : "none" }} />
      {!isCode && <XmlTableView xml={tableXml} />}
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
