/**
 * LineItemsEditor — reusable line-items table for invoices and recurring templates.
 * Re-skinned to the Claude-Design vocabulary (.scr-table / .input / .select /
 * .mini-btn / .banner.warn / add-row affordance).
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
import { useTranslation } from "react-i18next";
import { Ic } from "@/components/shared/Ic";
import { ProductPickerButton } from "@/components/shared/ProductCombobox";
import { VAT_RATES, VAT_CATEGORIES, VAT_CATEGORY_LABELS } from "@/lib/constants";
import { fmtRON } from "@/lib/utils";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { CreateLineInput, Product, VatCategory } from "@/types";

// Heroicons outline paths not present in Ic.tsx (inlined verbatim, design convention).
const SVG_TRASH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';
const SVG_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

/** EU alpha-2 country codes (excluding Romania). Used for auto-deduction of vatCategory. */
const EU_CODES = new Set([
  "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI",
  "FR", "DE", "GR", "HU", "IE", "IT", "LV", "LT", "LU",
  "MT", "NL", "PL", "PT", "SK", "SI", "ES", "SE",
]);

/**
 * Pure helper — deduces the correct VAT category from rate, buyer country,
 * seller VAT payer status and (optionally) whether the buyer has a VAT id.
 *
 * Rules:
 *  - vatRate > 0  → 'S' (standard)
 *  - vatRate === 0 — SELLER VAT status is resolved FIRST:
 *      - seller is NOT a VAT payer → 'O' (în afara sferei TVA) for ANY buyer
 *        country. A neplătitor has no VAT id, so K/E/AE/G would all be
 *        ANAF-fatal (BR-IC-02 / BR-E-02 / BR-AE-02 / BR-G-02 each require the
 *        seller VAT identifier BT-31).
 *      - seller IS a VAT payer:
 *          - buyer is EU (non-RO) WITH a VAT id → 'K' (livrare intracomunitară
 *            scutită — BR-IC-02 also requires the buyer VAT id BT-48)
 *          - buyer is EU (non-RO) WITHOUT a VAT id → 'S' (a B2C EU sale is
 *            normally taxed, not exempt-intra-EU)
 *          - buyer is non-EU and non-RO         → 'G' (export scutit)
 *          - buyer is RO (or unknown)           → 'E' (scutit intern fără
 *            drept de deducere)
 *      NOTE: 'AE' (taxare inversă) is NOT auto-assigned here; it is only correct
 *      for genuine intra-community or domestic reverse-charge situations and must
 *      be set explicitly by the user.
 *  - default                               → 'S'
 *
 * `buyerHasVatId` defaults to `true` so callers that cannot know the buyer's
 * VAT id keep the historical deduction; the rocius preflight (BR-IC-02) still
 * blocks a K invoice whose buyer has no VAT id before it reaches ANAF.
 */
export function deduceVatCategory(
  vatRate: number,
  buyerCountry: string,
  sellerVatPayer: boolean,
  buyerHasVatId: boolean = true,
): VatCategory {
  if (vatRate > 0) return "S";
  if (vatRate === 0) {
    // Seller status wins — a non-VAT-payer must get 'O' regardless of country.
    if (!sellerVatPayer) return "O";
    const country = (buyerCountry ?? "").toUpperCase().trim();
    // K only when the buyer is VAT-registered; a B2C EU sale is normally taxed.
    if (EU_CODES.has(country)) return buyerHasVatId ? "K" : "S";
    if (country && country !== "RO") return "G";
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
  /**
   * Whether the buyer has a VAT id (CUI/cod TVA) on file — gates the 'K'
   * auto-deduction for EU buyers (BR-IC-02 requires the buyer VAT id BT-48).
   * Defaults to true so callers that don't pass it keep the historical behavior.
   */
  buyerHasVatId?: boolean;
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
  /** Invoice issue date (ISO YYYY-MM-DD) — used to warn on the transitional 9% housing rate after its
   *  2026-07-31 sunset. Optional: when absent, the 9% warning is simply not shown. */
  issueDate?: string;
}

export function LineItemsEditor({
  lines,
  onChange,
  buyerCountry = "RO",
  sellerVatPayer = true,
  buyerHasVatId = true,
  showTotals = true,
  companyId,
  currency = "RON",
  issueDate,
}: LineItemsEditorProps) {
  const { t } = useTranslation();
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

  // Auto-deduce vatCategory for each line when vatRate, buyerCountry,
  // sellerVatPayer or buyerHasVatId changes. Manual changes to vatCategory made
  // by the user are NOT clobbered because the deduceKey only changes for
  // buyerCountry+sellerVatPayer+buyerHasVatId.
  useEffect(() => {
    const key = `${buyerCountry}|${sellerVatPayer}|${buyerHasVatId}`;
    if (key === prevDeduceKey.current) return;
    prevDeduceKey.current = key;

    const updated = lines.map((l) => ({
      ...l,
      vatCategory: deduceVatCategory(l.vatRate, buyerCountry, sellerVatPayer, buyerHasVatId),
    }));
    onChange(updated);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [buyerCountry, sellerVatPayer, buyerHasVatId]);

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
          buyerHasVatId,
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
      vatRate: sellerVatPayer ? 21 : 0,
      vatCategory: deduceVatCategory(
        sellerVatPayer ? 21 : 0,
        buyerCountry,
        sellerVatPayer,
        buyerHasVatId,
      ),
      rowId: crypto.randomUUID(),
    };
    onChange([...lines, newLine]);
  };

  const removeLine = (idx: number) =>
    onChange(lines.filter((_, i) => i !== idx));

  /**
   * Fill a line from a product picked from the catalog.
   * Overwrites name, unit, unitPrice, vatRate, vatCategory, cpvCode, art331Code.
   * When the product is marked as a service (isService=true), defaults revenueKind to "service"
   * so the GL posts to account 704 instead of 707 — the user can still change this.
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
        // Snapshot the art.331 code from the product for D394 codPR tracking.
        art331Code: product.art331Code ?? undefined,
        // B4: revenueKind tracks the FILLED product's nature so the GL revenue account is always
        // correct: service → 704; goods → undefined (backend default "goods" → 707). We reset it on
        // every product fill (not preserve) so re-picking a goods product after a service one can't
        // leave the line silently posting to 704 — there is no UI control to override it.
        revenueKind: product.isService ? "service" : undefined,
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
    S: t("shared.lineItems.cat.S"), AE: t("shared.lineItems.cat.AE"),
    E: t("shared.lineItems.cat.E"), Z: t("shared.lineItems.cat.Z"),
    O: t("shared.lineItems.cat.O"), K: t("shared.lineItems.cat.K"),
    G: t("shared.lineItems.cat.G"),
  };

  // Column count: # + Cod + Descriere + Cant + UM + Preț + TVA% + Categorie + Net + Total + del = 11
  const COL_SPAN = 11;

  // Compact design .input/.select sizing for editor cells.
  const cellInput: React.CSSProperties = { height: 30, padding: "0 8px" };
  const cellInputNum: React.CSSProperties = { ...cellInput, textAlign: "right" };
  const cellSelect: React.CSSProperties = {
    height: 30,
    padding: "0 24px 0 8px",
    fontSize: 12,
    backgroundPosition: "right 6px center",
  };
  const cellTd: React.CSSProperties = { padding: "6px 4px", verticalAlign: "middle" };

  // Plain-text tooltip for the Categorie header help badge.
  const CAT_HELP = t("shared.lineItems.catHelp");

  // Legea 141/2025: 19% and 5% are abolished from 01.08.2025 for new operations. Warn if a line
  // still carries one (e.g. auto-filled from an old product), steering to 21% / 11%.
  const hasAbolishedRate = lines.some((l) => l.vatRate === 19 || l.vatRate === 5);
  // Legea 141/2025 art. III: the transitional 9% housing rate expired 31.07.2026. After that a 9% line
  // is valid ONLY for regularizări/storno of pre-cutoff deliveries — so this is a non-blocking nudge
  // (mirrors the backend W04 rule in ubl/rocius_rules.rs), not a hard block.
  const has9pAfterCutoff =
    !!issueDate && issueDate > "2026-07-31" && lines.some((l) => l.vatRate === 9);

  return (
    <div style={{ overflowX: "auto", borderTop: "1px solid var(--line)" }}>
      {hasAbolishedRate && (
        <div className="banner warn" style={{ margin: "10px 12px", marginBottom: 10 }}>
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
          <span>
            {t("shared.lineItems.abolishedWarn1")} <b>21%</b>{" "}
            {t("shared.lineItems.abolishedWarn2")} <b>11%</b>{" "}
            {t("shared.lineItems.abolishedWarn3")}
          </span>
        </div>
      )}
      {has9pAfterCutoff && (
        <div className="banner warn" style={{ margin: "10px 12px", marginBottom: 10 }}>
          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_WARN }} />
          <span>{t("shared.lineItems.housing9Warn")}</span>
        </div>
      )}
      <table className="scr-table li-table">
        <thead>
          <tr>
            <th style={{ width: 32, padding: "9px 8px", textAlign: "center" }}>#</th>
            <th style={{ width: 110, padding: "9px 8px" }}>{t("shared.lineItems.headers.code")}</th>
            <th style={{ padding: "9px 8px" }}>{t("shared.lineItems.headers.description")}</th>
            <th className="r" style={{ width: 64, padding: "9px 8px", whiteSpace: "nowrap" }}>{t("shared.lineItems.headers.qty")}</th>
            <th style={{ width: 56, padding: "9px 8px" }}>{t("shared.lineItems.headers.unit")}</th>
            <th className="r" style={{ width: 100, padding: "9px 8px", whiteSpace: "nowrap" }}>{t("shared.lineItems.headers.unitPrice")}</th>
            <th className="r" style={{ width: 64, padding: "9px 8px", whiteSpace: "nowrap" }}>{t("shared.lineItems.headers.vatPct")}</th>
            <th style={{ width: 150, padding: "9px 8px" }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 3 }}>
                {t("shared.lineItems.headers.category")}
                <span
                  title={CAT_HELP}
                  aria-label={t("shared.lineItems.catHelpAria")}
                  style={{
                    cursor: "help", fontSize: 10, color: "var(--dim)",
                    border: "1px solid var(--line)", borderRadius: "50%",
                    width: 13, height: 13, display: "inline-flex", alignItems: "center",
                    justifyContent: "center", lineHeight: 1, flexShrink: 0,
                  }}
                >?</span>
              </span>
            </th>
            <th className="r" style={{ width: 110, padding: "9px 8px", whiteSpace: "nowrap" }}>{t("shared.lineItems.headers.net")}</th>
            <th className="r" style={{ width: 110, padding: "9px 8px", whiteSpace: "nowrap" }}>{t("shared.lineItems.headers.totalWithVat")}</th>
            <th style={{ width: 36, padding: "9px 4px" }}></th>
          </tr>
        </thead>
        <tbody>
          {lines.map((l, i) => {
            const lineNet = l.quantity * l.unitPrice;
            // VAT1: only category 'S' charges VAT; all others display 0 VAT.
            const effectiveVatRate = l.vatCategory === "S" ? l.vatRate : 0;
            const lineTotal = lineNet * (1 + effectiveVatRate / 100);
            return (
              <tr key={l.rowId}>
                {/* # */}
                <td className="muted num" style={{ ...cellTd, padding: "6px 8px", textAlign: "center" }}>
                  {i + 1}
                </td>
                {/* Cod */}
                <td style={cellTd}>
                  <input
                    className="input num"
                    value={l.cpvCode ?? ""}
                    onChange={(e) => updateLine(i, "cpvCode", e.target.value || undefined)}
                    style={cellInput}
                    placeholder={t("shared.lineItems.codePlaceholder")}
                  />
                </td>
                {/* Descriere */}
                <td style={cellTd}>
                  <div style={{ display: "flex", alignItems: "center", gap: 3 }}>
                    <input
                      className="input"
                      value={l.name}
                      onChange={(e) => updateLine(i, "name", e.target.value)}
                      style={{ ...cellInput, flex: 1, minWidth: 0 }}
                      placeholder={t("shared.lineItems.descPlaceholder")}
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
                <td style={cellTd}>
                  <input
                    className="input num"
                    type="number"
                    value={l.quantity}
                    onChange={(e) => updateLine(i, "quantity", parseFloat(e.target.value) || 0)}
                    style={cellInputNum}
                  />
                </td>
                {/* UM */}
                <td style={cellTd}>
                  <input
                    className="input"
                    value={l.unit}
                    onChange={(e) => updateLine(i, "unit", e.target.value)}
                    style={cellInput}
                  />
                </td>
                {/* Preț */}
                <td style={cellTd}>
                  <input
                    className="input num"
                    type="number"
                    value={l.unitPrice}
                    onChange={(e) => updateLine(i, "unitPrice", parseFloat(e.target.value) || 0)}
                    style={cellInputNum}
                  />
                </td>
                {/* TVA % */}
                <td style={cellTd}>
                  <select
                    className="select num"
                    style={cellSelect}
                    value={l.vatRate}
                    onChange={(e) => updateLine(i, "vatRate", Number(e.target.value))}
                  >
                    {buildVatRateOptions(l.vatRate).map((r) => (
                      <option key={r} value={r}>{r}%</option>
                    ))}
                  </select>
                </td>
                {/* Categorie */}
                <td style={cellTd}>
                  <select
                    className="select"
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
                <td className="r num muted" style={{ ...cellTd, padding: "6px 8px" }}>
                  {lineNet.toFixed(2)}
                </td>
                {/* Total cu TVA */}
                <td className="r num" style={{ ...cellTd, padding: "6px 8px", fontWeight: 600 }}>
                  {lineTotal.toFixed(2)}
                </td>
                {/* Delete */}
                <td style={{ ...cellTd, padding: "6px 4px" }}>
                  <div className="row-acts">
                    <button
                      className="mini-btn"
                      onClick={() => removeLine(i)}
                      disabled={lines.length === 1}
                      title={t("shared.lineItems.deleteLine")}
                      style={lines.length === 1 ? { opacity: 0.3, cursor: "default" } : undefined}
                    >
                      <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: SVG_TRASH }} />
                    </button>
                  </div>
                </td>
              </tr>
            );
          })}
          {/* Add-line row (design add-row affordance, cf. .add-sm) */}
          <tr style={{ cursor: "pointer" }} onClick={addLine}>
            <td colSpan={COL_SPAN} style={{ padding: "10px 16px" }}>
              <span
                style={{
                  display: "inline-flex", alignItems: "center", gap: 5,
                  fontSize: 12.5, fontWeight: 500, color: "var(--text-2)",
                }}
              >
                <Ic name="plus" cls="ic" /> {t("shared.lineItems.addLine")}
              </span>
            </td>
          </tr>
        </tbody>
        {showTotals && (
          <tfoot>
            <tr style={{ background: "var(--fill)" }}>
              <td
                colSpan={7}
                className="r muted"
                style={{ padding: "10px 8px", fontSize: 12, borderTop: "1px solid var(--line)" }}
              >
                {t("shared.lineItems.subtotalNet")}
              </td>
              <td style={{ borderTop: "1px solid var(--line)" }}></td>
              <td
                className="r num"
                style={{ padding: "10px 8px", fontWeight: 600, fontSize: 13, borderTop: "1px solid var(--line)" }}
              >
                {fmtRON(net)}
              </td>
              <td style={{ borderTop: "1px solid var(--line)" }}></td>
              <td style={{ borderTop: "1px solid var(--line)" }}></td>
            </tr>
            <tr style={{ background: "var(--fill)" }}>
              <td colSpan={7} className="r muted" style={{ padding: "8px", fontSize: 12 }}>
                {t("shared.lineItems.vat")}
              </td>
              <td></td>
              <td className="r num" style={{ padding: "8px", fontSize: 13 }}>
                {fmtRON(vat)}
              </td>
              <td></td>
              <td></td>
            </tr>
            <tr style={{ background: "var(--fill)" }}>
              <td
                colSpan={7}
                className="r"
                style={{
                  padding: "10px 8px", fontSize: 11, textTransform: "uppercase",
                  letterSpacing: ".04em", fontWeight: 700, color: "var(--text)",
                }}
              >
                {t("shared.lineItems.totalDue")}
              </td>
              <td></td>
              <td></td>
              <td className="r num" style={{ padding: "10px 8px", fontSize: 15, fontWeight: 600, letterSpacing: "-.02em" }}>
                {fmtRON(total)}{" "}
                <span className="muted" style={{ fontSize: 10.5, fontWeight: 400 }}>{currency}</span>
              </td>
              <td></td>
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
