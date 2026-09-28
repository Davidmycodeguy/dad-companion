import type { Plan, PlanEntry, PriceSource } from "@/lib/lister-api";
import { RARITIES, type Rarity } from "@/lib/rarity";

/** Listing costs this share of the price, rounded up, and at least LISTING_FEE_MIN (not refunded). */
const LISTING_FEE_RATE = 0.05;
const LISTING_FEE_MIN = 15;
/** The game refuses listings above this price. */
export const MAX_LISTING_PRICE = 1_000_000;
/** Most items one run may list, and most listing spots an account has. */
export const MAX_ITEMS_PER_RUN = 40;
export const MAX_UNDERCUT_PCT = 90;

/** What listing at `price` costs, as the game charges it. */
export function listingFee(price: number): number {
  return Math.max(LISTING_FEE_MIN, Math.ceil(price * LISTING_FEE_RATE));
}

/** A copy of `entry` at a new price (and its fee); the lister's suggestion is kept. */
export function withPrice(entry: PlanEntry, price: number): PlanEntry {
  return { ...entry, price, fee: listingFee(price) };
}

/** A price typed by the player: whole gold from 1 to the game's maximum, or null. */
export function parsePrice(text: string): number | null {
  const cleaned = text.replace(/[,\s]/g, "");
  if (!/^\d+$/.test(cleaned)) return null;
  const price = Number(cleaned);
  return price >= 1 && price <= MAX_LISTING_PRICE ? price : null;
}

export interface PlanTotals {
  count: number;
  price: number;
  fee: number;
  net: number;
}

/** What listing the ticked entries asks, costs and pays. */
export function planTotals(entries: PlanEntry[], ticked: Set<string>): PlanTotals {
  return entries
    .filter((entry) => ticked.has(entry.uniqueId))
    .reduce(
      (sum, entry) => ({
        count: sum.count + 1,
        price: sum.price + entry.price,
        fee: sum.fee + entry.fee,
        net: sum.net + entry.price - entry.fee,
      }),
      { count: 0, price: 0, fee: 0, net: 0 },
    );
}

/** The name of a lister rarity id (1 Poor … 8 Artifact). */
export function rarityName(id: number): Rarity {
  return RARITIES[id - 1] ?? "Unknown";
}

/** Rarities the minimum can be set to, as the lister's ids. */
export const MIN_RARITY_OPTIONS = [2, 3, 4, 5, 6, 7].map((id) => ({ id, name: rarityName(id) }));

export const UNDERCUT_PRESETS = [
  { label: "Fast", pct: 10, hint: "Sells fast" },
  { label: "Balanced", pct: 3, hint: "Just under the cheapest comparable listing" },
  { label: "Max", pct: 1, hint: "Keeps the most gold" },
] as const;

export const PRICE_SOURCE_OPTIONS: { value: PriceSource; label: string; hint: string }[] = [
  { value: "live", label: "Live market", hint: "Searches the Marketplace in game before listing" },
  { value: "database", label: "Saved market data", hint: "Uses the listings the app has seen, no game searches" },
  { value: "model", label: "Value formula", hint: "The lowest reasonable price for each item's rolls" },
];

/** The hint an unpriced plan carries until the game prices it. */
const PRICE_FROM_GAME_HINT = "Price from game";

/** `plan` with a pricing run's results merged in: priced entries replace theirs, entries the run
 * couldn't price move to skipped, and entries it wasn't asked to price stay as they were. */
export function mergePriced(plan: Plan, priced: Plan): Plan {
  const pricedById = new Map(priced.entries.map((entry) => [entry.uniqueId, entry]));
  const inPlan = new Set(plan.entries.map((entry) => entry.uniqueId));
  const nowSkipped = priced.skipped.filter((skip) => inPlan.has(skip.uniqueId));
  const skippedIds = new Set(nowSkipped.map((skip) => skip.uniqueId));
  const warnings = plan.warnings.filter((warning) => !warning.includes(PRICE_FROM_GAME_HINT));
  return {
    entries: plan.entries.filter((entry) => !skippedIds.has(entry.uniqueId)).map((entry) => pricedById.get(entry.uniqueId) ?? entry),
    skipped: [...plan.skipped, ...nowSkipped],
    warnings: [...warnings, ...priced.warnings.filter((warning) => !warnings.includes(warning))],
  };
}
