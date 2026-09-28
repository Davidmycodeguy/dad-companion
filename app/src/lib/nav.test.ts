import { describe, expect, it } from "vitest";

import { isActivePath } from "@/lib/nav";

describe("isActivePath", () => {
  it("matches a section and the pages inside it", () => {
    expect(isActivePath("/market", "/market")).toBe(true);
    expect(isActivePath("/market/Arming%20Sword", "/market")).toBe(true);
    expect(isActivePath("/marketplace", "/market")).toBe(false);
  });

  it("only matches the overview on the root itself", () => {
    expect(isActivePath("/", "/")).toBe(true);
    expect(isActivePath("/market", "/")).toBe(false);
  });
});
