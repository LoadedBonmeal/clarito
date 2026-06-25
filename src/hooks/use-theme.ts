import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAppStore, type DensityMode } from "@/lib/store";

function applyDensityClass(density: DensityMode) {
  const root = document.documentElement;
  root.classList.remove("density-compact", "density-comfy", "density-relaxed");
  if (density === "compact") root.classList.add("density-compact");
  if (density === "comfortable") root.classList.add("density-comfy");
  // "relaxed" uses default row heights — no extra class
}

export function useTheme() {
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const density = useAppStore((s) => s.density);

  useEffect(() => {
    const root = document.documentElement;
    const apply = (mode: "light" | "dark") =>
      root.classList.toggle("dark", mode === "dark");

    // Explicit user choice — just apply it.
    if (theme !== "system") {
      apply(theme);
      return;
    }

    // "system" → track the OS appearance dynamically. WKWebView's
    // prefers-color-scheme is unreliable on macOS (it can stay stuck on light
    // even when the OS is dark), so use Tauri's NATIVE window theme API
    // (NSWindow.effectiveAppearance) as the source of truth, and keep
    // matchMedia only as a fallback signal.
    let cancelled = false;
    let unlistenTheme: (() => void) | undefined;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onMq = (e: MediaQueryListEvent) => apply(e.matches ? "dark" : "light");

    const win = getCurrentWindow();
    win
      .theme()
      .then((t) => { if (!cancelled) apply(t === "dark" ? "dark" : "light"); })
      .catch(() => { if (!cancelled) apply(mq.matches ? "dark" : "light"); });
    win
      .onThemeChanged(({ payload }) =>
        apply(payload === "dark" ? "dark" : "light"),
      )
      .then((u) => { if (cancelled) u(); else unlistenTheme = u; })
      .catch(() => {});
    mq.addEventListener("change", onMq);

    return () => {
      cancelled = true;
      unlistenTheme?.();
      mq.removeEventListener("change", onMq);
    };
  }, [theme]);

  // Sync density class on mount + whenever density changes
  useEffect(() => {
    applyDensityClass(density ?? "comfortable");
  }, [density]);

  return { theme, setTheme };
}
