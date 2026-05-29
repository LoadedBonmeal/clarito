import { describe, expect, it } from "vitest";
import { formatOptionalRon } from "./formatters";

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
});
