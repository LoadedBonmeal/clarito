/**
 * xml-highlight — a tiny, dependency-free tokenizer for read-only syntax highlighting of the
 * declaration XML the app generates (already pretty-printed, 2-space, by the Rust backend).
 *
 * It NEVER reformats: it splits the input into lines and classifies each character into a typed
 * token so the viewer can color it. The hard invariant (covered by tests) is byte-verbatim:
 *
 *     tokenizeLines(xml).map(r => r.map(t => t.s).join("")).join("\n") === xml
 *
 * so "what you see is what you save". A small state machine (TEXT / TAG / COMMENT) carried across
 * lines handles multi-line open tags (the SAF-T `<AuditFile …>` header) and comments. It never
 * throws and degrades gracefully on anything it can't classify (emits the chars as plain text).
 */

export type TokenType =
  | "prolog" // <?xml …?>, <!DOCTYPE …>
  | "comment" // <!-- … -->
  | "punct" // < > </ /> =
  | "tag" // element name
  | "attr" // attribute name
  | "val" // quoted attribute value (quotes included)
  | "text"; // element text, indentation, inter-attribute whitespace, BOM

export interface Token {
  t: TokenType;
  s: string;
}

const isSpace = (c: string) => c === " " || c === "\t" || c === "\r" || c === "\f" || c === "\v";

/** Consume the inside of a tag (attributes) from `i`, pushing tokens. Returns where it stopped and
 *  whether the tag closed on this line (`>` / `/>`). If it didn't close, the caller stays in TAG
 *  mode for the next line (multi-line open tag). */
function consumeTag(line: string, start: number, out: Token[]): { i: number; closed: boolean } {
  let i = start;
  while (i < line.length) {
    const c = line[i];
    if (c === ">") {
      out.push({ t: "punct", s: ">" });
      return { i: i + 1, closed: true };
    }
    if (c === "/" && line[i + 1] === ">") {
      out.push({ t: "punct", s: "/>" });
      return { i: i + 2, closed: true };
    }
    if (isSpace(c)) {
      let j = i + 1;
      while (j < line.length && isSpace(line[j])) j++;
      out.push({ t: "text", s: line.slice(i, j) });
      i = j;
      continue;
    }
    if (c === "=") {
      out.push({ t: "punct", s: "=" });
      i += 1;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < line.length && line[j] !== c) j++;
      const end = j < line.length ? j + 1 : j; // include the closing quote when present
      out.push({ t: "val", s: line.slice(i, end) });
      i = end;
      continue;
    }
    // attribute name (run up to whitespace, '=', '>' or '/>')
    let j = i;
    while (
      j < line.length &&
      !isSpace(line[j]) &&
      line[j] !== "=" &&
      line[j] !== ">" &&
      !(line[j] === "/" && line[j + 1] === ">")
    ) {
      j++;
    }
    if (j > i) {
      out.push({ t: "attr", s: line.slice(i, j) });
      i = j;
      continue;
    }
    // safety net: emit the single char so the verbatim invariant always holds
    out.push({ t: "text", s: c });
    i += 1;
  }
  return { i, closed: false };
}

type Mode = "text" | "tag" | "comment";

/** Tokenize one line given the carried mode; returns the line's tokens and the mode for the next line. */
function tokenizeLine(line: string, modeIn: Mode): { tokens: Token[]; mode: Mode } {
  const out: Token[] = [];
  let mode = modeIn;
  let i = 0;

  while (i < line.length) {
    if (mode === "comment") {
      const end = line.indexOf("-->", i);
      if (end === -1) {
        out.push({ t: "comment", s: line.slice(i) });
        i = line.length;
      } else {
        out.push({ t: "comment", s: line.slice(i, end + 3) });
        i = end + 3;
        mode = "text";
      }
      continue;
    }

    if (mode === "tag") {
      const res = consumeTag(line, i, out);
      i = res.i;
      mode = res.closed ? "text" : "tag";
      continue;
    }

    // mode === "text"
    const lt = line.indexOf("<", i);
    if (lt === -1) {
      out.push({ t: "text", s: line.slice(i) });
      i = line.length;
      continue;
    }
    if (lt > i) out.push({ t: "text", s: line.slice(i, lt) }); // indentation / BOM / text

    if (line.startsWith("<!--", lt)) {
      const end = line.indexOf("-->", lt);
      if (end === -1) {
        out.push({ t: "comment", s: line.slice(lt) });
        i = line.length;
        mode = "comment";
      } else {
        out.push({ t: "comment", s: line.slice(lt, end + 3) });
        i = end + 3;
      }
      continue;
    }
    if (line.startsWith("<?", lt)) {
      const end = line.indexOf("?>", lt);
      const stop = end === -1 ? line.length : end + 2;
      out.push({ t: "prolog", s: line.slice(lt, stop) });
      i = stop;
      continue;
    }
    if (line.startsWith("<!", lt)) {
      const end = line.indexOf(">", lt);
      const stop = end === -1 ? line.length : end + 1;
      out.push({ t: "prolog", s: line.slice(lt, stop) });
      i = stop;
      continue;
    }

    // normal element tag: lead punct + name, then continue in TAG mode
    if (line.startsWith("</", lt)) {
      out.push({ t: "punct", s: "</" });
      i = lt + 2;
    } else {
      out.push({ t: "punct", s: "<" });
      i = lt + 1;
    }
    let j = i;
    while (
      j < line.length &&
      !isSpace(line[j]) &&
      line[j] !== ">" &&
      !(line[j] === "/" && line[j + 1] === ">")
    ) {
      j++;
    }
    if (j > i) out.push({ t: "tag", s: line.slice(i, j) });
    i = j;
    mode = "tag";
  }

  return { tokens: out, mode };
}

/** Tokenize a full (already pretty-printed) XML document into per-line token arrays. */
export function tokenizeLines(xml: string): Token[][] {
  const lines = xml.split("\n");
  const rows: Token[][] = [];
  let mode: Mode = "text";
  for (const line of lines) {
    const { tokens, mode: next } = tokenizeLine(line, mode);
    rows.push(tokens);
    mode = next;
  }
  return rows;
}
