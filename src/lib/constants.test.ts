import { describe, expect, it } from "vitest";
import {
  VAT_RATES,
  VAT_CATEGORIES,
  VAT_CATEGORY_LABELS,
  COUNTRIES,
  CURRENCIES,
} from "./constants";

describe("VAT_RATES", () => {
  it("contains exactly [0, 5, 9, 11, 19, 21]", () => {
    expect([...VAT_RATES]).toEqual([0, 5, 9, 11, 19, 21]);
  });

  it("all entries are non-negative numbers", () => {
    expect([...VAT_RATES].every((r) => typeof r === "number" && r >= 0)).toBe(true);
  });
});

describe("VAT_CATEGORIES", () => {
  const required = ["S", "AE", "E", "Z", "K", "G", "O"] as const;

  it("contains all required category codes", () => {
    for (const code of required) {
      expect([...VAT_CATEGORIES]).toContain(code);
    }
  });

  it("has 7 entries", () => {
    expect(VAT_CATEGORIES.length).toBe(7);
  });
});

describe("VAT_CATEGORY_LABELS", () => {
  it("every VAT_CATEGORY has a label entry", () => {
    for (const cat of VAT_CATEGORIES) {
      expect(VAT_CATEGORY_LABELS).toHaveProperty(cat);
      expect(typeof VAT_CATEGORY_LABELS[cat]).toBe("string");
      expect(VAT_CATEGORY_LABELS[cat].length).toBeGreaterThan(0);
    }
  });

  it("S label contains 'Standard'", () => {
    expect(VAT_CATEGORY_LABELS.S).toMatch(/Standard/i);
  });

  it("AE label contains 'invers' (taxare inversă)", () => {
    expect(VAT_CATEGORY_LABELS.AE).toMatch(/invers/i);
  });
});

describe("COUNTRIES", () => {
  it("has at least 27 entries (EU member count)", () => {
    expect(COUNTRIES.length).toBeGreaterThanOrEqual(27);
  });

  it("includes RO (România)", () => {
    const ro = COUNTRIES.find((c) => c.code === "RO");
    expect(ro).toBeDefined();
    expect(ro?.name).toBeTruthy();
  });

  it("all codes are 2-character uppercase strings", () => {
    for (const { code } of COUNTRIES) {
      expect(code).toMatch(/^[A-Z]{2}$/);
    }
  });

  it("all entries have non-empty name strings", () => {
    for (const { name } of COUNTRIES) {
      expect(typeof name).toBe("string");
      expect(name.length).toBeGreaterThan(0);
    }
  });

  it("no duplicate country codes", () => {
    const codes = COUNTRIES.map((c) => c.code);
    const unique = new Set(codes);
    expect(unique.size).toBe(codes.length);
  });
});

describe("CURRENCIES", () => {
  it("includes RON", () => {
    expect([...CURRENCIES]).toContain("RON");
  });

  it("includes EUR", () => {
    expect([...CURRENCIES]).toContain("EUR");
  });

  it("includes USD", () => {
    expect([...CURRENCIES]).toContain("USD");
  });

  it("has at least 3 entries", () => {
    expect(CURRENCIES.length).toBeGreaterThanOrEqual(3);
  });
});
