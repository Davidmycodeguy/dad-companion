import { describe, expect, it } from "vitest";

import { highestRarity, rarityColor, singleRarity } from "@/lib/rarity";

describe("rarity helpers", () => {
  it("finds the highest rarity, ignoring Unknown", () => {
    expect(highestRarity(["Common", "Epic", "Rare"])).toBe("Epic");
    expect(highestRarity(["Unknown", "Poor"])).toBe("Poor");
    expect(highestRarity([])).toBe("Unknown");
  });

  it("names a single rarity only when every variant shares it", () => {
    expect(singleRarity(["Rare"])).toBe("Rare");
    expect(singleRarity(["Rare", "Rare"])).toBe("Rare");
    expect(singleRarity(["Rare", "Epic"])).toBeNull();
    expect(singleRarity([])).toBeNull();
  });

  it("maps a rarity to its CSS colour variable", () => {
    expect(rarityColor("Legendary")).toBe("var(--rarity-legendary)");
  });
});
