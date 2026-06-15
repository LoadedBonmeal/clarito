/**
 * formatXml — a small, dependency-free XML pretty-printer for the in-app XML viewer/editor.
 *
 * Re-indents well-formed XML (the declaration / e-Factura XML this app emits) by element depth.
 * Works whether the input is already pretty-printed or all on one line: it first puts each tag on
 * its own line, then walks the lines tracking depth. The `<?xml?>` prolog, comments, self-closing
 * tags, and single-line "<a>text</a>" elements keep their own indent without opening a new level.
 *
 * It is deliberately tolerant — it never throws and, on input it can't classify cleanly, still
 * returns a reasonable layout. The authoritative correctness check is the DUK re-validate, not this.
 */
export function formatXml(input: string, indentUnit = "  "): string {
  const xml = input.trim();
  if (!xml) return "";

  // 1. Break between adjacent tags ("><" → ">\n<"). Inter-tag whitespace/newlines collapse first,
  //    so this is idempotent. Text content ("<a>text</a>") has non-space between > and < and is
  //    therefore left on a single line.
  const withBreaks = xml.replace(/\r\n?/g, "\n").replace(/>\s+</g, "><").replace(/></g, ">\n<");

  // 2. Re-indent line by line.
  let depth = 0;
  const out: string[] = [];
  for (const raw of withBreaks.split("\n")) {
    const line = raw.trim();
    if (!line) continue;

    const isClosing = /^<\//.test(line);
    const isProlog = /^<\?/.test(line); // <?xml ... ?>
    const isComment = /^<!--/.test(line); // <!-- ... -->
    const isDoctype = /^<!/.test(line) && !isComment; // <!DOCTYPE ...>
    const isOpening = /^<[^/!?]/.test(line);
    // Opens AND closes on the same line: self-closing ("<a/>"), or "<a ...>text</a>".
    const selfContained =
      isProlog ||
      isComment ||
      isDoctype ||
      /\/>\s*$/.test(line) ||
      /^<([\w:.-]+)(\s[^>]*)?>.*<\/\1>\s*$/.test(line);

    if (isClosing) depth = Math.max(depth - 1, 0);
    out.push(indentUnit.repeat(depth) + line);
    if (isOpening && !selfContained && !isClosing) depth += 1;
  }
  return out.join("\n");
}
