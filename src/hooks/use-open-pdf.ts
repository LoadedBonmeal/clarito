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
      open({ bytes, name, externalPath: path });
    },
    [open],
  );
}
