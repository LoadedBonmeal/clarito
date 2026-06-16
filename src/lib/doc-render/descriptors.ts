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
  /** For `as: "kv"`: render only these attributes (in order), skipping ones absent on the element.
   *  Lets several sections group the attributes of a single element (e.g. D300's root-only attrs). */
  attrs?: string[];
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

export const D112_DESCRIPTOR: DocDescriptor = {
  key: "D112",
  title: "Declarația 112 — contribuții sociale și impozit pe venit",
  rootTag: "declaratieUnica",
  sections: [
    { match: "declaratieUnica", title: "Declarant", as: "kv" },
    { match: "angajator", title: "Angajator", as: "kv" },
    { match: "angajatorA", title: "Obligații de plată", as: "table", columns: ["A_codOblig", "A_datorat", "A_deductibil", "A_scutit", "A_plata"] },
    { match: "angajatorB", title: "Total asigurați", as: "kv" },
    { match: "angajatorC4", title: "Contribuție asigurătorie de muncă (CAM)", as: "kv" },
    { match: "asigurat", title: "Asigurați", as: "table", columns: ["idAsig", "cnpAsig", "numeAsig", "prenAsig", "dataAng", "Timp_E3"] },
    { match: "asiguratA", title: "Contribuții (pe asigurat)", as: "table", columns: ["A_1", "A_sal2", "A_8", "A_13", "A_14", "A_11", "A_12", "A_5"] },
    { match: "asiguratE3", title: "Impozit pe venit (pe asigurat)", as: "table", columns: ["E3_8", "E3_9", "E3_12", "E3_14", "E3_15", "E3_16"] },
  ],
};

export const D300_DESCRIPTOR: DocDescriptor = {
  key: "D300",
  title: "Decont de taxă pe valoarea adăugată (D300)",
  rootTag: "declaratie300",
  sections: [
    { match: "declaratie300", title: "Antet și date de identificare", as: "kv", attrs: ["luna", "an", "depusReprezentant", "bifa_interne", "temei", "nume_declar", "prenume_declar", "functie_declar", "cui", "den", "adresa", "banca", "cont", "caen", "tip_decont", "pro_rata", "bifa_cereale", "bifa_mob", "bifa_disp", "bifa_cons", "solicit_ramb", "nr_evid"] },
    { match: "declaratie300", title: "TVA colectată", as: "kv", attrs: ["R1_1", "R9_1", "R9_2", "R10_1", "R10_2", "R11_1", "R11_2", "R12_1", "R12_2", "R12_1_1", "R12_1_2", "R12_2_1", "R12_2_2", "R13_1", "R16_1", "R16_2", "R17_1", "R17_2"] },
    { match: "declaratie300", title: "TVA deductibilă", as: "kv", attrs: ["R5_1", "R5_2", "R7_1", "R7_2", "R7_1_1", "R7_1_2", "R18_1", "R18_2", "R20_1", "R20_2", "R20_1_1", "R20_1_2", "R22_1", "R22_2", "R23_1", "R23_2", "R25_1", "R25_2", "R25_1_1", "R25_1_2", "R25_2_1", "R25_2_2", "R27_1", "R27_2", "R28_2"] },
    { match: "declaratie300", title: "Regularizări (cote vechi)", as: "kv", attrs: ["R30_1", "R30_2"] },
    { match: "declaratie300", title: "Totaluri și solduri", as: "kv", attrs: ["R32_2", "R33_2", "R34_2", "R37_2", "R40_2", "R41_2", "R42_2", "totalPlata_A"] },
  ],
};

export const D390_DESCRIPTOR: DocDescriptor = {
  key: "D390",
  title: "Declarație recapitulativă VIES (D390)",
  rootTag: "declaratie390",
  sections: [
    { match: "declaratie390", title: "Antet și date contribuabil", as: "kv" },
    { match: "rezumat", title: "Rezumat — totaluri pe coduri", as: "kv" },
    { match: "operatie", title: "Operațiuni intracomunitare", as: "table", columns: ["tip", "tara", "codO", "denO", "baza"] },
  ],
};

export const D394_DESCRIPTOR: DocDescriptor = {
  key: "D394",
  title: "Declarația 394 — livrări/achiziții pe teritoriul național",
  rootTag: "declaratie394",
  sections: [
    { match: "declaratie394", title: "Antet și identificare", as: "kv" },
    { match: "informatii", title: "Rezumat general", as: "kv" },
    { match: "rezumat1", title: "Rezumat pe tip partener și cotă", as: "table" },
    { match: "detaliu", title: "Detalii art. 331", as: "table" },
    { match: "rezumat2", title: "Rezumat pe cotă TVA", as: "table" },
    { match: "serieFacturi", title: "Serii de facturi", as: "table" },
    { match: "op1", title: "Operațiuni pe partener", as: "table" },
    { match: "op11", title: "Detalii operațiuni art. 331", as: "table" },
  ],
};

/** SAF-T (D406) — rendered by the dedicated typed `SaftDocView` (summary cover page), not the
 *  generic walker. No sections: the renderer parses the AuditFile summary itself. */
export const D406_DESCRIPTOR: DocDescriptor = {
  key: "D406",
  title: "Rezumat pre-depunere SAF-T (D406)",
  rootTag: "AuditFile",
  sections: [],
};

export const DESCRIPTORS: DocDescriptor[] = [
  D205_DESCRIPTOR,
  INVOICE_DESCRIPTOR,
  D112_DESCRIPTOR,
  D300_DESCRIPTOR,
  D390_DESCRIPTOR,
  D394_DESCRIPTOR,
  D406_DESCRIPTOR,
];

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
