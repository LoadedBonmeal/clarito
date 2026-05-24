/**
 * platform — detecție SO și formatare scurtături de tastatură.
 *
 * Pe macOS afișăm simbolurile native (⌘ ⌥ ⇧); pe Windows/Linux păstrăm
 * forma "Ctrl+N". Detecția folosește `navigator`, deci funcționează atât în
 * Tauri (WebView) cât și în browser.
 */

export const isMac: boolean =
  typeof navigator !== "undefined" &&
  (navigator.platform.toLowerCase().startsWith("mac") ||
    navigator.userAgent.toLowerCase().includes("mac os"));

/**
 * Convertește o scurtătură în forma potrivită SO-ului curent.
 * Pe Windows/Linux returnează șirul neschimbat.
 * Acceptă atât forma cu plus ("Ctrl+F") cât și cea cu spațiu ("Ctrl F").
 * Tastele fără modificator (F5, "G C", ↑↓) trec neschimbate.
 */
export function fmtShortcut(s: string): string {
  if (!isMac) return s;
  return s
    .replace(/Ctrl\+Shift\+/gi, "⌘⇧")
    .replace(/Ctrl\+Alt\+/gi, "⌘⌥")
    .replace(/Ctrl\+/gi, "⌘")
    .replace(/Ctrl\s+/gi, "⌘")
    .replace(/Alt\+/gi, "⌥")
    .replace(/Shift\+/gi, "⇧");
}
