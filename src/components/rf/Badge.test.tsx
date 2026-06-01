/**
 * Tests for Badge component — variant class logic.
 * Pure logic tests following the pattern in LineItemsEditor.test.tsx.
 */
import { describe, expect, it } from "vitest";
import type { BadgeVariant } from "./Badge";

// ─── Class-name logic extracted from Badge ────────────────────────────────────
// Mirrors: cn("rf-badge", `rf-badge--${variant}`)

function badgeClasses(variant: BadgeVariant): string[] {
  return ["rf-badge", `rf-badge--${variant}`];
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("Badge class logic", () => {
  const VARIANTS: BadgeVariant[] = ["success", "error", "warning", "info", "neutral"];

  it("always has rf-badge base class", () => {
    for (const v of VARIANTS) {
      expect(badgeClasses(v)).toContain("rf-badge");
    }
  });

  it("success variant → rf-badge--success", () => {
    expect(badgeClasses("success")).toContain("rf-badge--success");
  });

  it("error variant → rf-badge--error", () => {
    expect(badgeClasses("error")).toContain("rf-badge--error");
  });

  it("warning variant → rf-badge--warning", () => {
    expect(badgeClasses("warning")).toContain("rf-badge--warning");
  });

  it("info variant → rf-badge--info", () => {
    expect(badgeClasses("info")).toContain("rf-badge--info");
  });

  it("neutral variant → rf-badge--neutral", () => {
    expect(badgeClasses("neutral")).toContain("rf-badge--neutral");
  });

  it("all five variants produce distinct modifier classes", () => {
    const modifiers = VARIANTS.map((v) => `rf-badge--${v}`);
    expect(new Set(modifiers).size).toBe(VARIANTS.length);
  });

  it("each variant class only appears once (no duplicates)", () => {
    for (const v of VARIANTS) {
      const cls = badgeClasses(v);
      const modifiers = cls.filter((c) => c.startsWith("rf-badge--"));
      expect(modifiers.length).toBe(1);
    }
  });
});
