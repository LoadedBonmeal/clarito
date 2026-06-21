/**
 * Auth session Zustand store — P2 Wave 8.
 *
 * Holds the current user after login. NOT persisted across restarts
 * (session is re-validated on startup via auth_status from AppState).
 */

import { create } from "zustand";
import type { CurrentUser } from "@/types";

interface AuthState {
  currentUser: CurrentUser | null;
  setCurrentUser: (user: CurrentUser) => void;
  clearCurrentUser: () => void;
  /** Convenience: the current user's role, or null if not logged in. */
  role: CurrentUser["role"] | null;
}

export const useAuthStore = create<AuthState>()((set) => ({
  currentUser: null,
  role: null,
  setCurrentUser: (user) => set({ currentUser: user, role: user.role }),
  clearCurrentUser: () => set({ currentUser: null, role: null }),
}));
