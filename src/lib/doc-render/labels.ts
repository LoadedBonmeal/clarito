/**
 * doc-render/labels — the human-label dictionary that turns cryptic ANAF XML attribute names
 * (cui, cifR, baza1, divid_D, …) into the Romanian labels a person reads, plus value formatters
 * (money / date / month / coded enums). This is the data that makes the XML preview look like the
 * official ANAF document instead of a tag dump.
 *
 * Labels are a TRANSCRIPTION of the authoritative sources already in the repo — the Rust generator
 * doc comments (verified against the official DUK validator, e.g. `anaf_decl/d205_xml.rs`) and the
 * entry forms / i18n — never invented. Resolution never throws: an unknown attribute falls back to
 * its raw name, so a new/unmapped field still renders (just un-prettified) rather than breaking.
 */
import { MONTHS_RO, fmtRON, formatDate, formatNumber } from "@/lib/utils";

export type FieldFormat = "money_lei" | "money2" | "date" | "month" | "enum" | "cnp" | "text";

export interface FieldSpec {
  label: string;
  format?: FieldFormat;
  /** For `format: "enum"` — code → human meaning (e.g. d_rec 0→"Inițială"). */
  enumMap?: Record<string, string>;
}

export type LabelDict = Record<string, FieldSpec>;

/** Format a raw XML attribute value per its FieldSpec. Always returns a string; never throws. */
export function formatValue(raw: string, spec: FieldSpec | undefined): string {
  const v = (raw ?? "").trim();
  if (!spec || !spec.format || spec.format === "text" || spec.format === "cnp") return v;
  switch (spec.format) {
    case "money_lei": {
      // ANAF declaration sums are whole lei (N15 integers) — no decimals, thousands grouped.
      const n = Number(v);
      return Number.isFinite(n) ? `${formatNumber(n, 0)} lei` : v;
    }
    case "money2": {
      const n = Number(v);
      return Number.isFinite(n) ? fmtRON(n) : v;
    }
    case "date":
      return /^\d{4}-\d{2}-\d{2}$/.test(v) ? formatDate(v) : v;
    case "month": {
      const m = Number(v);
      return m >= 1 && m <= 12 ? MONTHS_RO[m - 1] : v;
    }
    case "enum":
      return spec.enumMap?.[v] ?? v;
    default:
      return v;
  }
}

// ── Pan-declaration fields (shared antet/header attributes across most ANAF declarations) ─────────
export const GLOBAL_FIELDS: LabelDict = {
  cui: { label: "Cod fiscal (CUI)" },
  cif: { label: "Cod fiscal (CIF)" },
  den: { label: "Denumire" },
  adresa: { label: "Adresă" },
  an: { label: "An" },
  luna: { label: "Lună", format: "month" },
  d_rec: {
    label: "Tip declarație",
    format: "enum",
    enumMap: { "0": "Inițială", "1": "Rectificativă" },
  },
  nume_declar: { label: "Nume declarant" },
  prenume_declar: { label: "Prenume declarant" },
  functie_declar: { label: "Funcție declarant" },
};

const TIP_VENIT: Record<string, string> = { "08": "Dividende" };

// ── D205 — informativă privind impozitul reținut la sursă (capitol dividende) ─────────────────────
export const D205_FIELDS: LabelDict = {
  an: { label: "An de venit" },
  totalPlata_A: { label: "Total plată (control)", format: "money_lei" },
  // sect_II — recapitulația secțiunii
  tip_venit: { label: "Tip venit", format: "enum", enumMap: TIP_VENIT },
  nrben: { label: "Nr. beneficiari" },
  Tcastig: { label: "Total câștig", format: "money_lei" },
  Tpierd: { label: "Total pierdere", format: "money_lei" },
  T_VB: { label: "Total venit brut", format: "money_lei" },
  T_GAR: { label: "Total garanție", format: "money_lei" },
  Tbaza: { label: "Total bază de calcul", format: "money_lei" },
  Timp: { label: "Total impozit", format: "money_lei" },
  // benef — rândul de beneficiar
  id_inreg: { label: "Nr." },
  tip_venit1: { label: "Tip venit", format: "enum", enumMap: TIP_VENIT },
  tip_plata: { label: "Tip plată", format: "enum", enumMap: { "2": "Finală" } },
  Rezid: { label: "Rezidență", format: "enum", enumMap: { "1": "Rezident", "2": "Nerezident" } },
  cifR: { label: "CNP beneficiar", format: "cnp" },
  den1: { label: "Nume beneficiar" },
  baza1: { label: "Bază de calcul", format: "money_lei" },
  imp1: { label: "Impozit reținut", format: "money_lei" },
  divid_D: { label: "Dividende distribuite", format: "money_lei" },
  divid_P: { label: "Dividende plătite", format: "money_lei" },
};

// ── D112 / D300 / D390 / D394 — generated from the app Rust generators (DUK-validated) ──────────
export const D112_FIELDS: LabelDict = {
  "luna_r": { label: "Luna", format: "month" },
  "an_r": { label: "An" },
  "d_rec": { label: "Tip declarație", format: "enum", enumMap: { "0": "Inițială", "1": "Rectificativă" } },
  "nume_declar": { label: "Nume declarant" },
  "prenume_declar": { label: "Prenume declarant" },
  "functie_declar": { label: "Funcție declarant" },
  "cif": { label: "Cod fiscal angajator (CUI)", format: "cnp" },
  "caen": { label: "Cod CAEN" },
  "den": { label: "Denumire angajator" },
  "casaAng": { label: "Casă de sănătate (cod)" },
  "datCAM": { label: "Datorează CAM", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "totalPlata_A": { label: "Total de plată", format: "money_lei" },
  "A_codOblig": { label: "Obligație", format: "enum", enumMap: { "602": "Impozit pe salarii", "412": "CAS (pensii)", "432": "CASS (sănătate)", "480": "CAM" } },
  "A_codBugetar": { label: "Cod bugetar" },
  "A_datorat": { label: "Datorat", format: "money_lei" },
  "A_deductibil": { label: "Deductibil", format: "money_lei" },
  "A_scutit": { label: "Scutit", format: "money_lei" },
  "A_plata": { label: "De plată", format: "money_lei" },
  "B_cnp": { label: "Nr. asigurați" },
  "B_sanatate": { label: "Nr. asig. sănătate" },
  "B_pensie": { label: "Nr. asig. pensie" },
  "B_sal": { label: "Nr. asig. salariați" },
  "B_brutSalarii": { label: "Fond brut de salarii", format: "money_lei" },
  "C1_11": { label: "Bază CAS — condiții normale", format: "money_lei" },
  "C1_12": { label: "Bază CAS — indemnizații CM", format: "money_lei" },
  "C4_baza": { label: "Bază CAM", format: "money_lei" },
  "C4_ct": { label: "CAM datorat", format: "money_lei" },
  "idAsig": { label: "Nr." },
  "cnpAsig": { label: "CNP", format: "cnp" },
  "numeAsig": { label: "Nume" },
  "prenAsig": { label: "Prenume" },
  "dataAng": { label: "Data angajării", format: "date" },
  "casaSn": { label: "Casă de sănătate (cod)" },
  "Timp_E3": { label: "Impozit pe venit", format: "money_lei" },
  "A_1": { label: "Tip asigurat" },
  "A_sal1": { label: "Salariu de bază (contract)", format: "money_lei" },
  "A_sal2": { label: "Venit brut realizat", format: "money_lei" },
  "A_2": { label: "Pensionar", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "A_3": { label: "Tip contract" },
  "A_4": { label: "Ore normă" },
  "A_5": { label: "Bază CAM", format: "money_lei" },
  "A_6": { label: "Ore lucrate" },
  "A_8": { label: "Zile lucrate" },
  "A_9": { label: "Bază asig. șomaj", format: "money_lei" },
  "A_11": { label: "Bază CASS", format: "money_lei" },
  "A_12": { label: "CASS reținut", format: "money_lei" },
  "A_13": { label: "Bază CAS", format: "money_lei" },
  "A_14": { label: "CAS reținut", format: "money_lei" },
  "E3_8": { label: "Venit brut", format: "money_lei" },
  "E3_9": { label: "Contribuții (CAS+CASS)", format: "money_lei" },
  "E3_12": { label: "Deducere personală", format: "money_lei" },
  "E3_14": { label: "Bază impozit", format: "money_lei" },
  "E3_15": { label: "Impozit", format: "money_lei" },
  "E3_16": { label: "Venit net încasat", format: "money_lei" },
};

export const D300_FIELDS: LabelDict = {
  "luna": { label: "Luna de raportare", format: "month" },
  "an": { label: "Anul de raportare" },
  "depusReprezentant": { label: "Depusă de reprezentant/împuternicit", format: "enum", enumMap: { "0": "Depusă de titular", "1": "Depusă de reprezentant/împuternicit" } },
  "bifa_interne": { label: "Bifă operațiuni interne", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "temei": { label: "Temei depunere", format: "enum", enumMap: { "0": "Declarație curentă", "2": "Conform unui act normativ" } },
  "nume_declar": { label: "Nume declarant" },
  "prenume_declar": { label: "Prenume declarant" },
  "functie_declar": { label: "Funcție declarant" },
  "cui": { label: "Cod de identificare fiscală (CUI, fără prefix RO)", format: "cnp" },
  "den": { label: "Denumire persoană impozabilă" },
  "adresa": { label: "Adresă (domiciliu fiscal)" },
  "banca": { label: "Banca" },
  "cont": { label: "Cont bancar (IBAN)" },
  "caen": { label: "Cod CAEN obiect principal de activitate" },
  "tip_decont": { label: "Tip decont (perioadă fiscală)", format: "enum", enumMap: { "L": "Lunar", "T": "Trimestrial", "S": "Semestrial", "A": "Anual" } },
  "pro_rata": { label: "Pro-rata de deducere (%)" },
  "bifa_cereale": { label: "Bifă achiziții cereale/plante tehnice (taxare inversă)", format: "enum", enumMap: { "D": "Da", "N": "Nu" } },
  "bifa_mob": { label: "Bifă livrări/achiziții telefoane mobile, microprocesoare, console etc.", format: "enum", enumMap: { "D": "Da", "N": "Nu" } },
  "bifa_disp": { label: "Bifă dispozitive cu circuite integrate (taxare inversă)", format: "enum", enumMap: { "D": "Da", "N": "Nu" } },
  "bifa_cons": { label: "Bifă energie electrică/certificate verzi (taxare inversă)", format: "enum", enumMap: { "D": "Da", "N": "Nu" } },
  "solicit_ramb": { label: "Solicită rambursarea soldului sumei negative a TVA", format: "enum", enumMap: { "D": "Da", "N": "Nu" } },
  "nr_evid": { label: "Număr de evidență a plății (NDP, 23 caractere)" },
  "totalPlata_A": { label: "Sumă de control (totalPlata_A — suma tuturor rândurilor R)", format: "money_lei" },
  "R1_1": { label: "Livrări intracomunitare de bunuri scutite (art. 294) — bază", format: "money_lei" },
  "R5_1": { label: "Achiziții intracomunitare de bunuri — bază", format: "money_lei" },
  "R5_2": { label: "Achiziții intracomunitare de bunuri — TVA colectată", format: "money_lei" },
  "R7_1": { label: "Achiziții intracomunitare de servicii — bază", format: "money_lei" },
  "R7_2": { label: "Achiziții intracomunitare de servicii — TVA colectată", format: "money_lei" },
  "R7_1_1": { label: "Achiziții servicii intra-UE cotă 21% — bază", format: "money_lei" },
  "R7_1_2": { label: "Achiziții servicii intra-UE cotă 21% — TVA", format: "money_lei" },
  "R9_1": { label: "Livrări taxabile cotă standard 21% — bază", format: "money_lei" },
  "R9_2": { label: "Livrări taxabile cotă standard 21% — TVA colectată", format: "money_lei" },
  "R10_1": { label: "Livrări taxabile cotă redusă 11% — bază", format: "money_lei" },
  "R10_2": { label: "Livrări taxabile cotă redusă 11% — TVA colectată", format: "money_lei" },
  "R11_1": { label: "Livrări taxabile cotă 9% tranzitorie (locuințe, art. III Legea 141/2025) — bază", format: "money_lei" },
  "R11_2": { label: "Livrări taxabile cotă 9% tranzitorie (locuințe, art. III Legea 141/2025) — TVA colectată", format: "money_lei" },
  "R12_1": { label: "Total taxare inversă domestică (beneficiar) — bază", format: "money_lei" },
  "R12_2": { label: "Total taxare inversă domestică (beneficiar) — TVA", format: "money_lei" },
  "R12_1_1": { label: "Taxare inversă cotă 21% — bază", format: "money_lei" },
  "R12_1_2": { label: "Taxare inversă cotă 21% — TVA", format: "money_lei" },
  "R12_2_1": { label: "Taxare inversă cotă 11% — bază", format: "money_lei" },
  "R12_2_2": { label: "Taxare inversă cotă 11% — TVA", format: "money_lei" },
  "R13_1": { label: "Livrări cu taxare inversă (vânzător, art.331) — bază", format: "money_lei" },
  "R16_1": { label: "Regularizări taxă colectată (cote vechi 19%/5%) — bază", format: "money_lei" },
  "R16_2": { label: "Regularizări taxă colectată (cote vechi 19%/5%) — TVA", format: "money_lei" },
  "R17_1": { label: "TOTAL taxă colectată — bază", format: "money_lei" },
  "R17_2": { label: "TOTAL taxă colectată — TVA", format: "money_lei" },
  "R18_1": { label: "Deductibil aferent achizițiilor intra-UE de bunuri — bază", format: "money_lei" },
  "R18_2": { label: "Deductibil aferent achizițiilor intra-UE de bunuri — TVA", format: "money_lei" },
  "R20_1": { label: "Deductibil aferent achizițiilor intra-UE de servicii — bază", format: "money_lei" },
  "R20_2": { label: "Deductibil aferent achizițiilor intra-UE de servicii — TVA", format: "money_lei" },
  "R20_1_1": { label: "Servicii intra-UE cotă 21% deductibil — bază", format: "money_lei" },
  "R20_1_2": { label: "Servicii intra-UE cotă 21% deductibil — TVA", format: "money_lei" },
  "R22_1": { label: "Achiziții interne cotă standard 21% deductibil — bază", format: "money_lei" },
  "R22_2": { label: "Achiziții interne cotă standard 21% deductibil — TVA", format: "money_lei" },
  "R23_1": { label: "Achiziții interne cotă redusă 11% deductibil — bază", format: "money_lei" },
  "R23_2": { label: "Achiziții interne cotă redusă 11% deductibil — TVA", format: "money_lei" },
  "R25_1": { label: "Total deductibil taxare inversă domestică — bază", format: "money_lei" },
  "R25_2": { label: "Total deductibil taxare inversă domestică — TVA", format: "money_lei" },
  "R25_1_1": { label: "Taxare inversă cotă 21% deductibil — bază", format: "money_lei" },
  "R25_1_2": { label: "Taxare inversă cotă 21% deductibil — TVA", format: "money_lei" },
  "R25_2_1": { label: "Taxare inversă cotă 11% deductibil — bază", format: "money_lei" },
  "R25_2_2": { label: "Taxare inversă cotă 11% deductibil — TVA", format: "money_lei" },
  "R27_1": { label: "TOTAL taxă deductibilă — bază", format: "money_lei" },
  "R27_2": { label: "TOTAL taxă deductibilă — TVA", format: "money_lei" },
  "R28_2": { label: "Sub-total taxă dedusă — TVA", format: "money_lei" },
  "R30_1": { label: "Regularizări taxă dedusă (cote vechi 19%/9%/5%) — bază", format: "money_lei" },
  "R30_2": { label: "Regularizări taxă dedusă (cote vechi 19%/9%/5%) — TVA", format: "money_lei" },
  "R32_2": { label: "TOTAL taxă dedusă — TVA", format: "money_lei" },
  "R33_2": { label: "TVA de recuperat", format: "money_lei" },
  "R34_2": { label: "TVA de plată", format: "money_lei" },
  "R37_2": { label: "Sold de plată înainte de compensare", format: "money_lei" },
  "R40_2": { label: "Sold de recuperat înainte de compensare", format: "money_lei" },
  "R41_2": { label: "Sold final de plată", format: "money_lei" },
  "R42_2": { label: "Sold final de recuperat", format: "money_lei" },
};

export const D390_FIELDS: LabelDict = {
  "luna": { label: "Luna de raportare", format: "month" },
  "an": { label: "An" },
  "d_rec": { label: "Tip declarație", format: "enum", enumMap: { "0": "Inițială", "1": "Rectificativă" } },
  "nume_declar": { label: "Nume declarant" },
  "prenume_declar": { label: "Prenume declarant" },
  "functie_declar": { label: "Funcție declarant" },
  "cui": { label: "Cod de identificare fiscală (fără prefix RO)", format: "cnp" },
  "den": { label: "Denumire contribuabil" },
  "adresa": { label: "Adresă (stradă, localitate, județ)" },
  "telefon": { label: "Telefon" },
  "mail": { label: "E-mail" },
  "totalPlata_A": { label: "Sumă de control (nrOPI + suma bazelor pe coduri)", format: "money_lei" },
  "nr_pag": { label: "Număr pagină" },
  "nrOPI": { label: "Număr total operațiuni intracomunitare" },
  "bazaL": { label: "Total bază livrări intracomunitare de bunuri (cod L)", format: "money_lei" },
  "bazaT": { label: "Total bază livrări în cadrul operațiunilor triunghiulare (cod T)", format: "money_lei" },
  "bazaA": { label: "Total bază achiziții intracomunitare de bunuri (cod A)", format: "money_lei" },
  "bazaP": { label: "Total bază prestări intracomunitare de servicii (cod P)", format: "money_lei" },
  "bazaS": { label: "Total bază achiziții intracomunitare de servicii (cod S)", format: "money_lei" },
  "bazaR": { label: "Total bază livrări intracomunitare de bunuri în regimul special pentru agricultori (cod R)", format: "money_lei" },
  "total_baza": { label: "Total general baze de impozitare", format: "money_lei" },
  "tip": { label: "Tip operațiune (L/T/A/P/S/R)", format: "enum", enumMap: { "L": "Livrări intracomunitare de bunuri", "T": "Livrări ulterioare în operațiuni triunghiulare", "A": "Achiziții intracomunitare de bunuri", "P": "Prestări intracomunitare de servicii", "S": "Achiziții intracomunitare de servicii", "R": "Livrări intracomunitare de bunuri (regim special agricultori)" } },
  "tara": { label: "Cod țară (stat membru)" },
  "codO": { label: "Cod operator (cod TVA partener)", format: "cnp" },
  "denO": { label: "Denumire operator (partener)" },
  "baza": { label: "Bază de impozitare a operațiunii", format: "money_lei" },
};

export const D394_FIELDS: LabelDict = {
  "luna": { label: "Luna de raportare", format: "month" },
  "an": { label: "Anul de raportare" },
  "tip_D394": { label: "Tip D394 (periodicitate)", format: "enum", enumMap: { "L": "Lunar", "T": "Trimestrial", "S": "Semestrial", "A": "Anual" } },
  "sistemTVA": { label: "Sistem TVA la încasare", format: "enum", enumMap: { "0": "Standard", "1": "La încasare" } },
  "op_efectuate": { label: "Operațiuni efectuate în perioadă", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "cui": { label: "Cod de identificare fiscală (CUI)", format: "cnp" },
  "caen": { label: "Cod CAEN principal" },
  "den": { label: "Denumire declarant" },
  "adresa": { label: "Adresa declarantului" },
  "telefon": { label: "Telefon" },
  "totalPlata_A": { label: "Total de control (R17)", format: "money_lei" },
  "denR": { label: "Denumire reprezentant legal" },
  "functie_reprez": { label: "Funcția reprezentantului" },
  "adresaR": { label: "Adresa reprezentantului" },
  "tip_intocmit": { label: "Tip persoană care a întocmit", format: "enum", enumMap: { "0": "Persoana proprie / reprezentant", "1": "Consultant" } },
  "den_intocmit": { label: "Denumire persoană care a întocmit" },
  "cif_intocmit": { label: "CIF persoană care a întocmit", format: "cnp" },
  "calitate_intocmit": { label: "Calitatea celui care a întocmit" },
  "optiune": { label: "Opțiune regim special", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "prsAfiliat": { label: "Operațiuni cu persoane afiliate", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "nrCui1": { label: "Nr. parteneri pers. înregistrate în scop TVA (distinct, tp=1)" },
  "nrCui2": { label: "Nr. linii pers. juridice neînregistrate (tp=2)" },
  "nrCui3": { label: "Nr. parteneri persoane fizice (distinct, tp=3)" },
  "nrCui4": { label: "Nr. parteneri nerezidenți (distinct, tp=4)" },
  "nr_BF_i1": { label: "Nr. bonuri fiscale (casa de marcat)" },
  "incasari_i1": { label: "Încasări numerar – categoria i1", format: "money_lei" },
  "incasari_i2": { label: "Încasări numerar – categoria i2", format: "money_lei" },
  "nrFacturi_terti": { label: "Nr. facturi emise de terți" },
  "nrFacturi_benef": { label: "Nr. facturi emise de beneficiar" },
  "nrFacturi": { label: "Nr. total facturi de livrare" },
  "nrFacturiL_PF": { label: "Nr. facturi livrări către persoane fizice" },
  "nrFacturiLS_PF": { label: "Nr. facturi livrări scutite către persoane fizice" },
  "val_LS_PF": { label: "Valoare livrări scutite către persoane fizice", format: "money_lei" },
  "solicit": { label: "Solicită rambursare", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "tvaCol24": { label: "TVA colectată cotă 24%", format: "money_lei" },
  "tvaCol21": { label: "TVA colectată cotă 21%", format: "money_lei" },
  "tvaCol11": { label: "TVA colectată cotă 11%", format: "money_lei" },
  "tvaCol20": { label: "TVA colectată cotă 20%", format: "money_lei" },
  "tvaCol19": { label: "TVA colectată cotă 19%", format: "money_lei" },
  "tvaCol9": { label: "TVA colectată cotă 9%", format: "money_lei" },
  "tvaCol5": { label: "TVA colectată cotă 5%", format: "money_lei" },
  "tvaDed24": { label: "TVA deductibilă cotă 24%", format: "money_lei" },
  "tvaDed21": { label: "TVA deductibilă cotă 21%", format: "money_lei" },
  "tvaDed11": { label: "TVA deductibilă cotă 11%", format: "money_lei" },
  "tvaDed20": { label: "TVA deductibilă cotă 20%", format: "money_lei" },
  "tvaDed19": { label: "TVA deductibilă cotă 19%", format: "money_lei" },
  "tvaDed9": { label: "TVA deductibilă cotă 9%", format: "money_lei" },
  "tvaDed5": { label: "TVA deductibilă cotă 5%", format: "money_lei" },
  "tvaDedAI24": { label: "TVA deductibilă achiziții intracomunitare cotă 24%", format: "money_lei" },
  "tvaDedAI21": { label: "TVA deductibilă achiziții intracomunitare cotă 21%", format: "money_lei" },
  "tvaDedAI11": { label: "TVA deductibilă achiziții intracomunitare cotă 11%", format: "money_lei" },
  "tvaDedAI20": { label: "TVA deductibilă achiziții intracomunitare cotă 20%", format: "money_lei" },
  "tvaDedAI19": { label: "TVA deductibilă achiziții intracomunitare cotă 19%", format: "money_lei" },
  "tvaDedAI9": { label: "TVA deductibilă achiziții intracomunitare cotă 9%", format: "money_lei" },
  "tvaDedAI5": { label: "TVA deductibilă achiziții intracomunitare cotă 5%", format: "money_lei" },
  "efectuat": { label: "Operațiuni efectuate (când se solicită)", format: "enum", enumMap: { "0": "Nu", "1": "Da" } },
  "tip": { label: "Tip serie facturi", format: "enum", enumMap: { "1": "Serie de început (deschidere)", "2": "Serie de sfârșit (lot facturi)" } },
  "nrI": { label: "Număr factură de început" },
  "tip_partener": { label: "Tip partener", format: "enum", enumMap: { "1": "Pers. înregistrată în scop TVA", "2": "Pers. juridică neînregistrată", "3": "Persoană fizică", "4": "Nerezident" } },
  "cota": { label: "Cotă TVA", format: "enum", enumMap: { "0": "0%", "5": "5%", "9": "9%", "11": "11%", "19": "19%", "20": "20%", "21": "21%", "24": "24%" } },
  "facturiL": { label: "Nr. facturi livrări taxate (tip L)" },
  "bazaL": { label: "Bază impozabilă livrări taxate", format: "money_lei" },
  "tvaL": { label: "TVA livrări taxate", format: "money_lei" },
  "facturiLS": { label: "Nr. facturi livrări scutite (tip LS)" },
  "bazaLS": { label: "Bază livrări scutite", format: "money_lei" },
  "facturiA": { label: "Nr. facturi achiziții taxate (tip A)" },
  "bazaA": { label: "Bază impozabilă achiziții taxate", format: "money_lei" },
  "tvaA": { label: "TVA achiziții taxate", format: "money_lei" },
  "facturiAI": { label: "Nr. facturi achiziții intracomunitare (tip AI)" },
  "bazaAI": { label: "Bază achiziții intracomunitare", format: "money_lei" },
  "tvaAI": { label: "TVA achiziții intracomunitare", format: "money_lei" },
  "facturiAS": { label: "Nr. facturi achiziții scutite (tip AS)" },
  "bazaAS": { label: "Bază achiziții scutite", format: "money_lei" },
  "facturiV": { label: "Nr. facturi taxare inversă livrare (tip V)" },
  "bazaV": { label: "Bază taxare inversă livrare", format: "money_lei" },
  "facturiC": { label: "Nr. facturi taxare inversă achiziție (tip C)" },
  "bazaC": { label: "Bază taxare inversă achiziție", format: "money_lei" },
  "tvaC": { label: "TVA taxare inversă achiziție (autotaxare)", format: "money_lei" },
  "facturiN": { label: "Nr. facturi categoria N (tp=2, cotă 0)" },
  "document_N": { label: "Nr. documente categoria N" },
  "bazaN": { label: "Bază categoria N", format: "money_lei" },
  "bun": { label: "Cod bun/serviciu art. 331 (nomenclator 21–36)" },
  "nrLivV": { label: "Nr. livrări taxare inversă (V) pentru cod bun" },
  "bazaLivV": { label: "Bază livrări taxare inversă (V) pentru cod bun", format: "money_lei" },
  "nrAchizC": { label: "Nr. achiziții taxare inversă (C) pentru cod bun" },
  "bazaAchizC": { label: "Bază achiziții taxare inversă (C) pentru cod bun", format: "money_lei" },
  "tvaAchizC": { label: "TVA achiziții taxare inversă (C) pentru cod bun", format: "money_lei" },
  "bazaFSLcod": { label: "Bază facturi simplificate livrări – cod", format: "money_lei" },
  "TVAFSLcod": { label: "TVA facturi simplificate livrări – cod", format: "money_lei" },
  "bazaFSL": { label: "Bază facturi simplificate livrări", format: "money_lei" },
  "TVAFSL": { label: "TVA facturi simplificate livrări", format: "money_lei" },
  "bazaFSA": { label: "Bază facturi simplificate achiziții", format: "money_lei" },
  "TVAFSA": { label: "TVA facturi simplificate achiziții", format: "money_lei" },
  "bazaFSAI": { label: "Bază facturi simplificate achiziții intracom.", format: "money_lei" },
  "TVAFSAI": { label: "TVA facturi simplificate achiziții intracom.", format: "money_lei" },
  "bazaBFAI": { label: "Bază bonuri fiscale achiziții intracom.", format: "money_lei" },
  "TVABFAI": { label: "TVA bonuri fiscale achiziții intracom.", format: "money_lei" },
  "nrFacturiL": { label: "Nr. facturi livrări (L+V) pe cotă" },
  "nrFacturiA": { label: "Nr. facturi achiziții (A+C) pe cotă" },
  "nrFacturiAI": { label: "Nr. facturi achiziții intracomunitare pe cotă" },
  "baza_incasari_i1": { label: "Bază încasări casa de marcat i1 (cotă ≠ 24)", format: "money_lei" },
  "tva_incasari_i1": { label: "TVA încasări casa de marcat i1 (cotă ≠ 24)", format: "money_lei" },
  "baza_incasari_i2": { label: "Bază încasări casa de marcat i2 (cotă ≠ 24)", format: "money_lei" },
  "tva_incasari_i2": { label: "TVA încasări casa de marcat i2 (cotă ≠ 24)", format: "money_lei" },
  "bazaL_PF": { label: "Bază livrări către persoane fizice", format: "money_lei" },
  "tvaL_PF": { label: "TVA livrări către persoane fizice", format: "money_lei" },
  "cuiP": { label: "CUI partener", format: "cnp" },
  "denP": { label: "Denumire partener" },
  "nrFact": { label: "Număr de facturi" },
  "baza": { label: "Bază impozabilă", format: "money_lei" },
  "tva": { label: "TVA", format: "money_lei" },
  "nrFactPR": { label: "Nr. facturi pentru cod produs (art. 331)" },
  "codPR": { label: "Cod produs/serviciu art. 331" },
  "bazaPR": { label: "Bază impozabilă pe cod produs", format: "money_lei" },
  "tvaPR": { label: "TVA pe cod produs (autotaxare)", format: "money_lei" },
};

// ── e-Transport (RO e-Transport, UIT) — atribute din generatorul etransport.rs ─────────────────────
const ETRANSPORT_FIELDS: LabelDict = {
  "codDeclarant": { label: "CIF declarant", format: "cnp" },
  "refDeclarant": { label: "Referință internă declarant" },
  "codTipOperatiune": { label: "Tip operațiune (cod)" },
  "codScopOperatiune": { label: "Scop operațiune (cod)" },
  "codTarifar": { label: "Cod tarifar (NC, 8 cifre)" },
  "denumireMarfa": { label: "Denumire marfă" },
  "cantitate": { label: "Cantitate" },
  "codUnitateMasura": { label: "U.M. (cod)" },
  "greutateNeta": { label: "Greutate netă (kg)" },
  "greutateBruta": { label: "Greutate brută (kg)" },
  "valoareLeiFaraTva": { label: "Valoare fără TVA (lei)", format: "money2" },
  "codTara": { label: "Cod țară partener" },
  "cod": { label: "Cod fiscal partener" },
  "denumire": { label: "Denumire partener" },
  "nrVehicul": { label: "Nr. înmatriculare vehicul" },
  "nrRemorca1": { label: "Nr. remorcă 1" },
  "nrRemorca2": { label: "Nr. remorcă 2" },
  "codTaraOrgTransport": { label: "Cod țară organizator transport" },
  "codOrgTransport": { label: "Cod organizator transport" },
  "denumireOrgTransport": { label: "Denumire organizator transport" },
  "dataTransport": { label: "Data transportului", format: "date" },
  "codPtf": { label: "Punct trecere frontieră (cod)" },
  "codBirouVamal": { label: "Birou vamal (cod)" },
  "codJudet": { label: "Județ (cod)" },
  "denumireStrada": { label: "Stradă" },
  "numar": { label: "Număr" },
  "codPostal": { label: "Cod poștal" },
  "alteInfo": { label: "Alte informații (localitate etc.)" },
  "tipDocument": { label: "Tip document transport (cod)" },
  "numarDocument": { label: "Număr document" },
  "dataDocument": { label: "Data document", format: "date" },
};

const DOC_DICTS: Record<string, LabelDict> = {
  D205: D205_FIELDS,
  D112: D112_FIELDS,
  D300: D300_FIELDS,
  D390: D390_FIELDS,
  D394: D394_FIELDS,
  ETRANSPORT: ETRANSPORT_FIELDS,
};

/**
 * Resolve a field's label + formatter for a given document: per-document override → global → raw
 * name fallback. `docKey` is the descriptor key (e.g. "D205"); unknown keys just use the globals.
 */
export function resolveField(docKey: string, attr: string): FieldSpec {
  const doc = DOC_DICTS[docKey];
  return doc?.[attr] ?? GLOBAL_FIELDS[attr] ?? { label: attr };
}

/**
 * CIUS-RO VAT category code → Romanian label, ported from `ubl/pdf.rs::vat_label` so the in-app
 * invoice document and the generated PDF agree. `percent` is the numeric rate (e.g. "21").
 */
export function vatCategoryLabel(code: string, percent: string): string {
  const p = (percent ?? "").trim();
  switch ((code ?? "").trim()) {
    case "S":
      return p ? `${p}%` : "TVA standard";
    case "Z":
      return "0% (cotă zero)";
    case "E":
      return "0% (scutit)";
    case "AE":
      return "0% (taxare inversă)";
    case "K":
      return "0% (intracomunitar)";
    case "G":
      return "0% (export)";
    case "O":
      return "0% (în afara sferei)";
    default:
      return p ? `${p}%` : (code ?? "");
  }
}
