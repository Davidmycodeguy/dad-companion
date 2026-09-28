/** Prices more than this many steps past the 95th percentile are far-out listings. */
const OVERFLOW_AFTER_STEPS = 2;

export interface PriceBin {
  from: number;
  to: number;
  count: number;
  /** The last bin, holding every price from `from` up (far-out high listings). */
  overflow: boolean;
}

/** The smallest of 1, 2, 2.5, 5 × 10ⁿ that is at least `raw` (and at least 1). */
export function niceStep(raw: number): number {
  if (raw <= 1) return 1;
  const power = 10 ** Math.floor(Math.log10(raw));
  const nice = [1, 2, 2.5, 5, 10].find((factor) => factor * power >= raw) ?? 10;
  return nice * power;
}

/**
 * Prices counted into about `target` even, round-numbered bins. The range stops near the 95th
 * percentile; anything far above it lands in one overflow bin so a single silly listing does not
 * squash the chart.
 */
export function priceBins(prices: number[], target = 12): PriceBin[] {
  if (prices.length === 0) return [];
  const sorted = [...prices].sort((a, b) => a - b);
  const low = sorted[0];
  const p95 = sorted[Math.floor((sorted.length - 1) * 0.95)];
  if (p95 === low && sorted[sorted.length - 1] === low) {
    return [{ from: low, to: low, count: sorted.length, overflow: false }];
  }
  const high = sorted[sorted.length - 1];
  const step = niceStep((p95 - low) / target);
  const start = Math.floor(low / step) * step;
  // Only prices well past the 95th percentile go to the overflow bin; a top price just above it
  // gets a regular bin.
  const end = high - p95 <= OVERFLOW_AFTER_STEPS * step ? high : p95;
  const regular = Math.floor((end - start) / step) + 1;
  const bins: PriceBin[] = Array.from({ length: regular }, (_, i) => ({
    from: start + i * step,
    to: start + (i + 1) * step,
    count: 0,
    overflow: false,
  }));
  const overflowFrom = start + regular * step;
  let overflow = 0;
  for (const price of sorted) {
    if (price >= overflowFrom) overflow += 1;
    else bins[Math.floor((price - start) / step)].count += 1;
  }
  if (overflow > 0) bins.push({ from: overflowFrom, to: Infinity, count: overflow, overflow: true });
  return bins;
}
