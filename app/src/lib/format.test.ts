import { describe, expect, it } from "vitest";

import { formatAge, formatGold } from "@/lib/format";

describe("formatGold", () => {
  it("shows whole coins with thousands separators", () => {
    expect(formatGold(0)).toBe("0");
    expect(formatGold(12400)).toBe("12,400");
    expect(formatGold(1234.6)).toBe("1,235");
  });
});

describe("formatAge", () => {
  it("rounds down to the largest whole unit", () => {
    expect(formatAge(5)).toBe("just now");
    expect(formatAge(59 * 60 + 59)).toBe("59 min ago");
    expect(formatAge(3 * 3600 + 10)).toBe("3 h ago");
    expect(formatAge(2 * 86400 + 5)).toBe("2 d ago");
  });
});
