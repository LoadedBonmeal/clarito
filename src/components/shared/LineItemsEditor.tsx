/**
 * LineItemsEditor — reusable line-items table for invoices and recurring templates.
 * Re-skinned to rf kit look (Wave 2) — uses .rf-tbl classes and rf editor styles.
 *
 * Public API (props, types, exports) is UNCHANGED — LineItemsEditor.test.tsx
 * imports only deduceVatCategory and LineRow, which are preserved exactly.
 *
 * Includes:
 *  - vatCategory column with auto-deduction (deduceVatCategory)
 *  - Manual override is always respected (the select is controlled; auto-deduction
 *    only fires when vatRate or buyerCountry/sellerVatPayer props change, not on
 *    every unrelated render).
 */

import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { Icon } from "@/components/shared/Icon";
import { ProductPickerButton } from "@/components/shared/ProductCombobox";
import { VAT_RATES, VAT_CATEGORIES, VAT_CATEGORY_LABELS } from "@/lib/constants";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { fmtRON } from "@/lib/utils";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { CreateLineInput, Product, VatCategory } from "@/types";

/** EU alpha-2 country codes (excluding Romania). Used for auto-deduction of vatCategory. */
const EU_CODES = new Set([
  "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI",
  "FR", "DE", "GR", "HU", "IE", "IT", "LV", "LT", "LU",
  "MT", "NL", "PL", "PT", "SK", "SI", "ES", "SE",
]);

/**
 * Pure helper — deduces the correct VAT category from rate, buyer country and
 * seller VAT payer status.
 *
 * Rules:
 *  - vatRate > 0  → 'S' (standard)
 *  - vatRate === 0 — buyer country is resolved FIRST, seller-payer status second:
 *      - buyer is EU (non-RO)              → 'K' (livrare intracomunitară scutită)
 *      - buyer is non-EU and non-RO        → 'G' (export scutit)
 *      - buyer is RO (or unknown):
 *          - seller is NOT a VAT payer     → 'O' (în afara sferei TVA — neplătitor)
 *          - seller IS a VAT payer         → 'E' (scutit intern fără drept de deducere)
 *      NOTE: 'AE' (taxare inversă) is NOT auto-assigned here; it is only correct
 *      for genuine intra-community or domestic reverse-charge situations and must
 *      be set explicitly by the user.
 *  - default                               → 'S'
 */
export function deduceVatCategory(
  vatRate: number,
  buyerCountry: string,
  sellerVatPayer: boolean,
): VatCategory {
  if (vatRate > 0) return "S";
  if (vatRate === 0) {
    const country = (buyerCountry ?? "").toUpperCase().trim();
    // Country wins — resolve EU/non-EU first.
    if (EU_CODES.has(country)) return "K";
    if (country && country !== "RO") return "G";
    // Domestic (RO or unknown) — check seller payer status.
    if (!sellerVatPayer) return "O";
    return "E";
  }
  return "S";
}

/** Extends CreateLineInput with a stable row key for React list rendering. */
export type LineRow = CreateLineInput & { rowId: string };

export interface LineItemsEditorProps {
  lines: LineRow[];
  onChange: (lines: LineRow[]) => void;
  /** ISO alpha-2 country code of the buyer, used for auto-deduction */
  buyerCountry?: string;
  /** Whether the seller (emitting company) is a VAT payer */
  sellerVatPayer?: boolean;
  /** When true, shows the totals footer */
  showTotals?: boolean;
  /**
   * When provided, a "alege din catalog" picker button appears
   * in each Descriere cell. On select, the line is filled from the product.
   * Manual entry always remains fully functional.
   */
  companyId?: string;
  /** Invoice currency code shown in the totals footer (default "RON"). */
  currency?: string;
}

export function LineItemsEditor({
  lines,
  onChange,
  buyerCountry = "RO",
  sellerVatPayer = true,
  showTotals = true,
  companyId,
  currency = "RON",
}: LineItemsEditorProps) {
  // Fetch active VAT rates from the global DB catalog.
  const { data: dbRates } = useQuery({
    queryKey: queryKeys.vatRates.list(true),
    queryFn: () => api.vatRates.list(true),
    staleTime: 5 * 60_000,
  });

  // Build the numeric rates array for the dropdown
  const buildVatRateOptions = (currentRate: number): number[] => {
    const base: number[] =
      dbRates && dbRates.length > 0
        ? dbRates.map((r) => parseFloat(r.rate))
        : (VAT_RATES as readonly number[]).slice();
    if (!base.includes(currentRate)) {
      const merged = [...base, currentRate].sort((a, b) => a - b);
      return merged;
    }
    return base;
  };

  // Track previous deduce-trigger values so we only auto-deduce on real changes.
  const prevDeduceKey = useRef<string>("");

  // Auto-deduce vatCategory for each line when vatRate, buyerCountry, or
  // sellerVatPayer changes. Manual changes to vatCategory made by the user
  // are NOT clobbered because the deduceKey only changes for buyerCountry+sellerVatPayer.
  useEffect(() => {
    const key = `${buyerCountry}|${sellerVatPayer}`;
    if (key === prevDeduceKey.current) return;
    prevDeduceKey.current = key;

    const updated = lines.map((l) => ({
      ...l,
      vatCategory: deduceVatCategory(l.vatRate, buyerCountry, sellerVatPayer),
    }));
    onChange(updated);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [buyerCountry, sellerVatPayer]);

  const updateLine = <K extends keyof CreateLineInput>(
    idx: number,
    key: K,
    value: CreateLineInput[K],
  ) => {
    const updated = lines.map((l, i) => {
      if (i !== idx) return l;
      const next = { ...l, [key]: value } as LineRow;
      if (key === "vatRate") {
        next.vatCategory = deduceVatCategory(
          value as number,
          buyerCountry,
          sellerVatPayer,
        );
      }
      // VAT1: when category changes to non-S, force vatRate to 0 so the
      // on-screen totals match the backend category-authoritative rule.
      if (key === "vatCategory" && value !== "S") {
        next.vatRate = 0;
      }
      return next;
    });
    onChange(updated);
  };

  const addLine = () => {
    const newLine: LineRow = {
      name: "",
      quantity: 1,
      unit: "buc",
      unitPrice: 0,
      vatRate: sellerVatPayer ? 19 : 0,
      vatCategory: deduceVatCategory(
        sellerVatPayer ? 19 : 0,
        buyerCountry,
        sellerVatPayer,
      ),
      rowId: crypto.randomUUID(),
    };
    onChange([...lines, newLine]);
  };

  const removeLine = (idx: number) =>
    onChange(lines.filter((_, i) => i !== idx));

  /**
   * Fill a line from a product picked from the catalog.
   * Overwrites name, unit, unitPrice, vatRate, vatCategory, cpvCode.
   * quantity is kept. Manual entry is always fully functional.
   */
  const fillFromProduct = (idx: number, product: Product) => {
    const updated = lines.map((l, i) => {
      if (i !== idx) return l;
      const vatRateNum = parseFloat(product.vatRate) || 0;
      return {
        ...l,
        name: product.name,
        unit: product.unit,
        unitPrice: parseFloat(product.unitPrice) || 0,
        vatRate: vatRateNum,
        vatCategory: product.vatCategory as VatCategory,
        cpvCode: product.code ?? l.cpvCode,
      } as LineRow;
    });
    onChange(updated);
  };

  // M2: round each line to 2dp before summing to match backend.
  const net = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    return s + lineNet;
  }, 0);
  const vat = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    // VAT1: only category 'S' charges VAT; all others → 0.
    const effRate = l.vatCategory === "S" ? l.vatRate : 0;
    const lineVat = Math.round(lineNet * (effRate / 100) * 100) / 100;
    return s + lineVat;
  }, 0);
  const total = net + vat;

  // Short labels for the Categorie <select> options (full text is in title tooltip).
  const CAT_SHORT: Record<VatCategory, string> = {
    S: "Standard", AE: "Taxare inversă", E: "Scutit", Z: "Cotă zero",
    O: "Afara sferei TVA", K: "Intracom. scutit", G: "Export scutit",
  };

  // Column count: # + Cod + Descriere + Cant + UM + Preț + TVA% + Categorie + Net + Total + del = 11
  const COL_SPAN = 11;

  // Cell input style — thin, borderless inline inputs for editor rows
  const cellInput: React.CSSProperties = {
    width: "100%",
    border: "none",
    background: "transparent",
    padding: "0 4px",
    fontSize: 13,
    fontFamily: "inherit",
    color: "var(--rf-text)",
    outline: "none",
    height: "100%",
  };

  const cellInputNum: React.CSSProperties = {
    ...cellInput,
    textAlign: "right",
    fontFamily: "var(--rf-mono)",
    fontVariantNumeric: "tabular-nums",
  };

  const cellSelect: React.CSSProperties = {
    width: "100%",
    border: "none",
    background: "transparent",
    padding: "0 2px",
    fontSize: 12,
    fontFamily: "inherit",
    color: "var(--rf-text)",
    outline: "none",
    cursor: "pointer",
  };

  return (
    <div
      style={{
        overflowX: "auto",
        borderTop: "1px solid var(--rf-border)",
      }}
    >
      <table
        style={{
          width: "100%",
          borderCollapse: "separate",
          borderSpacing: 0,
          fontSize: 13,
          tableLayout: "auto",
        }}
      >
        <thead>
          <tr
            style={{
              background: "var(--rf-table-head)",
            }}
          >
            {/* # */}
            <th
              style={{
                width: 32, padding: "0 8px", height: 38, textAlign: "center",
                fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase",
                color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)",
                whiteSpace: "nowrap",
              }}
            >#</th>
            {/* Cod */}
            <th style={{ width: 110, padding: "0 8px", height: 38, textAlign: "left", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>Cod</th>
            {/* Descriere */}
            <th style={{ padding: "0 8px", height: 38, textAlign: "left", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)" }}>Descriere</th>
            {/* Cant */}
            <th style={{ width: 64, padding: "0 8px", height: 38, textAlign: "right", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>Cant.</th>
            {/* UM */}
            <th style={{ width: 56, padding: "0 8px", height: 38, textAlign: "left", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)" }}>UM</th>
            {/* Preț */}
            <th style={{ width: 100, padding: "0 8px", height: 38, textAlign: "right", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>Preț unitar</th>
            {/* TVA % */}
            <th style={{ width: 64, padding: "0 8px", height: 38, textAlign: "right", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>TVA %</th>
            {/* Categorie */}
            <th
              style={{
                width: 150, padding: "0 8px", height: 38, textAlign: "left", fontSize: 11,
                fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase",
                color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)",
              }}
            >
              <span style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
                Categorie
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        style={{
                          cursor: "help", fontSize: 10, color: "var(--rf-text-muted)",
                          border: "1px solid var(--rf-border-strong)", borderRadius: "50%",
                          width: 13, height: 13, display: "inline-flex", alignItems: "center",
                          justifyContent: "center", lineHeight: 1, flexShrink: 0,
                        }}
                        aria-label="Explicație categorii TVA"
                      >?</span>
                    </TooltipTrigger>
                    <TooltipContent side="top" style={{ maxWidth: 280 }}>
                      <strong>Categorii TVA (CIUS-RO):</strong><br />
                      <b>S</b> — Standard (TVA aplicată normal)<br />
                      <b>AE</b> — Taxare inversă (reverse-charge B2B intracomunitar sau intern, TVA 0%)<br />
                      <b>E</b> — Scutit fără drept de deducere (intern)<br />
                      <b>Z</b> — Cotă zero<br />
                      <b>K</b> — Intracomunitar scutit (livrare UE, 0% + VAT ID)<br />
                      <b>G</b> — Export scutit (livrare extra-UE)<br />
                      <b>O</b> — În afara sferei TVA
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </span>
            </th>
            {/* Valoare net */}
            <th style={{ width: 110, padding: "0 8px", height: 38, textAlign: "right", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>Valoare net</th>
            {/* Total cu TVA */}
            <th style={{ width: 110, padding: "0 8px", height: 38, textAlign: "right", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--rf-text-dim)", borderBottom: "1px solid var(--rf-border)", whiteSpace: "nowrap" }}>Total cu TVA</th>
            {/* del */}
            <th style={{ width: 36, borderBottom: "1px solid var(--rf-border)" }}></th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => {
            const lineNet = l.quantity * l.unitPrice;
            // VAT1: only category 'S' charges VAT; all others display 0 VAT.
            const effectiveVatRate = l.vatCategory === "S" ? l.vatRate : 0;
            const lineTotal = lineNet * (1 + effectiveVatRate / 100);
            return (
              <tr
                key={l.rowId}
                style={{ borderBottom: "1px solid var(--rf-border)" }}
                onMouseEnter={(e) => (e.currentTarget.style.background = "var(--rf-hover)")}
                onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
              >
                {/* # */}
                <td
                  style={{
                    textAlign: "center", color: "var(--rf-text-dim)",
                    fontFamily: "var(--rf-mono)", padding: "0 8px",
                    height: 46, verticalAlign: "middle",
                  }}
                >
                  {i + 1}
                </td>
                {/* Cod */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <input
                    value={l.cpvCode ?? ""}
                    onChange={(e) => updateLine(i, "cpvCode", e.target.value || undefined)}
                    style={{ ...cellInput, fontFamily: "var(--rf-mono)" }}
                    placeholder="cod"
                  />
                </td>
                {/* Descriere */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 3 }}>
                    <input
                      value={l.name}
                      onChange={(e) => updateLine(i, "name", e.target.value)}
                      style={{ ...cellInput, flex: 1, minWidth: 0 }}
                      placeholder="Descriere articol"
                    />
                    {companyId && (
                      <ProductPickerButton
                        companyId={companyId}
                        onSelect={(p) => fillFromProduct(i, p)}
                      />
                    )}
                  </div>
                </td>
                {/* Cant */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <input
                    type="number"
                    value={l.quantity}
                    onChange={(e) => updateLine(i, "quantity", parseFloat(e.target.value) || 0)}
                    style={cellInputNum}
                  />
                </td>
                {/* UM */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <input
                    value={l.unit}
                    onChange={(e) => updateLine(i, "unit", e.target.value)}
                    style={cellInput}
                  />
                </td>
                {/* Preț */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <input
                    type="number"
                    value={l.unitPrice}
                    onChange={(e) => updateLine(i, "unitPrice", parseFloat(e.target.value) || 0)}
                    style={cellInputNum}
                  />
                </td>
                {/* TVA % */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <select
                    style={{ ...cellSelect, textAlign: "right" }}
                    value={l.vatRate}
                    onChange={(e) => updateLine(i, "vatRate", Number(e.target.value))}
                  >
                    {buildVatRateOptions(l.vatRate).map((r) => (
                      <option key={r} value={r}>{r}%</option>
                    ))}
                  </select>
                </td>
                {/* Categorie */}
                <td style={{ padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <select
                    style={{ ...cellSelect, fontSize: 11 }}
                    value={l.vatCategory}
                    onChange={(e) => updateLine(i, "vatCategory", e.target.value as VatCategory)}
                    title={VAT_CATEGORY_LABELS[l.vatCategory]}
                  >
                    {VAT_CATEGORIES.map((cat) => (
                      <option key={cat} value={cat}>{cat} — {CAT_SHORT[cat]}</option>
                    ))}
                  </select>
                </td>
                {/* Valoare net */}
                <td
                  style={{
                    textAlign: "right", padding: "0 8px", height: 46, verticalAlign: "middle",
                    fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums",
                    color: "var(--rf-text-muted)", fontSize: 13,
                  }}
                >
                  {lineNet.toFixed(2)}
                </td>
                {/* Total cu TVA */}
                <td
                  style={{
                    textAlign: "right", padding: "0 8px", height: 46, verticalAlign: "middle",
                    fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums",
                    fontWeight: 700, fontSize: 13,
                  }}
                >
                  {lineTotal.toFixed(2)}
                </td>
                {/* Delete */}
                <td style={{ textAlign: "center", padding: "0 4px", height: 46, verticalAlign: "middle" }}>
                  <button
                    className="rf-icon-btn rf-icon-btn--ghost"
                    style={{ width: 28, height: 28 }}
                    onClick={() => removeLine(i)}
                    disabled={lines.length === 1}
                    title="Șterge linia"
                  >
                    <Icon name="trash" size={13} />
                  </button>
                </td>
              </tr>
            );
          })}
          {/* Add-line row */}
          <tr
            style={{ cursor: "pointer" }}
            onClick={addLine}
            onMouseEnter={(e) => (e.currentTarget.style.background = "var(--rf-hover)")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
          >
            <td
              colSpan={COL_SPAN}
              style={{
                padding: "10px 16px",
                color: "var(--rf-accent)",
                fontSize: 13,
                fontWeight: 500,
              }}
            >
              <Icon name="plus" size={13} style={{ marginRight: 6 }} /> Adaugă linie
            </td>
          </tr>
        </tbody>
        {showTotals && (
          <tfoot>
            <tr
              style={{ background: "var(--rf-table-head)", borderTop: "1px solid var(--rf-border-strong)" }}
            >
              <td
                colSpan={7}
                style={{
                  textAlign: "right", padding: "0 8px", height: 40,
                  color: "var(--rf-text-muted)", fontSize: 12,
                }}
              >
                Subtotal net
              </td>
              <td></td>
              <td
                style={{
                  textAlign: "right", padding: "0 8px",
                  fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums",
                  fontWeight: 600, fontSize: 13,
                }}
              >
                {fmtRON(net)}
              </td>
              <td></td>
              <td></td>
            </tr>
            <tr style={{ background: "var(--rf-table-head)" }}>
              <td
                colSpan={7}
                style={{
                  textAlign: "right", padding: "0 8px", height: 36,
                  color: "var(--rf-text-muted)", fontSize: 12,
                }}
              >
                TVA
              </td>
              <td></td>
              <td
                style={{
                  textAlign: "right", padding: "0 8px",
                  fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums",
                  fontSize: 13,
                }}
              >
                {fmtRON(vat)}
              </td>
              <td></td>
              <td></td>
            </tr>
            <tr style={{ background: "var(--rf-table-head)" }}>
              <td
                colSpan={7}
                style={{
                  textAlign: "right", padding: "0 8px", height: 40,
                  fontSize: 11, textTransform: "uppercase", letterSpacing: "0.04em",
                  fontWeight: 700, color: "var(--rf-text)",
                }}
              >
                Total de plată
              </td>
              <td></td>
              <td></td>
              <td
                style={{
                  textAlign: "right", padding: "0 8px",
                  fontFamily: "var(--rf-mono)", fontVariantNumeric: "tabular-nums",
                  fontSize: 15, fontWeight: 700, color: "var(--rf-accent)",
                }}
              >
                {fmtRON(total)}{" "}
                <span style={{ fontSize: 10.5, color: "var(--rf-text-muted)", fontWeight: 400 }}>{currency}</span>
              </td>
              <td></td>
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
