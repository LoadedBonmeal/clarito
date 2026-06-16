import { describe, expect, it } from "vitest";

import { pickDescriptor } from "./descriptors";

describe("pickDescriptor", () => {
  it("selects by document key (declKind / docKey)", () => {
    expect(pickDescriptor("D205", "declaratie205")?.key).toBe("D205");
    expect(pickDescriptor("INVOICE", "Invoice")?.key).toBe("INVOICE");
    expect(pickDescriptor("D112", "declaratieUnica")?.key).toBe("D112");
  });
  it("selects by root tag when no key is given", () => {
    expect(pickDescriptor(undefined, "declaratie205")?.key).toBe("D205");
    expect(pickDescriptor(undefined, "Invoice")?.key).toBe("INVOICE");
    expect(pickDescriptor(undefined, "declaratieUnica")?.key).toBe("D112");
    expect(pickDescriptor(undefined, "declaratie300")?.key).toBe("D300");
    expect(pickDescriptor(undefined, "declaratie390")?.key).toBe("D390");
    expect(pickDescriptor(undefined, "declaratie394")?.key).toBe("D394");
    expect(pickDescriptor(undefined, "AuditFile")?.key).toBe("D406"); // SAF-T summary
  });
  it("selects the SAF-T summary by declaration key", () => {
    expect(pickDescriptor("D406", "AuditFile")?.key).toBe("D406");
  });
  it("returns null for not-yet-described documents (→ generic table fallback)", () => {
    expect(pickDescriptor(undefined, "habarnam")).toBeNull();
  });
});
