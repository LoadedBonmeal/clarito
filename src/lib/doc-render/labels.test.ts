import { describe, expect, it } from "vitest";

import { formatDate, formatNumber, MONTHS_RO } from "@/lib/utils";

import { formatValue, resolveField, vatCategoryLabel } from "./labels";

describe("resolveField", () => {
  it("uses the D205 override for declaration-specific attributes", () => {
    expect(resolveField("D205", "cifR").label).toBe("CNP beneficiar");
    expect(resolveField("D205", "baza1")).toMatchObject({ label: "Bază de calcul", format: "money_lei" });
    expect(resolveField("D205", "divid_D").label).toBe("Dividende distribuite");
  });
  it("falls back global → raw name", () => {
    expect(resolveField("D205", "cui").label).toBe("Cod fiscal (CUI)"); // from globals
    expect(resolveField("UNKNOWN_DOC", "cui").label).toBe("Cod fiscal (CUI)");
    expect(resolveField("D205", "habarnam")).toEqual({ label: "habarnam" }); // never throws
  });
});

describe("formatValue", () => {
  it("formats whole-lei money, month and date via the app formatters", () => {
    expect(formatValue("40000", { label: "x", format: "money_lei" })).toBe(`${formatNumber(40000, 0)} lei`);
    expect(formatValue("12", { label: "x", format: "month" })).toBe(MONTHS_RO[11]);
    expect(formatValue("2022-06-14", { label: "x", format: "date" })).toBe(formatDate("2022-06-14"));
  });
  it("maps coded enums; leaves text/cnp untouched; unknown code → raw", () => {
    const drec = { label: "x", format: "enum" as const, enumMap: { "0": "Inițială", "1": "Rectificativă" } };
    expect(formatValue("0", drec)).toBe("Inițială");
    expect(formatValue("99", drec)).toBe("99");
    expect(formatValue("1960101410019", { label: "x", format: "cnp" })).toBe("1960101410019");
    expect(formatValue("ceva", { label: "x" })).toBe("ceva");
  });
});

describe("vatCategoryLabel", () => {
  it("maps CIUS-RO categories like the invoice PDF", () => {
    expect(vatCategoryLabel("S", "21")).toBe("21%");
    expect(vatCategoryLabel("O", "")).toBe("0% (în afara sferei)");
    expect(vatCategoryLabel("AE", "")).toBe("0% (taxare inversă)");
    expect(vatCategoryLabel("E", "")).toBe("0% (scutit)");
  });
});
