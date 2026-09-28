import { invoke } from "@tauri-apps/api/core";

import type { Rarity } from "@/lib/rarity";

/** Where the lister's prices come from. */
export type PriceSource = "live" | "database" | "model";

export interface ListerRules {
  /** Stash tabs to list from, as the game's inventory ids ("2" is the bag). */
  sourceStashIds: string[];
  /** 1 Poor … 8 Artifact. */
  minRarity: number;
  minPrice: number;
  undercutPct: number;
  minListings: number;
  maxItemsPerRun: number;
  excludeItemIds: string[];
  /** Price minus fee must be at least this share of the price. */
  minNetRatio: number;
  allowStacks: boolean;
  priceSource: PriceSource;
}

/** A stash tab the lister can take items from. */
export interface ListerSource {
  id: string;
  label: string;
  items: number;
}

/** What the game last showed of the player's listings (Trade → Marketplace → My Listings). */
export interface ListingsInfo {
  /** False until the game has shown the listings screen this session. */
  seen: boolean;
  /** Free listing spots. */
  free: number | null;
  /** Seconds since the listings screen was read. */
  ageS: number | null;
  /** Sold listings waiting to be collected. */
  payouts: number;
  /** Gold those sales pay out. */
  payoutGold: number;
}

export type Confidence = "high" | "medium" | "low" | "";

/** One item the plan lists. Sent back unchanged (bar the price) to price or list it. */
export interface PlanEntry {
  uniqueId: string;
  name: string;
  /** 1 Poor … 8 Artifact. */
  rarity: number;
  stashId: string;
  slotId: number;
  width: number;
  height: number;
  /** Price of the whole listing. */
  price: number;
  fee: number;
  vendorPrice: number;
  itemId: string;
  baseRolls: [string, number][];
  rolls: [string, number][];
  /** A doubt about the price worth a look before listing. */
  flag: string;
  /** What the price was compared with. */
  compared: string;
  confidence: Confidence;
  quantity: number;
  /** The price the lister suggested; differs from `price` once the player edits it. */
  recommended: number;
}

export interface PlanSkip {
  name: string;
  stashId: string;
  slotId: number;
  reason: string;
  flag: string;
  confidence: Confidence;
  uniqueId: string;
  /** A merchant is the better buyer for it. */
  merchant: boolean;
}

export interface Plan {
  entries: PlanEntry[];
  skipped: PlanSkip[];
  warnings: string[];
}

/** Display details the page needs beside a plan: icons and names the plan itself doesn't carry. */
export interface PlanItemInfo {
  icon: string | null;
  rarity: Rarity;
  stashLabel: string;
  /** The item's rolls as the game shows them: "+2 Strength". */
  rolls: string[];
}

export interface PlanResponse {
  plan: Plan;
  /** Keyed by unique id, for entries and skipped items alike. */
  items: Record<string, PlanItemInfo>;
  listings: ListingsInfo;
  /** The entries have no prices yet: the in-game market must price them first. */
  needsGamePricing: boolean;
}

export type RunMode = "list" | "dryRun" | "price" | "crawl" | "collect" | "merchant" | "merchantDryRun" | "hover";

export interface ItemResult {
  uniqueId: string;
  name: string;
  /** "listed", "skipped", "failed", "sold", "dry run", "priced", … */
  status: string;
  message: string;
}

export interface ListerStatus {
  state: "idle" | "running" | "done";
  mode: RunMode | null;
  results: ItemResult[];
  stoppedReason: string | null;
  /** The plan a pricing run produced, once it is done. */
  plan: Plan | null;
  listings: ListingsInfo;
  /** How many items the current run started with. */
  total: number;
  /** Whether the runs that click in the game are available in this build. */
  canRun: boolean;
}

export interface MerchantEntry {
  uniqueId: string;
  name: string;
  stashId: string;
  quantity: number;
  vendorPrice: number;
  /** What the merchant pays for the whole stack. */
  value: number;
}

export interface MerchantPlan {
  merchant: string;
  entries: MerchantEntry[];
  refused: { uniqueId: string; reason: string }[];
  total: number;
  warnings: string[];
}

export interface MarketDataSummary {
  listings: number;
  items: number;
  /** Listings that vanished before expiring: most likely sold. */
  vanished: number;
  mySold: number;
}

export interface WorthInfo {
  trained: boolean;
  /** Listings it learned from. */
  listings: number;
  items: number;
  /** Unix seconds; null when unknown. */
  trainedAt: number | null;
  /** Median error on held-out listings, 0..1. */
  mdape: number | null;
}

export const listerApi = {
  rules: () => invoke<ListerRules>("lister_rules"),
  saveRules: (rules: ListerRules) => invoke<ListerRules>("save_lister_rules", { rules }),
  sources: (characterId: string) => invoke<ListerSource[]>("lister_sources", { characterId }),
  buildPlan: (characterId: string, rules: ListerRules) => invoke<PlanResponse>("lister_build_plan", { characterId, rules }),
  status: () => invoke<ListerStatus>("lister_status"),
  priceFromGame: (entries: PlanEntry[]) => invoke<void>("lister_price_from_game", { entries }),
  start: (characterId: string, entries: PlanEntry[], dryRun: boolean, recheck: boolean) =>
    invoke<void>("lister_start", { characterId, entries, dryRun, recheck }),
  /** The calibration check: the mouse rests on each Marketplace spot, no clicks. */
  hoverTest: () => invoke<void>("lister_hover_test"),
  stop: () => invoke<boolean>("lister_stop"),
  collect: () => invoke<void>("lister_collect"),
  crawl: (deep: boolean) => invoke<void>("lister_crawl", { deep }),
  merchantPlan: (characterId: string, uniqueIds: string[]) =>
    invoke<MerchantPlan>("lister_merchant_plan", { characterId, uniqueIds }),
  sellToMerchant: (characterId: string, uniqueIds: string[], dryRun: boolean) =>
    invoke<void>("lister_sell_to_merchant", { characterId, uniqueIds, dryRun }),
  marketData: () => invoke<MarketDataSummary>("market_data_summary"),
  worthInfo: () => invoke<WorthInfo>("worth_info"),
  trainWorth: () => invoke<WorthInfo>("train_worth"),
};
