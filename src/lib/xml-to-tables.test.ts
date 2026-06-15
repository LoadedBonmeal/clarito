import { describe, expect, it } from "vitest";

import { formatValue, resolveField } from "./doc-render/labels";
import { xmlToTables } from "./xml-to-tables";

const D205_XML = `<?xml version="1.0" encoding="UTF-8"?>
<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="2026" cui="40">
  <sect_II tip_venit="08" nrben="1" Tbaza="40000"/>
  <benef id_inreg="1" cifR="1960101410019" den1="Popescu Andrei" baza1="40000"/>
  <benef id_inreg="2" cifR="2960101410010" den1="Ana Pop" baza1="10000"/>
</declaratie205>`;

describe("xmlToTables", () => {
  it("labels a D205 declaration like the printed document (titles + headers + formatted values)", () => {
    const tables = xmlToTables(D205_XML, "D205");

    // Root → the descriptor's document title; attributes resolved to human labels + formatted values.
    const header = tables.find((t) => t.title.startsWith("Declarația 205"));
    expect(header).toBeTruthy();
    expect(header!.columns).toEqual(["Câmp", "Valoare"]);
    const luna = resolveField("D205", "luna");
    expect(header!.rows).toContainEqual([luna.label, formatValue("12", luna)]);
    expect(header!.rows).toContainEqual([resolveField("D205", "cui").label, "40"]);

    // benef group → titled "Beneficiari", headers mapped to labels, money values formatted.
    const benef = tables.find((t) => t.title.startsWith("Beneficiari"));
    expect(benef!.title).toBe("Beneficiari (×2)");
    expect(benef!.columns).toEqual([
      resolveField("D205", "id_inreg").label,
      resolveField("D205", "cifR").label,
      resolveField("D205", "den1").label,
      resolveField("D205", "baza1").label,
    ]);
    const baza1 = resolveField("D205", "baza1");
    expect(baza1.format).toBe("money_lei"); // guards the labeling premise
    expect(benef!.rows[0]).toEqual([
      "1",
      "1960101410019",
      "Popescu Andrei",
      formatValue("40000", baza1), // money_lei → "40.000 lei", NOT the raw "40000"
    ]);
    expect(benef!.rows[0][3]).not.toBe("40000");
  });

  it("falls back to raw tag/name for a document without a descriptor", () => {
    const tables = xmlToTables(`<oarecare camp_x="1"/>`); // no docKey, unknown root → no dictionary
    const t = tables.find((tbl) => tbl.title === "oarecare");
    expect(t).toBeTruthy();
    expect(t!.rows).toEqual([["camp_x", "1"]]); // raw name + raw value
  });

  it("returns [] for invalid XML and never throws", () => {
    expect(xmlToTables("not xml <<<")).toEqual([]);
    expect(() => xmlToTables("")).not.toThrow();
  });
});
