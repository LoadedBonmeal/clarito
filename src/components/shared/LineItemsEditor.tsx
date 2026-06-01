/**
 * LineItemsEditor — reusable line-items table for invoices and recurring templates.
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
   * R15 Wave 1: When provided, a "alege din catalog" picker button appears
   * in each Descriere cell. On select, the line is filled from the product.
   * Manual entry always remains fully functional.
   */
  companyId?: string;
}

export function LineItemsEditor({
  lines,
  onChange,
  buyerCountry = "RO",
  sellerVatPayer = true,
  showTotals = true,
  companyId,
}: LineItemsEditorProps) {
  // R15 Wave 2: Fetch active VAT rates from the global DB catalog.
  // Falls back to the VAT_RATES constant when loading or on error so the
  // editor always works even during initial load or if the backend is
  // unavailable. The query is global (not company-scoped) — see db/vat_rates.rs.
  const { data: dbRates } = useQuery({
    queryKey: queryKeys.vatRates.list(true),
    queryFn: () => api.vatRates.list(true),
    staleTime: 5 * 60_000,
  });

  // Build the numeric rates array for the dropdown: prefer DB rates (sorted
  // by sort_order / rate), fall back to the hardcoded constant.
  // F3: union the current line's vatRate into the options so that deactivated
  // rates don't cause a blank <select> when editing old invoices.
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
  // are NOT clobbered because: after manual change the user's value is stored
  // in lines[i].vatCategory, and auto-deduction only fires when the
  // deduceKey (buyerCountry+sellerVatPayer) changes — at that point we
  // re-compute from the rate. If the user just edited the select directly,
  // that line's onChange already called onChange(updatedLines) with the new
  // vatCategory, so this effect does nothing (key hasn't changed).
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
      // When vatRate changes, auto-deduce category unless user is explicitly
      // changing vatCategory directly (key === "vatCategory").
      if (key === "vatRate") {
        next.vatCategory = deduceVatCategory(
          value as number,
          buyerCountry,
          sellerVatPayer,
        );
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
   * R15 Wave 1: Fill a line from a product picked from the catalog.
   * Overwrites name, unit, unitPrice, vatRate, vatCategory, cpvCode(=code).
   * quantity is kept (user may have already typed one). Manual entry is
   * always fully functional — this is a convenience only.
   * Does NOT link the line to the product id (lines stay free-text as today).
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

  // M2: round each line to 2dp before summing to match the backend's
  // round-then-sum approach, so the displayed totals match stored values.
  const net = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    return s + lineNet;
  }, 0);
  const vat = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    const lineVat = Math.round(lineNet * (l.vatRate / 100) * 100) / 100;
    return s + lineVat;
  }, 0);
  const total = net + vat;

  // Column count: # + Cod + Descriere + Cant + UM + Preț + TVA% + Categorie + Net + Total + del = 11
  const COL_SPAN = 11;

  return (
    <div className="line-items">
      <table>
        <thead>
          <tr>
            <th style={{ width: 28 }}>#</th>
            <th style={{ width: 110 }}>Cod</th>
            <th>Descriere</th>
            <th style={{ width: 64 }} className="num">Cant.</th>
            <th style={{ width: 56 }}>UM</th>
            <th style={{ width: 100 }} className="num">Preț unitar</th>
            <th style={{ width: 64 }} className="num">TVA %</th>
            <th style={{ width: 110 }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
                Categorie
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        style={{
                          cursor: "help",
                          fontSize: 10,
                          color: "var(--text-muted)",
                          border: "1px solid var(--text-dim, #aaa)",
                          borderRadius: "50%",
                          width: 13,
                          height: 13,
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          lineHeight: 1,
                          flexShrink: 0,
                        }}
                        aria-label="Explicație categorii TVA"
                      >
                        ?
                      </span>
                    </TooltipTrigger>
                    <TooltipContent side="top" style={{ maxWidth: 280 }}>
                      <strong>Categorii TVA (CIUS-RO):</strong>
                      <br />
                      <b>S</b> — Standard (TVA aplicată normal)
                      <br />
                      <b>AE</b> — Taxare inversă (reverse-charge B2B intracomunitar sau intern, TVA 0%)
                      <br />
                      <b>E</b> — Scutit fără drept de deducere (intern)
                      <br />
                      <b>Z</b> — Cotă zero
                      <br />
                      <b>K</b> — Intracomunitar scutit (livrare UE, 0% + VAT ID)
                      <br />
                      <b>G</b> — Export scutit (livrare extra-UE)
                      <br />
                      <b>O</b> — În afara sferei TVA
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </span>
            </th>
            <th style={{ width: 110 }} className="num">Valoare net</th>
            <th style={{ width: 110 }} className="num">Total cu TVA</th>
            <th style={{ width: 28 }}></th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => {
            const lineNet = l.quantity * l.unitPrice;
            const lineTotal = lineNet * (1 + l.vatRate / 100);
            return (
              <tr key={l.rowId}>
                <td
                  style={{
                    textAlign: "center",
                    color: "var(--text-dim)",
                    fontFamily: "var(--font-mono)",
                  }}
                >
                  {i + 1}
                </td>
                <td>
                  <input
                    value={l.cpvCode ?? ""}
                    onChange={(e) =>
                      updateLine(i, "cpvCode", e.target.value || undefined)
                    }
                    className="mono"
                  />
                </td>
                <td>
                  <div style={{ display: "flex", alignItems: "center", gap: 3 }}>
                    <input
                      value={l.name}
                      onChange={(e) => updateLine(i, "name", e.target.value)}
                      style={{ flex: 1, minWidth: 0 }}
                    />
                    {companyId && (
                      <ProductPickerButton
                        companyId={companyId}
                        onSelect={(p) => fillFromProduct(i, p)}
                      />
                    )}
                  </div>
                </td>
                <td className="num">
                  <input
                    type="number"
                    value={l.quantity}
                    onChange={(e) =>
                      updateLine(i, "quantity", parseFloat(e.target.value) || 0)
                    }
                    className="num"
                  />
                </td>
                <td>
                  <input
                    value={l.unit}
                    onChange={(e) => updateLine(i, "unit", e.target.value)}
                  />
                </td>
                <td className="num">
                  <input
                    type="number"
                    value={l.unitPrice}
                    onChange={(e) =>
                      updateLine(
                        i,
                        "unitPrice",
                        parseFloat(e.target.value) || 0,
                      )
                    }
                    className="num"
                  />
                </td>
                <td className="num">
                  <select
                    className="num"
                    value={l.vatRate}
                    onChange={(e) =>
                      updateLine(i, "vatRate", Number(e.target.value))
                    }
                  >
                    {buildVatRateOptions(l.vatRate).map((r) => (
                      <option key={r} value={r}>
                        {r}%
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <select
                    value={l.vatCategory}
                    onChange={(e) =>
                      updateLine(
                        i,
                        "vatCategory",
                        e.target.value as VatCategory,
                      )
                    }
                    style={{ width: "100%", fontSize: 11 }}
                    title={VAT_CATEGORY_LABELS[l.vatCategory]}
                  >
                    {VAT_CATEGORIES.map((cat) => (
                      <option key={cat} value={cat}>
                        {cat} — {VAT_CATEGORY_LABELS[cat]}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="num">
                  <input
                    value={lineNet.toFixed(2)}
                    className="num"
                    readOnly
                    style={{ color: "var(--text-muted)" }}
                  />
                </td>
                <td className="num">
                  <input
                    value={lineTotal.toFixed(2)}
                    className="num"
                    readOnly
                    style={{ fontWeight: 600 }}
                  />
                </td>
                <td>
                  <button
                    className="btn-icon"
                    onClick={() => removeLine(i)}
                    disabled={lines.length === 1}
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </td>
              </tr>
            );
          })}
          <tr
            className="line-add-row"
            onClick={addLine}
            style={{ cursor: "pointer" }}
          >
            <td colSpan={COL_SPAN}>
              <Icon name="plus" size={12} /> Adaugă linie
            </td>
          </tr>
        </tbody>
        {showTotals && (
          <tfoot>
            <tr>
              <td
                colSpan={7}
                style={{ textAlign: "right", color: "var(--text-muted)" }}
              >
                Subtotal net
              </td>
              <td className="num"></td>
              <td className="num tnum">{fmtRON(net)}</td>
              <td className="num"></td>
              <td></td>
            </tr>
            <tr>
              <td
                colSpan={7}
                style={{ textAlign: "right", color: "var(--text-muted)" }}
              >
                TVA
              </td>
              <td className="num"></td>
              <td className="num tnum">{fmtRON(vat)}</td>
              <td className="num"></td>
              <td></td>
            </tr>
            <tr>
              <td
                colSpan={7}
                style={{
                  textAlign: "right",
                  textTransform: "uppercase",
                  fontSize: 11,
                  letterSpacing: 0.04,
                }}
              >
                Total de plată
              </td>
              <td className="num"></td>
              <td className="num"></td>
              <td
                className="num tnum"
                style={{ fontSize: 14, color: "var(--accent)" }}
              >
                {fmtRON(total)}{" "}
                <span style={{ fontSize: 10.5, color: "var(--text-muted)" }}>
                  RON
                </span>
              </td>
              <td></td>
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
