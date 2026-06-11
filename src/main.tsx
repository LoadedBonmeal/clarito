import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "@/lib/i18n";
import "@/styles/globals.css";
import { isDemoMode } from "@/lib/demo";
import { useAppStore } from "@/lib/store";

// Demo harness (dev-only, ?demo=1): pre-select the fixture company + light theme
// so the UI renders populated, exactly like the design prototypes.
if (isDemoMode()) {
  useAppStore.setState({ activeCompanyId: "demo-co", theme: "light", sidebarCollapsed: false });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
