/**
 * useOpenXml — opens an XML string in the in-app viewer/editor (XmlViewerModal).
 *
 * The XML is already in memory (returned by the declaration/e-Factura generators), so unlike the
 * PDF viewer there is no file read — this hook just guards the input size and hands it to the store.
 * Identical path for the real app and the demo harness (?demo=1).
 */
import { useCallback } from "react";

import { useXmlViewerStore, type XmlViewerPayload } from "@/lib/xml-viewer-store";

// Guards CodeMirror against pathological input. ANAF declaration / e-Factura XML is well under this.
const MAX_XML_CHARS = 25 * 1024 * 1024; // ~25 MB

export function useOpenXml() {
  const open = useXmlViewerStore((s) => s.open);

  return useCallback(
    (payload: XmlViewerPayload) => {
      const xml = payload.xml ?? "";
      if (xml.trim().length === 0) {
        throw new Error("XML gol — nimic de afișat.");
      }
      if (xml.length > MAX_XML_CHARS) {
        throw new Error("Fișier XML prea mare pentru editor (peste 25 MB).");
      }
      open({ ...payload, xml });
    },
    [open],
  );
}
