import { describe, expect, it } from "vitest";

import { formatXml } from "./xml-format";

describe("formatXml", () => {
  it("indents nested elements by depth", () => {
    const out = formatXml("<a><b><c>1</c></b></a>");
    expect(out).toBe(["<a>", "  <b>", "    <c>1</c>", "  </b>", "</a>"].join("\n"));
  });

  it("keeps the <?xml?> prolog flush-left and at depth 0", () => {
    const out = formatXml('<?xml version="1.0" encoding="UTF-8"?><root><x>1</x></root>');
    expect(out.split("\n")[0]).toBe('<?xml version="1.0" encoding="UTF-8"?>');
    expect(out).toContain("<root>");
    expect(out).toContain("  <x>1</x>");
  });

  it("treats self-closing tags as a single level (does not over-indent siblings)", () => {
    const out = formatXml('<sect_II nrben="1"/><benef id="1"/>');
    expect(out).toBe('<sect_II nrben="1"/>\n<benef id="1"/>');
  });

  it("keeps single-line text elements on one line", () => {
    const out = formatXml("<den1>Popescu Andrei</den1>");
    expect(out).toBe("<den1>Popescu Andrei</den1>");
  });

  it("is idempotent on already-pretty input", () => {
    const pretty = formatXml("<a><b>1</b><b>2</b></a>");
    expect(formatXml(pretty)).toBe(pretty);
  });

  it("returns empty string for blank input and never throws", () => {
    expect(formatXml("")).toBe("");
    expect(formatXml("   \n  ")).toBe("");
    expect(() => formatXml("<a><b></a>")).not.toThrow();
  });
});
