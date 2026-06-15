/**
 * doc-render/descriptors — per-document presentation intent for the labeled document view.
 *
 * A descriptor is intentionally thin: it names the document, picks its label dictionary (by key),
 * and declares the SECTION ORDER + whether each section is a key/value block or a columnar table.
 * The generic renderer (`XmlDocView`) does the XML walking; the descriptor only supplies layout.
 * The UBL invoice has its own typed renderer, so its descriptor carries no sections (key "INVOICE").
 *
 * Selection: by document key (declKind or an explicit `docKey` from the caller), else by root tag,
 * else `null` → the renderer falls back to the generic attribute table. Adding a new declaration is
 * a new descriptor + label dict — no renderer changes.
 */
export interface SectionSpec {
  /** Local tag name this section renders (the root tag → the header/antet block). */
  match: string;
  title: string;
  as: "kv" | "table";
  /** Optional explicit column order for `as: "table"` (else the union of the rows' attributes). */
  columns?: string[];
}

export interface DocDescriptor {
  /** Stable key — matches a declKind ("D205") or an explicit docKey ("INVOICE"). */
  key: string;
  title: string;
  /** Local name of the XML root element (namespace prefix ignored). */
  rootTag: string;
  sections: SectionSpec[];
}

export const D205_DESCRIPTOR: DocDescriptor = {
  key: "D205",
  title: "Declarația 205 — informativă privind impozitul reținut la sursă (dividende)",
  rootTag: "declaratie205",
  sections: [
    { match: "declaratie205", title: "Declarant", as: "kv" },
    { match: "sect_II", title: "Recapitulație (dividende)", as: "kv" },
    {
      match: "benef",
      title: "Beneficiari",
      as: "table",
      columns: [
        "id_inreg",
        "cifR",
        "den1",
        "Rezid",
        "tip_plata",
        "baza1",
        "imp1",
        "divid_D",
        "divid_P",
      ],
    },
  ],
};

/** UBL e-Factura invoice — rendered by the dedicated typed `InvoiceDocView`, not the generic walker. */
export const INVOICE_DESCRIPTOR: DocDescriptor = {
  key: "INVOICE",
  title: "Factură electronică (RO e-Factura)",
  rootTag: "Invoice",
  sections: [],
};

export const DESCRIPTORS: DocDescriptor[] = [D205_DESCRIPTOR, INVOICE_DESCRIPTOR];

const BY_KEY = new Map(DESCRIPTORS.map((d) => [d.key, d]));
const BY_ROOT = new Map(DESCRIPTORS.map((d) => [d.rootTag, d]));

/**
 * Pick the descriptor for a document: explicit key (docKey/declKind) first, then root tag, else null.
 * `rootTag` should be the root element's local name (no namespace prefix).
 */
export function pickDescriptor(key: string | undefined, rootTag: string): DocDescriptor | null {
  if (key && BY_KEY.has(key)) return BY_KEY.get(key)!;
  return BY_ROOT.get(rootTag) ?? null;
}
