import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

export type ThemeMode = "light" | "dark" | "system";
export type DensityMode = "compact" | "comfortable" | "relaxed";

/** Apply density class to documentElement — called on every density change */
function applyDensity(density: DensityMode) {
  const root = document.documentElement;
  root.classList.remove("density-compact", "density-comfy", "density-relaxed");
  if (density === "compact") root.classList.add("density-compact");
  if (density === "comfortable") root.classList.add("density-comfy");
  // "relaxed" = no extra class (default row heights)
}

interface AppState {
  // Theme
  theme: ThemeMode;
  setTheme: (theme: ThemeMode) => void;

  // Row density
  density: DensityMode;
  setDensity: (density: DensityMode) => void;

  // Sidebar
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;

  // Command palette
  commandOpen: boolean;
  setCommandOpen: (open: boolean) => void;

  // Active company (multi-tenant aware)
  activeCompanyId: string | null;
  setActiveCompanyId: (id: string | null) => void;

  // Last selected invoice (for ribbon actions)
  selectedInvoiceId: string | null;
  setSelectedInvoiceId: (id: string | null) => void;
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      theme: "light",
      setTheme: (theme) => set({ theme }),

      density: "comfortable",
      setDensity: (density) => {
        applyDensity(density);
        set({ density });
      },

      sidebarCollapsed: false,
      toggleSidebar: () =>
        set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
      setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),

      commandOpen: false,
      setCommandOpen: (open) => set({ commandOpen: open }),

      activeCompanyId: null,
      setActiveCompanyId: (id) => set({ activeCompanyId: id }),

      selectedInvoiceId: null,
      setSelectedInvoiceId: (id) => set({ selectedInvoiceId: id }),
    }),
    {
      name: "rofactura-app-state",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        theme: state.theme,
        density: state.density,
        sidebarCollapsed: state.sidebarCollapsed,
        activeCompanyId: state.activeCompanyId,
      }),
      // Redesign rollout (v1): the new monochrome design system is light-first.
      // Reset any previously-persisted "system"/"dark" choice to light once, so
      // the approved light design is what users see. Dark stays available via toggle.
      migrate: (persisted, version) => {
        const s = (persisted ?? {}) as Record<string, unknown>;
        if (version < 1) s.theme = "light";
        return s as unknown as AppState;
      },
    },
  ),
);
