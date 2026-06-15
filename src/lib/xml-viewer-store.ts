/**
 * Transient store for the in-app XML viewer/editor (XmlViewerModal).
 *
 * NOT persisted — pure ephemeral UI state. `xml` is the document text (the declaration / e-Factura
 * XML the app generates); `declKind`, when set, enables the "re-validate with DUK" action for that
 * declaration type. Opening always replaces any current document (one viewer at a time).
 */
import { create } from "zustand";

import type { XmlDeclKind } from "@/lib/tauri";

export interface XmlViewerPayload {
  /** The XML text to show, and the initial editor content. */
  xml: string;
  /** Display name — modal title + default download filename (".xml" appended if missing). */
  name: string;
  /** When set, enables "re-validate with DUK" for this declaration type (D300/D394/D406/D112/D205). */
  declKind?: XmlDeclKind;
  /** Explicit document-render key (e.g. "INVOICE") for callers with no `declKind` — selects the
   *  labeled-document descriptor in `XmlDocView`. Falls back to declKind / root tag when unset. */
  docKey?: string;
  /** Start in editable mode instead of the read-only viewer (default: false). */
  editable?: boolean;
}

interface XmlViewerState {
  payload: XmlViewerPayload | null;
  open: (payload: XmlViewerPayload) => void;
  close: () => void;
}

export const useXmlViewerStore = create<XmlViewerState>((set) => ({
  payload: null,
  open: (payload) => set({ payload }),
  close: () => set({ payload: null }),
}));
