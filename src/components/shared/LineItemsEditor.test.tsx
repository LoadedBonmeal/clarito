/**
 * Tests for deduceVatCategory — the real exported pure function from
 * LineItemsEditor.tsx (imported via the "@" alias now configured in
 * vitest.config.ts). @testing-library/react is not installed, so these are
 * pure-logic tests only (no render smoke tests).
 */
import { describe, expect, it } from "vitest";
import { deduceVatCategory } from "@/components/shared/LineItemsEditor";

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
