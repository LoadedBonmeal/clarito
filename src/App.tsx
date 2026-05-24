import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";

import { Toaster } from "@/components/ui/sonner";
import { queryClient } from "@/lib/queries";
import { router } from "@/router";
import { isTauriContext } from "@/lib/tauri";

/** Afișat când utilizatorul deschide URL-ul în browser în loc de aplicația nativă. */
function NotTauriScreen() {
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "#0e0e0e",
        fontFamily: "system-ui, sans-serif",
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 400, padding: "40px 24px" }}>
        <div
          style={{
            width: 56,
            height: 56,
            background: "#1a1a2e",
            border: "1px solid #2d2d4e",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 24,
            fontWeight: 700,
            color: "#4f8ef7",
            marginBottom: 20,
            fontFamily: "monospace",
          }}
        >
          eF
        </div>
        <h1 style={{ color: "#f0f0f0", fontSize: 18, fontWeight: 700, margin: "0 0 10px" }}>
          Deschideți aplicația nativă
        </h1>
        <p style={{ color: "#888", fontSize: 13, lineHeight: 1.7, margin: "0 0 6px" }}>
          RoFactura este o aplicație desktop și nu poate rula în browser.
        </p>
        <p style={{ color: "#666", fontSize: 12, lineHeight: 1.6, margin: 0 }}>
          Porniți aplicația din <strong style={{ color: "#888" }}>Finder / Dock</strong> (macOS)
          sau din <strong style={{ color: "#888" }}>meniu Start</strong> (Windows).
        </p>
      </div>
    </div>
  );
}

function App() {
  // Guard: dacă rulăm în browser (dev server deschis direct), afișăm un mesaj clar.
  if (!isTauriContext()) {
    return <NotTauriScreen />;
  }

  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      <Toaster />
    </QueryClientProvider>
  );
}

export default App;
