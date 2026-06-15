import { describe, expect, it } from "vitest";

import { pickDescriptor } from "./descriptors";

describe("pickDescriptor", () => {
  it("selects by document key (declKind / docKey)", () => {
    expect(pickDescriptor("D205", "declaratie205")?.key).toBe("D205");
    expect(pickDescriptor("INVOICE", "Invoice")?.key).toBe("INVOICE");
  });
  it("selects by root tag when no key is given", () => {
    expect(pickDescriptor(undefined, "declaratie205")?.key).toBe("D205");
    expect(pickDescriptor(undefined, "Invoice")?.key).toBe("INVOICE");
  });
  it("returns null for not-yet-described documents (→ generic table fallback)", () => {
    expect(pickDescriptor("D300", "declaratie300")).toBeNull(); // Phase 2
    expect(pickDescriptor(undefined, "habarnam")).toBeNull();
  });
});
