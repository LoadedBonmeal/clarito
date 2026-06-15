import { describe, expect, it } from "vitest";

import { xmlToTables } from "./xml-to-tables";

describe("xmlToTables", () => {
  it("turns a D205 declaration into a header KV table + a benef table", () => {
    const xml = `<?xml version="1.0" encoding="UTF-8"?>
<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="2026" cui="40">
  <sect_II tip_venit="08" nrben="1" Tbaza="40000"/>
  <benef id_inreg="1" cifR="1960101410019" den1="Popescu Andrei" baza1="40000"/>
  <benef id_inreg="2" cifR="2960101410010" den1="Ana Pop" baza1="10000"/>
</declaratie205>`;
    const tables = xmlToTables(xml);
    // root attrs → a key/value table
    const header = tables.find((t) => t.title === "declaratie205");
    expect(header).toBeTruthy();
    expect(header!.columns).toEqual(["Câmp", "Valoare"]);
    expect(header!.rows).toEqual([
      ["luna", "12"],
      ["an", "2026"],
      ["cui", "40"],
    ]);
    // sect_II → its own 1-row table
    expect(tables.find((t) => t.title === "sect_II")).toBeTruthy();
    // benef (×2) → a table with the union of attribute columns, one row each
    const benef = tables.find((t) => t.title.startsWith("benef"));
    expect(benef!.title).toBe("benef (×2)");
    expect(benef!.columns).toEqual(["id_inreg", "cifR", "den1", "baza1"]);
    expect(benef!.rows[0]).toEqual(["1", "1960101410019", "Popescu Andrei", "40000"]);
    expect(benef!.rows[1]).toEqual(["2", "2960101410010", "Ana Pop", "10000"]);
  });

  it("returns [] for invalid XML and never throws", () => {
    expect(xmlToTables("not xml <<<")).toEqual([]);
    expect(() => xmlToTables("")).not.toThrow();
  });
});
