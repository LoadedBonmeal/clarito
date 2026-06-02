import { describe, expect, it } from "vitest";
import { formatOptionalRon } from "./formatters";
import { parseDec, fmtRON, formatDate, formatNumber } from "./utils";

describe("formatOptionalRon", () => {
  it("formats backend decimal strings", () => {
    expect(formatOptionalRon("123.4")).toBe("123.40 RON");
  });

  it("formats integers", () => {
    expect(formatOptionalRon("100")).toBe("100.00 RON");
  });

  it("handles undefined", () => {
    expect(formatOptionalRon(undefined)).toBe("sumă necunoscută");
  });

  it("handles null", () => {
    expect(formatOptionalRon(null)).toBe("sumă necunoscută");
  });

  it("handles empty string", () => {
    expect(formatOptionalRon("")).toBe("sumă necunoscută");
  });

  it("handles number type", () => {
    expect(formatOptionalRon(42.5)).toBe("42.50 RON");
  });

  it("handles NaN string", () => {
    expect(formatOptionalRon("not-a-number")).toBe("sumă necunoscută");
  });

  it("formats zero as 0.00 RON", () => {
    expect(formatOptionalRon(0)).toBe("0.00 RON");
  });

  it("formats negative amount", () => {
    expect(formatOptionalRon(-50.5)).toBe("-50.50 RON");
  });

  it("formats large amount with correct precision", () => {
    expect(formatOptionalRon("999999.99")).toBe("999999.99 RON");
  });
});

describe("parseDec", () => {
  it("parses a decimal string", () => {
    expect(parseDec("123.45")).toBe(123.45);
  });

  it("parses integer string", () => {
    expect(parseDec("100")).toBe(100);
  });

  it("returns 0 for non-numeric string", () => {
    expect(parseDec("abc")).toBe(0);
  });

  it("passes through a number directly", () => {
    expect(parseDec(42.5)).toBe(42.5);
  });

  it("returns 0 for undefined", () => {
    expect(parseDec(undefined)).toBe(0);
  });

  it("returns 0 for null", () => {
    expect(parseDec(null)).toBe(0);
  });
});

describe("fmtRON", () => {
  it("formats a positive amount with 2 decimals", () => {
    // Romanian locale uses comma as decimal separator
    const result = fmtRON(1234.56);
    expect(result).toMatch(/1\.234,56|1234\.56|1234,56/);
  });

  it("formats zero", () => {
    const result = fmtRON(0);
    expect(result).toMatch(/0,00|0\.00/);
  });

  it("formats a string amount", () => {
    const result = fmtRON("50.00");
    expect(result).toMatch(/50,00|50\.00/);
  });
});

describe("formatDate", () => {
  it("formats a date object without time by default", () => {
    const d = new Date("2024-06-15T12:00:00Z");
    const result = formatDate(d);
    // Should contain year 2024 and June in Romanian (iunie)
    expect(result).toContain("2024");
    expect(result.toLowerCase()).toMatch(/iunie|jun/);
  });

  it("formats a date string", () => {
    const result = formatDate("2024-01-01");
    expect(result).toContain("2024");
  });

  it("includes time when withTime=true", () => {
    const d = new Date("2024-06-15T14:30:00");
    const result = formatDate(d, true);
    expect(result).toContain("2024");
    // Should contain hour:minute pattern
    expect(result).toMatch(/\d{2}:\d{2}/);
  });

  // C: YYYY-MM-DD strings must parse as LOCAL dates so the displayed day matches
  // the stored date even in EET (UTC+2/+3). new Date("2026-01-15") would parse
  // as UTC midnight → render as "14 ianuarie" in EET.
  it("C: YYYY-MM-DD string renders the correct local day (not UTC-shifted)", () => {
    const result = formatDate("2026-01-15");
    // Must contain 15 (not 14 due to UTC shift)
    expect(result).toContain("15");
    expect(result).toContain("2026");
  });

  it("C: YYYY-MM-DD '2026-12-31' renders 31 not 30", () => {
    const result = formatDate("2026-12-31");
    expect(result).toContain("31");
    expect(result).toContain("2026");
  });
});

describe("formatNumber", () => {
  it("formats with 2 decimal places by default", () => {
    const result = formatNumber(1234.5);
    expect(result).toMatch(/1\.234,50|1234\.50|1234,50/);
  });

  it("formats with 0 decimal places", () => {
    const result = formatNumber(1234, 0);
    expect(result).toMatch(/1\.234|1234/);
    expect(result).not.toContain(",00");
    expect(result).not.toContain(".00");
  });
});

describe("RON-equivalent computation (Wave 5 multi-currency)", () => {
  /**
   * Helper that mirrors the LineItemsEditor M2 rounding used in the exchange-rate
   * RON-equivalent display: round each line to 2dp then sum.
   */
  function computeTotal(lines: { quantity: number; unitPrice: number; vatRate: number }[]) {
    const net = lines.reduce((s, l) => {
      const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
      return s + lineNet;
    }, 0);
    const vat = lines.reduce((s, l) => {
      const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
      const lineVat = Math.round(lineNet * (l.vatRate / 100) * 100) / 100;
      return s + lineVat;
    }, 0);
    return { net, vat, total: net + vat };
  }

  it("computes 1190 EUR × 4.97 = 5914.30 RON", () => {
    // Single line: qty=10, unitPrice=100 EUR, vatRate=19% → net=1000 EUR, vat=190 EUR, total=1190 EUR
    const lines = [{ quantity: 10, unitPrice: 100, vatRate: 19 }];
    const { total } = computeTotal(lines);
    expect(total).toBeCloseTo(1190, 2);
    const ronEquiv = Math.round(total * 4.97 * 100) / 100;
    expect(ronEquiv).toBeCloseTo(5914.3, 1);
  });

  it("returns 0 for an empty line list", () => {
    const { total } = computeTotal([]);
    expect(total).toBe(0);
  });

  it("applies M2 rounding: each line net is rounded to 2dp before summing", () => {
    // line: 3 × 10.005 = 30.015 → Math.round(30.015 * 100)/100 = 30.02
    const lines = [{ quantity: 3, unitPrice: 10.005, vatRate: 0 }];
    const { net } = computeTotal(lines);
    // 3 * 10.005 = 30.015, Math.round(30.015 * 100) = 3002, / 100 = 30.02
    expect(net).toBeCloseTo(30.02, 2);
  });
});
