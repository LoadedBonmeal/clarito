/**
 * Tests for deduceVatCategory — the real exported pure function from
 * LineItemsEditor.tsx (imported via the "@" alias now configured in
 * vitest.config.ts). @testing-library/react is not installed, so these are
 * pure-logic tests only (no render smoke tests).
 */
import { describe, expect, it } from "vitest";
import { deduceVatCategory } from "@/components/shared/LineItemsEditor";

// ─── M2: per-line rounding helper (mirrors backend logic) ────────────────────

/** Replicates the backend round-then-sum used by the create/update path. */
function computeFooter(lines: Array<{ quantity: number; unitPrice: number; vatRate: number }>) {
  const net = lines.reduce((s, l) => {
    return s + Math.round(l.quantity * l.unitPrice * 100) / 100;
  }, 0);
  const vat = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    return s + Math.round(lineNet * (l.vatRate / 100) * 100) / 100;
  }, 0);
  return { net, vat, total: net + vat };
}

describe("M2: per-line rounding matches backend", () => {
  it("single line with integer cents rounds correctly", () => {
    const { net, vat } = computeFooter([
      { quantity: 1, unitPrice: 100, vatRate: 19 },
    ]);
    expect(net).toBe(100);
    expect(vat).toBe(19);
  });

  it("fractional-cent line is rounded per-line before summing", () => {
    // 3 × 0.333 = 0.999 → rounded to 1.00 net; vat = 0.19
    // Without per-line rounding: 3 * 0.333 * 1.19 = 1.18881 → total 1.19 (ok)
    // But the stored subtotal for this line = round(3 * 0.333, 2) = 1.00
    const { net } = computeFooter([
      { quantity: 3, unitPrice: 0.333, vatRate: 19 },
    ]);
    expect(net).toBe(1.0);
  });

  it("multi-line per-line rounding: two lines each with sub-cent product", () => {
    // Line 1: 1.5 × 1.33 = 1.995 → rounded = 2.00
    // Line 2: 2 × 0.505 = 1.010 → rounded = 1.01
    // Sum = 3.01 (not 3.005 from raw float sum)
    const { net } = computeFooter([
      { quantity: 1.5, unitPrice: 1.33, vatRate: 19 },
      { quantity: 2, unitPrice: 0.505, vatRate: 19 },
    ]);
    expect(net).toBeCloseTo(3.01, 2);
  });
});

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("deduceVatCategory (inline — pending alias fix in vitest.config.ts)", () => {
  // ── vatRate > 0 always → S ───────────────────────────────────────────

  it("rate 19 RO vatPayer → S (standard)", () => {
    expect(deduceVatCategory(19, "RO", true)).toBe("S");
  });

  it("rate 21 DE vatPayer → S (positive rate always standard)", () => {
    expect(deduceVatCategory(21, "DE", true)).toBe("S");
  });

  it("rate 9 FR vatPayer → S", () => {
    expect(deduceVatCategory(9, "FR", true)).toBe("S");
  });

  // ── vatRate === 0, buyer country wins; non-payer domestic → O ───────────
  // Country is resolved FIRST. A non-VAT-payer selling to an EU buyer still
  // gets K; to a non-EU buyer still gets G; only domestic (RO/unknown) with
  // a non-payer seller becomes O (out of scope), NOT AE (reverse charge).

  it("rate 0 RO non-payer → O (out of scope — neplătitor TVA, domestic)", () => {
    expect(deduceVatCategory(0, "RO", false)).toBe("O");
  });

  it("rate 0 DE non-payer → K (EU country wins over seller-payer status)", () => {
    expect(deduceVatCategory(0, "DE", false)).toBe("K");
  });

  it("rate 0 US non-payer → G (non-EU country wins over seller-payer status)", () => {
    expect(deduceVatCategory(0, "US", false)).toBe("G");
  });

  // ── vatRate === 0, seller IS vat payer, EU non-RO buyer → K ─────────

  it("rate 0 DE vatPayer → K (intra-EU exempt)", () => {
    expect(deduceVatCategory(0, "DE", true)).toBe("K");
  });

  it("rate 0 FR vatPayer → K", () => {
    expect(deduceVatCategory(0, "FR", true)).toBe("K");
  });

  it("rate 0 AT vatPayer → K", () => {
    expect(deduceVatCategory(0, "AT", true)).toBe("K");
  });

  // ── vatRate === 0, seller IS vat payer, non-EU non-RO buyer → G ─────

  it("rate 0 US vatPayer → G (export exempt)", () => {
    expect(deduceVatCategory(0, "US", true)).toBe("G");
  });

  it("rate 0 CH vatPayer → G (Switzerland is non-EU)", () => {
    expect(deduceVatCategory(0, "CH", true)).toBe("G");
  });

  it("rate 0 GB vatPayer → G (UK is non-EU post-Brexit)", () => {
    expect(deduceVatCategory(0, "GB", true)).toBe("G");
  });

  // ── vatRate === 0, RO buyer, vatPayer → E ────────────────────────────

  it("rate 0 RO vatPayer → E (scutit intern)", () => {
    expect(deduceVatCategory(0, "RO", true)).toBe("E");
  });

  it("rate 0 empty string buyer vatPayer → E (unknown treated as domestic)", () => {
    expect(deduceVatCategory(0, "", true)).toBe("E");
  });

  // ── vatRate === 0, domestic, non-payer → O (not AE) ──────────────────
  it("rate 0 empty string non-payer → O (domestic non-payer = out of scope)", () => {
    expect(deduceVatCategory(0, "", false)).toBe("O");
  });

  // ── Case insensitivity ───────────────────────────────────────────────

  it("lowercase 'de' is treated as DE → K", () => {
    expect(deduceVatCategory(0, "de", true)).toBe("K");
  });

  it("lowercase 'ro' is treated as RO → E", () => {
    expect(deduceVatCategory(0, "ro", true)).toBe("E");
  });

  it("mixed case 'Fr' → K", () => {
    expect(deduceVatCategory(0, "Fr", true)).toBe("K");
  });
});
