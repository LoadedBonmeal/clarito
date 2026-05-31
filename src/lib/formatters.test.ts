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
