import { describe, expect, it, vi, afterEach } from "vitest";
import { formatError } from "./error-mapper";

// Silence console.error during tests that trigger internal logging
afterEach(() => {
  vi.restoreAllMocks();
});

describe("formatError", () => {
  // ── User-facing kinds ────────────────────────────────────────────────

  it("Validation: returns message from payload", () => {
    expect(formatError({ kind: "Validation", message: "CUI invalid" })).toBe("CUI invalid");
  });

  it("Validation: returns fallback when message is empty", () => {
    expect(formatError({ kind: "Validation", message: "" })).toBe("A apărut o eroare neașteptată.");
  });

  it("NotFound: returns message from payload", () => {
    expect(formatError({ kind: "NotFound", message: "Factura nu există" })).toBe("Factura nu există");
  });

  it("NotFound: returns default RO message when message is absent", () => {
    const result = formatError({ kind: "NotFound", message: "" });
    expect(result).toBe("Element negăsit.");
  });

  it("Conflict: returns message from payload", () => {
    expect(formatError({ kind: "Conflict", message: "Duplicate serie" })).toBe("Duplicate serie");
  });

  it("Conflict: returns default RO message when message is absent", () => {
    expect(formatError({ kind: "Conflict", message: "" })).toBe("Conflict de date.");
  });

  // ── Export-specific kinds ────────────────────────────────────────────

  it("Xlsx: returns Romanian Excel error message and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Xlsx", message: "internal" })).toBe(
      "Eroare la generarea fișierului Excel."
    );
    expect(spy).toHaveBeenCalled();
  });

  it("Xml: returns Romanian XML error message and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Xml", message: "internal" })).toBe(
      "Eroare la generarea fișierului XML."
    );
    expect(spy).toHaveBeenCalled();
  });

  it("Pdf: returns Romanian PDF error message and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Pdf", message: "internal" })).toBe(
      "Eroare la generarea PDF."
    );
    expect(spy).toHaveBeenCalled();
  });

  it("Archive: returns generic RO archive message (contains 'arhivă/backup'), does not echo p.message, and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const result = formatError({ kind: "Archive", message: "/secret/path/backup.zip: permission denied" });
    // Must contain the generic RO phrase — never the internal path/message.
    expect(result).toContain("arhivă/backup");
    expect(result).not.toContain("/secret/path");
    expect(result).not.toContain("permission denied");
    expect(spy).toHaveBeenCalled();
  });

  // ── Internal kinds (generic fallback) ───────────────────────────────

  it("Database: returns custom fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Database", message: "SQL error" }, "Eroare BD")).toBe("Eroare BD");
    expect(spy).toHaveBeenCalled();
  });

  it("Io: returns default fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Io", message: "disk full" })).toBe(
      "A apărut o eroare neașteptată."
    );
    expect(spy).toHaveBeenCalled();
  });

  it("Other: returns fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "Other", message: "unknown" })).toBe(
      "A apărut o eroare neașteptată."
    );
    expect(spy).toHaveBeenCalled();
  });

  it("Unknown kind: returns fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError({ kind: "SomeFutureVariant", message: "x" })).toBe(
      "A apărut o eroare neașteptată."
    );
    expect(spy).toHaveBeenCalled();
  });

  // ── Non-object inputs ────────────────────────────────────────────────

  it("plain string: returned as-is", () => {
    expect(formatError("Ceva a mers prost")).toBe("Ceva a mers prost");
  });

  it("Error instance: returns fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError(new Error("JS crash"))).toBe("A apărut o eroare neașteptată.");
    expect(spy).toHaveBeenCalled();
  });

  it("undefined: returns fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError(undefined)).toBe("A apărut o eroare neașteptată.");
    expect(spy).toHaveBeenCalled();
  });

  it("null: returns fallback and logs", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(formatError(null)).toBe("A apărut o eroare neașteptată.");
    expect(spy).toHaveBeenCalled();
  });

  it("custom fallback is used when provided", () => {
    expect(formatError("direct string", "unused fallback")).toBe("direct string");
  });

  // ── SessionExpired ───────────────────────────────────────────────────

  it("SessionExpired: returns the message from payload directly (user-facing)", () => {
    const result = formatError({
      kind: "SessionExpired",
      message: "Sesiunea a expirat din cauza inactivității. Vă rugăm să vă autentificați din nou.",
    });
    expect(result).toContain("expirat");
    expect(result).not.toBe("A apărut o eroare neașteptată.");
  });

  it("SessionExpired: falls back to hardcoded RO message when payload message is empty", () => {
    const result = formatError({ kind: "SessionExpired", message: "" });
    expect(result).toContain("autentificați");
  });
});
