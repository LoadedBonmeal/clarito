import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

export type ThemeMode = "light" | "dark" | "system";

interface AppState {
  // Theme
  theme: ThemeMode;
  setTheme: (theme: ThemeMode) => void;

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
      theme: "system",
      setTheme: (theme) => set({ theme }),

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
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        theme: state.theme,
        sidebarCollapsed: state.sidebarCollapsed,
        activeCompanyId: state.activeCompanyId,
      }),
    },
  ),
);
