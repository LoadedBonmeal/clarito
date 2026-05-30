/**
 * platform — detecție SO și formatare scurtături de tastatură.
 *
 * Pe macOS afișăm simbolurile native (⌘ ⌥ ⇧); pe Windows/Linux păstrăm
 * forma "Ctrl+N". Detecție bazată pe userAgentData (modern) cu fallback
 * pe userAgent — navigator.platform e deprecat.
 */

interface NavigatorUAData {
  platform?: string;
}

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const uaData = (navigator as Navigator & { userAgentData?: NavigatorUAData })
    .userAgentData;
  if (uaData?.platform) {
    return uaData.platform.toLowerCase().includes("mac");
  }
  return navigator.userAgent.toLowerCase().includes("mac os");
}

export const isMac: boolean = detectMac();

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
