/**
 * Sample data — Romanian SRLs and invoices.
 * Portat din Claude Design (data.jsx).
 *
 * Folosit ca date demo în UI până când conectăm backend-ul real.
 */

export interface SampleCompany {
  id: string;
  cui: string;
  name: string;
  city: string;
  county: string;
  spv: boolean;
  serie: string;
  lastNo: number;
  regCom: string;
  color: string;
}

export interface SampleClient {
  id: string;
  cui: string;
  name: string;
  city: string;
  county: string;
}

export type SampleInvoiceStatus =
  | "validated"
  | "submitted"
  | "pending"
  | "draft"
  | "rejected"
  | "archived"
  | "new"
  | "reviewed"
  | "approved";

export interface SampleInvoiceOut {
  no: string;
  date: string;
  client: SampleClient;
  net: number;
  vat: number;
  total: number;
  status: SampleInvoiceStatus;
  anafId: string | null;
  due: string;
}

export interface SampleInvoiceIn {
  id: string;
  no: string;
  date: string;
  issuer: { cui: string; name: string };
  net: number;
  vat: number;
  total: number;
  status: SampleInvoiceStatus;
  due: string;
  category: string;
  note?: string;
}

export interface AnafEvent {
  time: string;
  kind: "ok" | "info" | "err" | "warn";
  label: string;
  detail: string;
}

export const COMPANIES: SampleCompany[] = [
  { id: "c1",  cui: "RO12345678", name: "ACME România SRL",          city: "București",   county: "B",  spv: true,  serie: "ACME", lastNo: 1247, regCom: "J40/2150/2015", color: "#2848A1" },
  { id: "c2",  cui: "RO98765432", name: "Cluj Tech SRL",             city: "Cluj-Napoca", county: "CJ", spv: true,  serie: "CT",   lastNo: 482,  regCom: "J12/899/2018",  color: "#7C3AED" },
  { id: "c3",  cui: "RO45123987", name: "Iași Software Solutions",   city: "Iași",        county: "IS", spv: true,  serie: "ISS",  lastNo: 1102, regCom: "J22/421/2017",  color: "#0891B2" },
  { id: "c4",  cui: "RO33445566", name: "Mureș Construct SRL",       city: "Târgu Mureș", county: "MS", spv: false, serie: "MC",   lastNo: 218,  regCom: "J26/812/2019",  color: "#D97706" },
  { id: "c5",  cui: "RO11223344", name: "Transilvania Logistics SRL",city: "Brașov",      county: "BV", spv: true,  serie: "TL",   lastNo: 3409, regCom: "J08/1124/2014", color: "#16A34A" },
  { id: "c6",  cui: "RO55667788", name: "Dobrogea Marina SRL",       city: "Constanța",   county: "CT", spv: true,  serie: "DM",   lastNo: 87,   regCom: "J13/302/2021",  color: "#0369A1" },
  { id: "c7",  cui: "RO77889900", name: "Banat Industrial SA",       city: "Timișoara",   county: "TM", spv: false, serie: "BI",   lastNo: 5612, regCom: "J35/441/2008",  color: "#E11D48" },
  { id: "c8",  cui: "RO22334455", name: "Oltenia AgroBiz SRL",       city: "Craiova",     county: "DJ", spv: true,  serie: "OAB",  lastNo: 943,  regCom: "J16/728/2016",  color: "#65A30D" },
  { id: "c9",  cui: "RO66778899", name: "MediaPrint Suceava SRL",    city: "Suceava",     county: "SV", spv: true,  serie: "MPS",  lastNo: 156,  regCom: "J33/210/2020",  color: "#525252" },
  { id: "c10", cui: "RO44556677", name: "Sibiu Restaurante SRL",     city: "Sibiu",       county: "SB", spv: true,  serie: "SR",   lastNo: 2110, regCom: "J32/87/2013",   color: "#B45309" },
  { id: "c11", cui: "RO88990011", name: "Maramureș Lemn SRL",        city: "Baia Mare",   county: "MM", spv: false, serie: "ML",   lastNo: 312,  regCom: "J24/501/2019",  color: "#4D7C0F" },
  { id: "c12", cui: "RO99001122", name: "Argeș AutoParts SRL",       city: "Pitești",     county: "AG", spv: true,  serie: "AAP",  lastNo: 1856, regCom: "J03/611/2012",  color: "#1D4ED8" },
  { id: "c13", cui: "RO33221100", name: "Buzău Confecții SRL",       city: "Buzău",       county: "BZ", spv: true,  serie: "BC",   lastNo: 678,  regCom: "J10/229/2017",  color: "#9333EA" },
  { id: "c14", cui: "RO77665544", name: "Galați Steel SRL",          city: "Galați",      county: "GL", spv: true,  serie: "GS",   lastNo: 4203, regCom: "J17/812/2009",  color: "#475569" },
  { id: "c15", cui: "RO12121212", name: "Prahova IT Consulting",     city: "Ploiești",    county: "PH", spv: true,  serie: "PIT",  lastNo: 502,  regCom: "J29/1106/2018", color: "#0D9488" },
];

export const CLIENTS: SampleClient[] = [
  { id: "k1",  cui: "RO27543210", name: "Globex Distribuție SRL",      city: "București",    county: "B"  },
  { id: "k2",  cui: "RO31998877", name: "Hyperion Software SRL",       city: "Cluj-Napoca",  county: "CJ" },
  { id: "k3",  cui: "RO19283746", name: "Stark Industries România SA", city: "Pitești",      county: "AG" },
  { id: "k4",  cui: "RO84736251", name: "Wayne Logistics SRL",         city: "Constanța",    county: "CT" },
  { id: "k5",  cui: "RO62718394", name: "Wonka Confiserie SRL",        city: "Brașov",       county: "BV" },
  { id: "k6",  cui: "RO49382716", name: "Initech Services SRL",        city: "Timișoara",    county: "TM" },
  { id: "k7",  cui: "RO74859302", name: "Umbrella Pharma SRL",         city: "Iași",         county: "IS" },
  { id: "k8",  cui: "RO38271946", name: "Cyberdyne Sisteme SRL",       city: "Sibiu",        county: "SB" },
  { id: "k9",  cui: "RO15263748", name: "Tyrell Bio SRL",              city: "București",    county: "B"  },
  { id: "k10", cui: "RO95837461", name: "Massive Dynamic SRL",         city: "Cluj-Napoca",  county: "CJ" },
  { id: "k11", cui: "RO20394857", name: "Soylent Foods SRL",           city: "Galați",       county: "GL" },
  { id: "k12", cui: "RO38475612", name: "Oscorp România SRL",          city: "Oradea",       county: "BH" },
];

export const INVOICES_OUT: SampleInvoiceOut[] = [
  { no: "ACME-0001247", date: "2026-05-19", client: CLIENTS[0],  net: 14250.00, vat: 2707.50, total: 16957.50, status: "validated", anafId: "5093712441", due: "2026-06-18" },
  { no: "ACME-0001246", date: "2026-05-19", client: CLIENTS[1],  net:  3800.00, vat:  722.00, total:  4522.00, status: "submitted", anafId: "5093712219", due: "2026-06-18" },
  { no: "ACME-0001245", date: "2026-05-18", client: CLIENTS[2],  net: 87500.00, vat: 16625.00, total: 104125.00, status: "validated", anafId: "5093698220", due: "2026-06-17" },
  { no: "ACME-0001244", date: "2026-05-18", client: CLIENTS[3],  net:  2150.50, vat:  408.60, total:  2559.10, status: "rejected",  anafId: "5093697112", due: "2026-06-17" },
  { no: "ACME-0001243", date: "2026-05-17", client: CLIENTS[4],  net:   980.00, vat:  186.20, total:  1166.20, status: "validated", anafId: "5093680033", due: "2026-06-16" },
  { no: "ACME-0001242", date: "2026-05-17", client: CLIENTS[5],  net: 12400.00, vat: 2356.00, total: 14756.00, status: "draft",     anafId: null,         due: "—"          },
  { no: "ACME-0001241", date: "2026-05-16", client: CLIENTS[6],  net:  5670.00, vat: 1077.30, total:  6747.30, status: "validated", anafId: "5093668001", due: "2026-06-15" },
  { no: "ACME-0001240", date: "2026-05-16", client: CLIENTS[7],  net: 21000.00, vat: 3990.00, total: 24990.00, status: "validated", anafId: "5093667890", due: "2026-06-15" },
  { no: "ACME-0001239", date: "2026-05-15", client: CLIENTS[8],  net:  4200.00, vat:  798.00, total:  4998.00, status: "pending",   anafId: null,         due: "2026-06-14" },
  { no: "ACME-0001238", date: "2026-05-15", client: CLIENTS[9],  net: 17800.00, vat: 3382.00, total: 21182.00, status: "validated", anafId: "5093649112", due: "2026-06-14" },
  { no: "ACME-0001237", date: "2026-05-14", client: CLIENTS[10], net:   620.00, vat:  117.80, total:   737.80, status: "draft",     anafId: null,         due: "—"          },
  { no: "ACME-0001236", date: "2026-05-14", client: CLIENTS[11], net:  9450.00, vat: 1795.50, total: 11245.50, status: "validated", anafId: "5093629204", due: "2026-06-13" },
  { no: "ACME-0001235", date: "2026-05-13", client: CLIENTS[0],  net:  3300.00, vat:  627.00, total:  3927.00, status: "validated", anafId: "5093610002", due: "2026-06-12" },
  { no: "ACME-0001234", date: "2026-05-13", client: CLIENTS[1],  net: 11200.00, vat: 2128.00, total: 13328.00, status: "submitted", anafId: "5093609881", due: "2026-06-12" },
  { no: "ACME-0001233", date: "2026-05-12", client: CLIENTS[2],  net:  1400.00, vat:  266.00, total:  1666.00, status: "validated", anafId: "5093589771", due: "2026-06-11" },
];

export const ANAF_EVENTS: AnafEvent[] = [
  { time: "12:34:09", kind: "ok",   label: "Validată e-Factura",        detail: "Răspuns ANAF • index 5093712441 • XML semnat" },
  { time: "12:33:42", kind: "info", label: "Trimisă către ANAF",        detail: "Mesaj 5093712441 • Mod fiscal: TVA la încasare" },
  { time: "12:33:38", kind: "info", label: "Generare XML (RO_CIUS)",    detail: "Schema CIUS-RO 1.0.1 • 14 linii, 21 atribute" },
  { time: "12:29:11", kind: "ok",   label: "Validare RO_CIUS locală OK",detail: "Toate câmpurile obligatorii completate" },
  { time: "11:58:02", kind: "info", label: "Factură deschisă pentru editare", detail: "User: D. Popescu" },
  { time: "11:57:48", kind: "info", label: "Creare factură nouă",       detail: "Serie: ACME • Nr: 1247 • Tip: F (vânzare bunuri)" },
];

// ─── Facturi primite (received) ──────────────────────────────────────────

export const INVOICES_IN: SampleInvoiceIn[] = [
  { id: "in1",  no: "FF-2026-00873", date: "2026-05-19", issuer: { cui: "RO15998877", name: "RCS-RDS Servicii SA" },        net:  4350.00, vat:  826.50, total:  5176.50, status: "new",      due: "2026-06-18", category: "—" },
  { id: "in2",  no: "ENG-19-882441", date: "2026-05-19", issuer: { cui: "RO13267117", name: "Engie România SA" },           net:  2890.30, vat:  549.16, total:  3439.46, status: "new",      due: "2026-06-18", category: "Utilități" },
  { id: "in3",  no: "ORF-118022",    date: "2026-05-18", issuer: { cui: "RO5888777",  name: "Orange România SA" },           net:   780.00, vat:  148.20, total:   928.20, status: "reviewed", due: "2026-06-17", category: "Telecom" },
  { id: "in4",  no: "B-2026-441",    date: "2026-05-18", issuer: { cui: "RO19283746", name: "BCR Servicii Financiare SRL" }, net:  1200.00, vat:  228.00, total:  1428.00, status: "approved", due: "2026-06-17", category: "Financiar" },
  { id: "in5",  no: "F-9912-2026",   date: "2026-05-17", issuer: { cui: "RO27543210", name: "Globex Distribuție SRL" },      net: 14200.00, vat: 2698.00, total: 16898.00, status: "approved", due: "2026-06-16", category: "Marfă" },
  { id: "in6",  no: "FF-2026-00854", date: "2026-05-17", issuer: { cui: "RO11876543", name: "Selgros Cash & Carry SRL" },    net:   620.45, vat:  117.89, total:   738.34, status: "approved", due: "2026-06-16", category: "Marfă" },
  { id: "in7",  no: "DED-7711",      date: "2026-05-16", issuer: { cui: "RO84736251", name: "Wayne Logistics SRL" },         net:  3200.00, vat:  608.00, total:  3808.00, status: "rejected", due: "2026-06-15", category: "Servicii", note: "Refacturare incorectă — CUI emitent" },
  { id: "in8",  no: "AUTO-2026-118", date: "2026-05-16", issuer: { cui: "RO99001122", name: "Argeș AutoParts SRL" },         net:  8900.00, vat: 1691.00, total: 10591.00, status: "new",      due: "2026-06-15", category: "Marfă" },
  { id: "in9",  no: "CT-2026-228",   date: "2026-05-15", issuer: { cui: "RO62718394", name: "Wonka Confiserie SRL" },        net:   450.00, vat:   85.50, total:   535.50, status: "reviewed", due: "2026-06-14", category: "Marfă" },
  { id: "in10", no: "ELEC-7811302",  date: "2026-05-14", issuer: { cui: "RO13267223", name: "Enel Energie SA" },            net:  3120.40, vat:  592.88, total:  3713.28, status: "approved", due: "2026-06-13", category: "Utilități" },
  { id: "in11", no: "FF-2026-00712", date: "2026-05-13", issuer: { cui: "RO77889900", name: "Banat Industrial SA" },         net:  9870.00, vat: 1875.30, total: 11745.30, status: "archived", due: "2026-06-12", category: "Marfă" },
  { id: "in12", no: "OFC-22198",     date: "2026-05-12", issuer: { cui: "RO94871221", name: "OfficeMax România SRL" },        net:  1480.00, vat:  281.20, total:  1761.20, status: "approved", due: "2026-06-11", category: "Birotică" },
];

// ─── Validare RO_CIUS (demo pentru editorul de factură) ───────────────────

export interface ValidationItem {
  kind: "ok" | "err" | "warn";
  title: string;
  desc: string;
  fix?: string;
}

export const VALIDATION: { score: number; items: ValidationItem[] } = {
  score: 86,
  items: [
    { kind: "ok",   title: "CUI cumpărător",        desc: "RO27543210 — verificat în registrul ANAF acum 9 sec" },
    { kind: "ok",   title: "Nr. factură unic",      desc: "ACME-0001247 nu mai există în arhivă" },
    { kind: "ok",   title: "Data emiterii ≤ data scadenței", desc: "19.05.2026 → 18.06.2026 (30 zile)" },
    { kind: "ok",   title: "Cota TVA per linie",    desc: "Toate liniile au cotă fiscală validă (19%, 9% sau 5%)" },
    { kind: "err",  title: "Adresa cumpărător incompletă", desc: "Lipsește codul județului (cod ISO 3166-2:RO).", fix: "Completează din ANAF" },
    { kind: "err",  title: "TVA cumpărător neînregistrat la 19.05.2026", desc: "Conform e-Factura, plătitor TVA doar până la 30.04.2026. Factura va fi respinsă.", fix: "Schimbă în neplătitor" },
    { kind: "warn", title: "Cod produs lipsă pe linia 3", desc: "Recomandat pentru declarația D394.", fix: "Sugerează coduri" },
    { kind: "ok",   title: "Total = Σ linii + TVA", desc: "Calcul corect: 14.250,00 + 2.707,50 = 16.957,50 RON" },
  ],
};

// ─── Vizualizări salvate (saved views pentru lista de facturi) ────────────

export interface SavedView {
  id: string;
  label: string;
  count: number;
  active: boolean;
}

export const SAVED_VIEWS: SavedView[] = [
  { id: "all",      label: "Toate",            count: 1247, active: false },
  { id: "today",    label: "Astăzi",           count: 14,   active: false },
  { id: "month",    label: "Mai 2026",         count: 187,  active: true },
  { id: "draft",    label: "Schițe",           count: 9,    active: false },
  { id: "rejected", label: "Respinse de ANAF", count: 3,    active: false },
  { id: "overdue",  label: "Restante",         count: 11,   active: false },
  { id: "highval",  label: "Peste 10.000 RON", count: 42,   active: false },
];

// ─── Linii demo pentru editorul de factură nouă ──────────────────────────

export interface SampleLine {
  id: number;
  code: string;
  desc: string;
  qty: number;
  um: string;
  price: number;
  vat: number;
}

export const SAMPLE_LINES: SampleLine[] = [
  { id: 1, code: "SRV-001",    desc: "Servicii consultanță IT — mai 2026",    qty: 80, um: "ore", price: 150.0, vat: 19 },
  { id: 2, code: "LIC-OFFICE", desc: "Licență anuală Microsoft 365 Business", qty: 5,  um: "buc", price: 285.0, vat: 19 },
  { id: 3, code: "—",          desc: "Decont transport delegație Brașov",     qty: 1,  um: "buc", price: 420.0, vat: 19 },
];

/** Vânzări demo pentru tabelul "Companii administrate" în dashboard. */
export const COMPANY_SALES_MAY: number[] = [
  47280, 88100, 14290, 28100, 110450, 4220, 192480, 8400,
];
/** Numărul de alerte pe coloana Alertă (NaN = "—"). */
export const COMPANY_ALERTS: number[] = [0, 3, 0, 1, 0, 0, 2, 0];

/** Helper pentru formatare RON cu locale ro-RO. */
export function fmtRON(n: number): string {
  return n.toLocaleString("ro-RO", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}
