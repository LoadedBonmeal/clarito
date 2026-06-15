import { describe, expect, it } from "vitest";

import { buildStandaloneHtml } from "./doc-html";

describe("buildStandaloneHtml", () => {
  it("wraps the document markup into a complete, self-contained HTML page", () => {
    const html = buildStandaloneHtml("d205-2026.xml", '<article class="docv"><h2>X</h2></article>');
    expect(html.startsWith("<!doctype html>")).toBe(true);
    expect(html).toContain("<title>d205-2026.xml</title>");
    expect(html).toContain(".docv"); // inline stylesheet present
    expect(html).toContain('<article class="docv"><h2>X</h2></article>'); // body markup preserved
    expect(html).toContain("window.print()"); // auto-print for the browser
  });

  it("escapes the title and can disable auto-print", () => {
    const html = buildStandaloneHtml('a "b" <c>', "<div/>", false);
    expect(html).toContain("<title>a &quot;b&quot; &lt;c&gt;</title>");
    expect(html).not.toContain("window.print()");
  });
});
