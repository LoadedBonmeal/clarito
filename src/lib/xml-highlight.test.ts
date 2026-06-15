import { describe, expect, it } from "vitest";

import { tokenizeLines, type Token } from "./xml-highlight";

const joinRow = (row: Token[]) => row.map((t) => t.s).join("");
const verbatim = (xml: string) => tokenizeLines(xml).map(joinRow).join("\n");
const types = (line: string) => tokenizeLines(line)[0].map((t) => t.t);
const tok = (line: string) => tokenizeLines(line)[0];

describe("tokenizeLines — byte-verbatim invariant", () => {
  it("reproduces a full D205 document exactly (what-you-see-is-what-you-save)", () => {
    const xml = `<?xml version="1.0" encoding="UTF-8"?>
<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="2026" cui="40">
  <sect_II tip_venit="08" nrben="1" Tbaza="40000"/>
  <benef id_inreg="1" cifR="1960101410019" den1="Popescu Andrei" baza1="40000"/>
  <den1>Popescu &amp; Asociații</den1>
</declaratie205>`;
    expect(verbatim(xml)).toBe(xml);
  });

  it("preserves a UTF-8 BOM and Romanian diacritics exactly", () => {
    const xml = `﻿<?xml version="1.0" encoding="UTF-8"?>\n<AuditFile>\n  <den>Societatea Țărănească ăîâșț</den>\n</AuditFile>`;
    expect(verbatim(xml)).toBe(xml);
  });

  it("preserves indentation, empty lines and trailing newline", () => {
    const xml = `<a>\n\n  <b x="1"/>\n</a>\n`;
    expect(verbatim(xml)).toBe(xml);
  });

  it("handles a multi-line open tag (SAF-T <AuditFile …> header) verbatim", () => {
    const xml = `<AuditFile xmlns="urn:StandardAuditFile-Taxation-Financial:RO"\n           xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">\n  <Header/>\n</AuditFile>`;
    expect(verbatim(xml)).toBe(xml);
  });
});

describe("tokenizeLines — classification", () => {
  it("classifies the XML prolog as a single prolog token", () => {
    const t = tok(`<?xml version="1.0" encoding="UTF-8"?>`);
    expect(t).toHaveLength(1);
    expect(t[0].t).toBe("prolog");
  });

  it("splits a self-closing element into punct / tag / attr / val", () => {
    const t = tok(`  <benef cifR="1960101410019" imp1="1600"/>`);
    // leading indentation is a text token, then the tag pieces
    expect(t[0]).toEqual({ t: "text", s: "  " });
    expect(t.find((x) => x.t === "tag")?.s).toBe("benef");
    expect(t.filter((x) => x.t === "attr").map((x) => x.s)).toEqual(["cifR", "imp1"]);
    expect(t.filter((x) => x.t === "val").map((x) => x.s)).toEqual([
      '"1960101410019"',
      '"1600"',
    ]);
    expect(t[t.length - 1]).toEqual({ t: "punct", s: "/>" });
  });

  it("keeps an escaped value (&amp;) as literal text, never unescaped", () => {
    const t = tok(`<den1>Popescu &amp; Co</den1>`);
    const text = t.filter((x) => x.t === "text").map((x) => x.s).join("");
    expect(text).toBe("Popescu &amp; Co"); // the literal 5-char entity is preserved, not decoded to "&"
  });

  it("classifies an open and a close tag", () => {
    expect(types(`<declaratie205>`)).toEqual(["punct", "tag", "punct"]);
    expect(types(`</declaratie205>`)).toEqual(["punct", "tag", "punct"]);
  });

  it("never throws on malformed input and stays verbatim", () => {
    for (const bad of [`<unclosed attr="x`, `plain text`, `<a><b`, ``, `<!-- open comment`]) {
      expect(() => tokenizeLines(bad)).not.toThrow();
      expect(verbatim(bad)).toBe(bad);
    }
  });
});
