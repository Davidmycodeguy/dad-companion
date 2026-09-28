import { describe, expect, it } from "vitest";

import { niceStep, priceBins } from "@/lib/price-bins";

describe("niceStep", () => {
  it("rounds up to 1, 2, 2.5 or 5 times a power of ten", () => {
    expect(niceStep(7)).toBe(10);
    expect(niceStep(180)).toBe(200);
    expect(niceStep(230)).toBe(250);
    expect(niceStep(420)).toBe(500);
    expect(niceStep(0.3)).toBe(1);
  });
});

describe("priceBins", () => {
  it("is empty without prices", () => {
    expect(priceBins([])).toEqual([]);
  });

  it("puts equal prices in one bin", () => {
    expect(priceBins([300, 300, 300])).toEqual([{ from: 300, to: 300, count: 3, overflow: false }]);
  });

  it("counts prices per even step starting at a round number", () => {
    const bins = priceBins([1000, 1100, 1250, 1900, 2000], 5);
    expect(bins.map((bin) => [bin.from, bin.count])).toEqual([
      [1000, 2],
      [1200, 1],
      [1400, 0],
      [1600, 0],
      [1800, 1],
      [2000, 1],
    ]);
    expect(bins.every((bin) => bin.to - bin.from === 200)).toBe(true);
  });

  it("gathers far-out high listings into one last bin", () => {
    const prices = [...Array.from({ length: 19 }, (_, i) => 1000 + i * 10), 50_000];
    const bins = priceBins(prices, 4);
    const last = bins[bins.length - 1];
    expect(last).toMatchObject({ overflow: true, count: 1 });
    expect(bins.reduce((sum, bin) => sum + bin.count, 0)).toBe(20);
    expect(bins.length).toBeLessThanOrEqual(6);
  });
});
