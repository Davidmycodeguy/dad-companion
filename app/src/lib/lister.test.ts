import { describe, expect, it } from "vitest";

import type { Plan, PlanEntry, PlanSkip } from "@/lib/lister-api";
import { listingFee, mergePriced, parsePrice, planTotals, rarityName, withPrice } from "@/lib/lister";

function entry(price: number, fee = listingFee(price)): PlanEntry {
  return {
    uniqueId: `u${price}`,
    name: "Heater Shield",
    rarity: 4,
    stashId: "2",
    slotId: 0,
    width: 2,
    height: 2,
    price,
    fee,
    vendorPrice: 10,
    itemId: "HeaterShield_4001",
    baseRolls: [],
    rolls: [],
    flag: "",
    compared: "",
    confidence: "high",
    quantity: 1,
    recommended: price,
  };
}

describe("listingFee", () => {
  it("charges 5% rounded up, at least 15 gold, like the game", () => {
    expect(listingFee(100)).toBe(15);
    expect(listingFee(300)).toBe(15);
    expect(listingFee(301)).toBe(16);
    expect(listingFee(1000)).toBe(50);
    expect(listingFee(1001)).toBe(51);
  });
});

describe("withPrice", () => {
  it("returns a copy with the new price and its fee, keeping the suggestion", () => {
    const original = entry(1000);
    const edited = withPrice(original, 1500);
    expect(edited).toMatchObject({ price: 1500, fee: 75, recommended: 1000 });
    expect(original.price).toBe(1000);
  });
});

describe("parsePrice", () => {
  it("accepts whole gold within the game's limits", () => {
    expect(parsePrice("1,250")).toBe(1250);
    expect(parsePrice(" 900 ")).toBe(900);
  });

  it("rejects empty, zero, fractional and too-high prices", () => {
    expect(parsePrice("")).toBeNull();
    expect(parsePrice("0")).toBeNull();
    expect(parsePrice("12.5")).toBeNull();
    expect(parsePrice("abc")).toBeNull();
    expect(parsePrice("1000001")).toBeNull();
  });
});

describe("planTotals", () => {
  it("sums only the ticked entries", () => {
    const entries = [entry(1000), entry(200), entry(4000)];
    const totals = planTotals(entries, new Set(["u1000", "u4000"]));
    expect(totals).toEqual({ count: 2, price: 5000, fee: 250, net: 4750 });
  });

  it("is all zeros when nothing is ticked", () => {
    expect(planTotals([entry(1000)], new Set())).toEqual({ count: 0, price: 0, fee: 0, net: 0 });
  });
});

describe("rarityName", () => {
  it("maps the lister's rarity ids to names", () => {
    expect(rarityName(2)).toBe("Common");
    expect(rarityName(4)).toBe("Rare");
    expect(rarityName(8)).toBe("Artifact");
    expect(rarityName(0)).toBe("Unknown");
    expect(rarityName(99)).toBe("Unknown");
  });
});

describe("mergePriced", () => {
  const plan = (entries: PlanEntry[], skipped: PlanSkip[] = [], warnings: string[] = []): Plan => ({ entries, skipped, warnings });
  const skip = (uniqueId: string, reason: string): PlanSkip => ({
    name: "Robe",
    stashId: "2",
    slotId: 0,
    reason,
    flag: "",
    confidence: "",
    uniqueId,
    merchant: false,
  });

  it("replaces priced entries, moves unpriceable ones to skipped and keeps the rest", () => {
    const current = plan([entry(0, 0), { ...entry(0, 0), uniqueId: "b" }, { ...entry(0, 0), uniqueId: "c" }], [skip("z", "gold is never listed")], [
      'Prices will come from the in-game market — click "Price from game".',
      "Stash data is 7 minutes old — reopen your character to refresh.",
    ]);
    const priced = plan([{ ...entry(900), uniqueId: "u0" }], [skip("b", "nobody is selling this right now")], ["1 price(s) need your check."]);
    const merged = mergePriced(current, priced);
    expect(merged.entries.map((e) => [e.uniqueId, e.price])).toEqual([
      ["u0", 900],
      ["c", 0],
    ]);
    expect(merged.skipped.map((s) => s.uniqueId)).toEqual(["z", "b"]);
    expect(merged.warnings).toEqual(["Stash data is 7 minutes old — reopen your character to refresh.", "1 price(s) need your check."]);
  });
});
