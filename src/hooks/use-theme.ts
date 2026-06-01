import { useEffect } from "react";
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
    const apply = (mode: "light" | "dark") => {
      root.classList.toggle("dark", mode === "dark");
    };

    if (theme === "system") {
      const mq = window.matchMedia("(prefers-color-scheme: dark)");
      apply(mq.matches ? "dark" : "light");
      const onChange = (e: MediaQueryListEvent) =>
        apply(e.matches ? "dark" : "light");
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    }
    apply(theme);
  }, [theme]);

  // Sync density class on mount + whenever density changes
  useEffect(() => {
    applyDensityClass(density ?? "comfortable");
  }, [density]);

  return { theme, setTheme };
}
