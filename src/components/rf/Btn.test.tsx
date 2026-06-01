/**
 * Tests for Btn component — variant/size class logic.
 * Pure logic tests (no @testing-library/react needed) following
 * the pattern established in LineItemsEditor.test.tsx.
 */
import { describe, expect, it } from "vitest";
import type { BtnVariant, BtnSize } from "./Btn";

// ─── Class-name logic extracted from Btn ─────────────────────────────────────
// Mirrors the cn() call inside Btn component:
//   cn("rf-btn", `rf-btn--${variant}`, size === "sm" && "rf-btn--sm", size === "lg" && "rf-btn--lg", block && "rf-btn--block")

function btnClasses(variant: BtnVariant, size: BtnSize, block?: boolean): string[] {
  const cls = ["rf-btn", `rf-btn--${variant}`];
  if (size === "sm") cls.push("rf-btn--sm");
  if (size === "lg") cls.push("rf-btn--lg");
  if (block) cls.push("rf-btn--block");
  return cls;
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("Btn class logic", () => {
  it("default (secondary md) has base + variant classes", () => {
    const cls = btnClasses("secondary", "md");
    expect(cls).toContain("rf-btn");
    expect(cls).toContain("rf-btn--secondary");
    expect(cls).not.toContain("rf-btn--sm");
    expect(cls).not.toContain("rf-btn--lg");
    expect(cls).not.toContain("rf-btn--block");
  });

  it("primary variant has rf-btn--primary class", () => {
    const cls = btnClasses("primary", "md");
    expect(cls).toContain("rf-btn--primary");
    expect(cls).not.toContain("rf-btn--secondary");
  });

  it("ghost variant has rf-btn--ghost class", () => {
    const cls = btnClasses("ghost", "md");
    expect(cls).toContain("rf-btn--ghost");
  });

  it("danger variant has rf-btn--danger class", () => {
    const cls = btnClasses("danger", "md");
    expect(cls).toContain("rf-btn--danger");
  });

  it("size sm adds rf-btn--sm", () => {
    const cls = btnClasses("primary", "sm");
    expect(cls).toContain("rf-btn--sm");
    expect(cls).not.toContain("rf-btn--lg");
  });

  it("size lg adds rf-btn--lg", () => {
    const cls = btnClasses("secondary", "lg");
    expect(cls).toContain("rf-btn--lg");
    expect(cls).not.toContain("rf-btn--sm");
  });

  it("block prop adds rf-btn--block", () => {
    const cls = btnClasses("primary", "md", true);
    expect(cls).toContain("rf-btn--block");
  });

  it("no block prop → no rf-btn--block", () => {
    const cls = btnClasses("primary", "md", false);
    expect(cls).not.toContain("rf-btn--block");
  });

  it("all four variants produce distinct classes", () => {
    const variants: BtnVariant[] = ["primary", "secondary", "ghost", "danger"];
    const variantClasses = variants.map((v) => btnClasses(v, "md"));
    const variantClassNames = variantClasses.map((cls) => cls.find((c) => c.startsWith("rf-btn--")));
    // All distinct
    expect(new Set(variantClassNames).size).toBe(variants.length);
  });
});
