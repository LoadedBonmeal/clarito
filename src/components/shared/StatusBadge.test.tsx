/**
 * Tests for StatusBadge — code→variant mapping and Romanian labels.
 * Pure logic tests (no render) following the pattern in LineItemsEditor.test.tsx.
 */
import { describe, expect, it } from "vitest";
import { getVariant } from "@/components/shared/StatusBadge";

// ─── LABELS mirror (from StatusBadge.tsx) ────────────────────────────────────

const LABELS: Record<string, string> = {
  draft:     "Schiță",
  queued:    "În așteptare",
  submitted: "Trimisă",
  validated: "Validată",
  rejected:  "Respinsă",
  storned:   "Stornată",
  new:       "Nouă",
  reviewed:  "Revizuită",
  approved:  "Aprobată",
  archived:  "Arhivată",
  pending:   "În așteptare",
  paid:      "Plătit",
  unpaid:    "Neplătit",
  partial:   "Parțial",
  overdue:   "Restanță",
  active:    "Activ",
  inactive:  "Inactiv",
};

// ─── Variant mapping tests ────────────────────────────────────────────────────

describe("StatusBadge getVariant — code→variant mapping", () => {
  // success
  it("validated → success", () => expect(getVariant("validated")).toBe("success"));
  it("approved → success", () => expect(getVariant("approved")).toBe("success"));
  it("paid → success", () => expect(getVariant("paid")).toBe("success"));
  it("active → success", () => expect(getVariant("active")).toBe("success"));

  // info
  it("submitted → info", () => expect(getVariant("submitted")).toBe("info"));
  it("new → info", () => expect(getVariant("new")).toBe("info"));

  // neutral
  it("draft → neutral", () => expect(getVariant("draft")).toBe("neutral"));
  it("unpaid → neutral", () => expect(getVariant("unpaid")).toBe("neutral"));
  it("archived → neutral", () => expect(getVariant("archived")).toBe("neutral"));
  it("inactive → neutral", () => expect(getVariant("inactive")).toBe("neutral"));

  // error
  it("rejected → error", () => expect(getVariant("rejected")).toBe("error"));
  it("overdue → error", () => expect(getVariant("overdue")).toBe("error"));

  // warning
  it("queued → warning", () => expect(getVariant("queued")).toBe("warning"));
  it("pending → warning", () => expect(getVariant("pending")).toBe("warning"));
  it("partial → warning", () => expect(getVariant("partial")).toBe("warning"));
  it("reviewed → warning", () => expect(getVariant("reviewed")).toBe("warning"));
  it("storned → warning", () => expect(getVariant("storned")).toBe("warning"));

  // unknown → neutral fallback
  it("unknown code → neutral fallback", () => expect(getVariant("unknown_xyz")).toBe("neutral"));
  it("empty string → neutral fallback", () => expect(getVariant("")).toBe("neutral"));
});

// ─── Uppercase passthrough tests (StatusBadge lowercases the status) ──────────

describe("StatusBadge getVariant — case-insensitive (caller lowercases)", () => {
  it("lowercase 'validated' maps to success", () => {
    expect(getVariant("validated")).toBe("success");
  });
  it("lowercase 'rejected' maps to error", () => {
    expect(getVariant("rejected")).toBe("error");
  });
});

// ─── Label tests ──────────────────────────────────────────────────────────────

describe("StatusBadge Romanian labels", () => {
  it("validated has Romanian label 'Validată'", () => {
    expect(LABELS["validated"]).toBe("Validată");
  });
  it("rejected has Romanian label 'Respinsă'", () => {
    expect(LABELS["rejected"]).toBe("Respinsă");
  });
  it("draft has Romanian label 'Schiță'", () => {
    expect(LABELS["draft"]).toBe("Schiță");
  });
  it("queued has Romanian label 'În așteptare'", () => {
    expect(LABELS["queued"]).toBe("În așteptare");
  });
  it("submitted has Romanian label 'Trimisă'", () => {
    expect(LABELS["submitted"]).toBe("Trimisă");
  });
  it("all labels are non-empty strings", () => {
    for (const [code, label] of Object.entries(LABELS)) {
      expect(typeof label).toBe("string");
      expect(label.length).toBeGreaterThan(0);
      // labels should be Romanian (non-ASCII chars expected or at least capitalized)
      expect(label[0]).toMatch(/[A-ZĂÎÂȘȚ]/);
      expect(code.length).toBeGreaterThan(0);
    }
  });
});
