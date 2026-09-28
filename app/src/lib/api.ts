import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import type { Rarity } from "@/lib/rarity";

export interface AppStatus {
  name: string;
  version: string;
  repo: string;
  dataDir: string;
  items: number;
  icons: number;
  gameRunning: boolean;
}

export interface ItemView {
  id: string;
  name: string;
  rarity: Rarity;
  /** "Sword · One Handed", "Chest · Cloth", "" for loot */
  kind: string;
  itemType: string;
  width: number;
  height: number;
  maxStack: number;
  vendorPrice: number;
  tradable: boolean;
  icon: string | null;
}

/** An item name with its rarity variants, lowest rarity first. */
export interface ItemGroup {
  name: string;
  variants: ItemView[];
}

export type Settings = Record<string, unknown>;

/** Whether live game data is flowing, and why not. */
export interface LiveStatus {
  state: "starting" | "on" | "off";
  adapter: string | null;
  error: string | null;
  /** Npcap is missing or too old: installing it turns live data on. */
  needsNpcap: boolean;
  /** Unix seconds of the last game message. */
  lastMessageAt: number | null;
}

export const api = {
  status: () => invoke<AppStatus>("app_status"),
  settings: () => invoke<Settings>("get_settings"),
  setSetting: (key: string, value: unknown) => invoke<void>("set_setting", { key, value }),
  searchItems: (query: string, limit = 30) => invoke<ItemGroup[]>("search_items", { query, limit }),
  item: (id: string) => invoke<ItemView | null>("get_item", { id }),
  liveStatus: () => invoke<LiveStatus>("live_status"),
};

/** URL of an icon inside the icon pack ("icons/Weapon/ArmingSword_5001.webp"). */
export function iconUrl(path: string): string {
  return convertFileSrc(path, "icon");
}

export const COIN_ICON = "icons/Misc/GoldCoins.webp";

export interface StatView {
  id: string;
  label: string;
  /** As the game shows it: "+1.7%", "+2". */
  text: string;
}

export interface ListingView {
  id: string;
  price: number;
  count: number;
  unitPrice: number;
  rolls: StatView[];
  /** Seconds since it was last seen (for a sale: since it vanished). */
  ageS: number;
}

export interface DayView {
  /** Start of the day, Unix seconds. */
  day: number;
  median: number;
  listings: number;
}

export interface ItemMarket {
  listings: ListingView[];
  listingCount: number;
  lowest: number | null;
  median: number | null;
  newestAgeS: number | null;
  sales: ListingView[];
  salesTracked: boolean;
  days: DayView[];
}

/** How busy one item's market is. */
export interface ItemActivity {
  itemId: string;
  name: string;
  rarity: Rarity;
  icon: string | null;
  count: number;
  /** Unit prices. */
  lowest: number;
  median: number;
}

export interface MarketOverview {
  /** Items with the most listings up for sale. */
  listed: ItemActivity[];
  /** Items with the most probable sales this week. */
  sold: ItemActivity[];
  newestAgeS: number | null;
}

export const marketApi = {
  item: (itemId: string) => invoke<ItemMarket>("item_market", { itemId }),
  overview: () => invoke<MarketOverview>("market_overview"),
  /** Open listings per item id; ids without any are left out. */
  counts: (itemIds: string[]) => invoke<Record<string, number>>("open_listing_counts", { itemIds }),
};

export interface CardRoll {
  label: string;
  /** As the game shows it: "+2%", "+3". */
  value: string;
  /** 0..1 within the stat's range on this item; null when the model has no range. */
  quality: number | null;
  /** Gold this roll adds (or takes) against an average roll. */
  gold: number | null;
  isMax: boolean;
}

/** Everything the hover value card shows for one item. */
export interface HoverCard {
  name: string;
  rarity: Rarity;
  kind: string;
  icon: string | null;
  /** Typical ask for these exact rolls. */
  value: number | null;
  low: number | null;
  high: number | null;
  /** The price that sells quickly. */
  fastSale: number | null;
  /** The fast sale after the listing fee. */
  net: number | null;
  confidence: "high" | "medium" | "low" | null;
  verdict: "list" | "merchant" | "unknown";
  /** Unit prices of other open listings, cheapest first. */
  asks: number[];
  /** How old the asks are when none were read lately. */
  seenAgoS: number | null;
  soldWeek: number;
  pace: string | null;
  trend: number[];
  trendPct: number | null;
  /** What a merchant pays for the whole stack. */
  merchant: number;
  slots: number;
  perSlot: number;
  rolls: CardRoll[];
  /** 0..100 across the rolls. */
  quality: number | null;
  pairs: { label: string; pct: number }[];
  unread: number;
  /** Listings the model learned this item from. */
  listings: number;
}

export interface CardPreview {
  card: HoverCard;
  /** Price of the listing whose rolls the preview uses, if any. */
  listingPrice: number | null;
}

export const hoverApi = {
  /** A card for any item as if hovered, with the rolls of its median-priced open listing. */
  preview: (itemId: string) => invoke<CardPreview>("preview_card", { itemId }),
};

export interface StashSummary {
  id: number;
  label: string;
  items: number;
  /** The locked Seasonal Shared Stash: its items are a preview, not the player's. */
  locked: boolean;
}

export interface CharacterSummary {
  id: string;
  name: string;
  class: string;
  level: number;
  stashes: StashSummary[];
}

export interface StashItem {
  uniqueId: string;
  itemId: string;
  name: string;
  rarity: Rarity;
  icon: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  count: number;
  /** Gold this item is (coins, or what a coin purse, pouch or bag holds). */
  gold: number | null;
  /** What the price model expects this exact item to sell for; null without market data or for gold. */
  value: number | null;
  /** What a merchant pays for the whole stack. */
  merchant: number;
  tradable: boolean;
}

export interface StashView {
  label: string;
  /** [width, height] in cells; null for stashes without a grid (equipment). */
  grid: [number, number] | null;
  locked: boolean;
  items: StashItem[];
  /** Gold in this stash: coins and what coin containers hold. */
  totalGold: number;
  /** What the other items are worth on the market. */
  totalValue: number;
  totalMerchant: number;
}

/** What one character owns across every stash but the locked seasonal one. */
export interface CharacterWealth {
  id: string;
  name: string;
  class: string;
  level: number;
  gold: number;
  /** What the price model expects the items to sell for. */
  value: number;
  /** What merchants pay for them. */
  merchant: number;
  items: number;
}

export const stashApi = {
  characters: () => invoke<CharacterSummary[]>("characters"),
  wealth: () => invoke<CharacterWealth[]>("wealth"),
  view: (characterId: string, inventoryId: number) => invoke<StashView>("stash_view", { characterId, inventoryId }),
  card: (characterId: string, uniqueId: string) => invoke<HoverCard>("stash_item_card", { characterId, uniqueId }),
};
