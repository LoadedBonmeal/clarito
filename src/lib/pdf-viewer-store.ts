/**
 * Transient store for the in-app PDF viewer (PdfViewerModal).
 *
 * NOT persisted — this is pure ephemeral UI state. The bytes are the PDF
 * content (read off disk via plugin-fs in the real app, or a bundled sample in
 * the demo harness); `externalPath` is the on-disk path so the viewer can offer
 * "open in the system viewer" as a secondary action.
 */
import { create } from "zustand";

export interface PdfViewerPayload {
  /** Raw PDF bytes fed to EmbedPDF's openDocumentBuffer. */
  bytes: Uint8Array;
  /** Display name (modal title + download filename). */
  name: string;
  /** On-disk path, when known — enables the "open externally" action. */
  externalPath?: string;
}

interface PdfViewerState {
  payload: PdfViewerPayload | null;
  open: (payload: PdfViewerPayload) => void;
  close: () => void;
}

export const usePdfViewerStore = create<PdfViewerState>((set) => ({
  payload: null,
  open: (payload) => set({ payload }),
  close: () => set({ payload: null }),
}));
