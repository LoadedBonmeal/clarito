/**
 * useOpenPdf — turns an on-disk PDF path into an in-app viewer session.
 *
 * Real app: reads the file bytes via @tauri-apps/plugin-fs (the generated
 * invoice/receipt/preview PDFs live under app_data_dir/** or $TEMP/**, both
 * already allowed by the fs capability — no new permission needed).
 *
 * Demo harness (?demo=1): there is no Tauri fs, so we fetch a bundled sample
 * PDF from /sample-invoice.pdf instead, which lets the Playwright loop exercise
 * the whole viewer without a backend.
 */
import { useCallback } from "react";
import { usePdfViewerStore } from "@/lib/pdf-viewer-store";
import { isDemoMode } from "@/lib/demo";

export function useOpenPdf() {
  const open = usePdfViewerStore((s) => s.open);

  return useCallback(
    async (path: string, name: string) => {
      let bytes: Uint8Array;
      if (isDemoMode()) {
        const res = await fetch("/sample-invoice.pdf");
        bytes = new Uint8Array(await res.arrayBuffer());
      } else {
        const { readFile } = await import("@tauri-apps/plugin-fs");
        bytes = await readFile(path);
      }
      // ROB-06: guard the PDFium WASM against bad input — reject empty/oversized files and
      // anything that isn't a PDF (magic bytes "%PDF") before handing bytes to the viewer.
      const MAX_BYTES = 100 * 1024 * 1024; // 100 MB
      if (bytes.length === 0 || bytes.length > MAX_BYTES) {
        throw new Error("Fișier PDF invalid (gol sau peste 100 MB).");
      }
      if (!(bytes[0] === 0x25 && bytes[1] === 0x50 && bytes[2] === 0x44 && bytes[3] === 0x46)) {
        throw new Error("Fișierul nu este un PDF valid (lipsește semnătura %PDF).");
      }
      open({ bytes, name, externalPath: path });
    },
    [open],
  );
}
