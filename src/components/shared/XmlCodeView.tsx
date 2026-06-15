/**
 * XmlCodeView — read-only, syntax-highlighted rendering of the declaration XML the app generates.
 *
 * It shows the EXACT bytes that "Salvează" writes (the backend already pretty-prints to 2-space,
 * ANAF-canonical XML), as a clean, line-numbered document — so the in-app preview and the saved
 * `.xml` are the same thing. Highlighting is purely cosmetic (`xml-highlight` tokenizer → React
 * spans, no `dangerouslySetInnerHTML`); the displayed characters equal the file bytes.
 *
 * Large documents (big SAF-T D406) fall back to a plain monospace block to stay snappy. Any
 * unexpected tokenizer error also falls back to the raw text — the document always renders.
 */
import { useMemo } from "react";

import { tokenizeLines, type TokenType } from "@/lib/xml-highlight";

// Above this many lines we skip per-token spans (still the exact XML, just un-colored) to avoid
// jank on large SAF-T files. Declaration XML is normally well under this.
const HIGHLIGHT_LINE_CAP = 5000;

const CLS: Record<TokenType, string> = {
  prolog: "xt-prolog",
  comment: "xt-comment",
  punct: "xt-punct",
  tag: "xt-tag",
  attr: "xt-attr",
  val: "xt-val",
  text: "xt-text",
};

export function XmlCodeView({ xml }: { xml: string }) {
  const rows = useMemo(() => {
    const lineCount = xml.length === 0 ? 0 : xml.split("\n").length;
    if (lineCount === 0 || lineCount > HIGHLIGHT_LINE_CAP) return null;
    try {
      return tokenizeLines(xml);
    } catch {
      return null;
    }
  }, [xml]);

  // Fallback: empty, oversized, or tokenizer error → show the raw text verbatim.
  if (!rows) {
    return (
      <div className="xmlv-code">
        <pre className="xmlv-raw">{xml}</pre>
      </div>
    );
  }

  return (
    <div className="xmlv-code">
      <div className="xmlv-code-inner">
        {rows.map((tokens, i) => (
          <div className="xmlv-ln" key={i}>
            <span className="xmlv-gutter" aria-hidden="true">
              {i + 1}
            </span>
            <code className="xmlv-lc">
              {tokens.map((tk, j) => (
                <span className={CLS[tk.t]} key={j}>
                  {tk.s}
                </span>
              ))}
            </code>
          </div>
        ))}
      </div>
    </div>
  );
}
