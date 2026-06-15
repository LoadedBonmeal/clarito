/**
 * doc-render/doc-html — wrap the rendered labeled document (`.docv` outerHTML) into a self-contained,
 * print-ready HTML file. Used for "Printează / Salvează PDF": Tauri's WKWebView can't `window.print()`,
 * so we write this HTML to a temp file and open it in the user's default BROWSER, where it auto-prints
 * (`onload`) and "Save as PDF" works reliably. The CSS here is standalone (concrete colors, A4) so the
 * file renders identically anywhere.
 */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const DOC_HTML_CSS = `
* { box-sizing: border-box; }
body { font-family: -apple-system, "Segoe UI", Roboto, Arial, sans-serif; color: #111; margin: 0; padding: 24px; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
.docv { max-width: 800px; margin: 0 auto; }
.docv-title { font-size: 18px; font-weight: 700; text-align: center; margin: 0 0 4px; }
.docv-title-sub { color: #666; font-weight: 500; }
.docv-sec { margin-top: 20px; }
.docv-sec-title { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .05em; color: #888; margin: 0 0 8px; display: flex; gap: 8px; align-items: center; }
.docv-count { font-size: 10px; font-weight: 600; color: #555; background: #f0f0f0; border-radius: 10px; padding: 1px 7px; }
.docv-kv { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 6px 28px; }
.docv-kv-row { display: flex; justify-content: space-between; gap: 12px; font-size: 12.5px; padding: 4px 0; border-bottom: 1px solid #eee; }
.docv-kv-k { color: #555; }
.docv-kv-v { font-weight: 600; text-align: right; }
.docv-parties { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; margin-top: 16px; }
.docv-party { font-size: 12.5px; }
.docv-party-name { font-weight: 700; margin: 2px 0 4px; }
.docv-party-row { color: #555; line-height: 1.5; }
.docv-tbl-wrap { border: 1px solid #ddd; border-radius: 8px; overflow: hidden; }
.docv-tbl { border-collapse: collapse; width: 100%; font-size: 12px; }
.docv-tbl th, .docv-tbl td { text-align: left; padding: 7px 10px; border-bottom: 1px solid #eee; }
.docv-tbl th { background: #f5f5f5; color: #555; font-weight: 600; font-size: 11px; }
.docv-tbl th.r, .docv-tbl td.r { text-align: right; }
.docv-tbl tr:last-child td { border-bottom: none; }
.docv-line-desc { color: #666; font-size: 11px; margin-top: 2px; }
.docv-cols { display: grid; grid-template-columns: 1fr 280px; gap: 24px; align-items: start; margin-top: 20px; }
.docv-cols .docv-sec { margin-top: 0; }
.docv-totals { border: 1px solid #ddd; border-radius: 8px; padding: 12px 14px; }
.docv-tot-row { display: flex; justify-content: space-between; gap: 16px; font-size: 13px; padding: 5px 0; }
.docv-tot-grand { border-top: 1px solid #ddd; margin-top: 4px; padding-top: 9px; font-weight: 700; font-size: 15px; }
.docv-note { font-size: 12px; color: #555; line-height: 1.5; }
@page { size: A4; margin: 14mm; }
@media print { body { padding: 0; } }
`;

/** Build a complete, portable HTML page from the document's `.docv` inner markup. */
export function buildStandaloneHtml(title: string, docvOuterHtml: string, autoPrint = true): string {
  const onload = autoPrint ? ` onload="setTimeout(function(){window.print()},250)"` : "";
  return `<!doctype html>
<html lang="ro">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<style>${DOC_HTML_CSS}</style>
</head>
<body${onload}>
${docvOuterHtml}
</body>
</html>`;
}
