import { invoke } from "@tauri-apps/api/core";

import type { Rarity } from "@/lib/rarity";

/** Where a quest stands. */
export type QuestState = "locked" | "available" | "active" | "ready" | "done";

/** Who says so: the game's own messages, the player's ticks, or a later quest of the chain. */
export type Source = "game" | "manual" | "implied";

export interface ItemRef {
  /** The catalog item shown; null when the catalog doesn't know it. */
  id: string | null;
  name: string;
  /** The rarity the slot is lit with ("Unknown" when any rarity will do). */
  rarity: Rarity;
  icon: string | null;
}

export interface Place {
  character: string;
  stash: string;
  usable: number;
  other: number;
}

/** What the player owns of what a quest asks for (never the locked seasonal stash). */
export interface Owned {
  /** Looted items that fit: what can be handed in. */
  usable: number;
  /** Items that fit but weren't looted: quests don't take them. */
  other: number;
  places: Place[];
}

export interface ObjectiveView {
  index: number;
  /** "Fetch", "Kill", "Explore", "Use Item", "Props", "Survive", "Hold", "Damage". */
  kind: string;
  label: string;
  count: number;
  submitted: number;
  done: boolean;
  source: Source | null;
  item: ItemRef | null;
  minRarity: Rarity | null;
  owned: Owned | null;
}

export interface RewardView {
  kind: string;
  label: string;
  count: number;
  item: ItemRef | null;
  rarity: Rarity | null;
  gold: boolean;
}

export interface PrerequisiteView {
  id: string;
  title: string;
  merchant: string;
  done: boolean;
}

/** What the game reported for missions that couldn't be tied to one objective. */
export interface GameTally {
  submitted: number;
  needed: number;
}

export interface QuestView {
  id: string;
  title: string;
  text: string | null;
  dungeons: string[];
  state: QuestState;
  source: Source | null;
  timeLimit: "Daily" | "Weekly" | "Seasonal" | null;
  prerequisite: PrerequisiteView | null;
  objectives: ObjectiveView[];
  rewards: RewardView[];
  unmatched: GameTally | null;
  /** When the game last showed this quest (Unix seconds). */
  seenAt: number | null;
}

export interface CurrentQuest {
  id: string;
  title: string;
  state: QuestState;
  waitsFor: string | null;
}

export interface MerchantSummary {
  name: string;
  total: number;
  done: number;
  ready: number;
  active: number;
  available: number;
  locked: number;
  /** The app has read this merchant's quests from the game. */
  tracked: boolean;
  /** A quest is ready to hand in here. */
  turnIn: boolean;
  current: CurrentQuest | null;
  /** Item objectives here the player has looted items for. */
  bring: number;
  seenAt: number | null;
}

export interface StepItem {
  item: ItemRef;
  /** How many to bring now. */
  count: number;
  /** How many the objective still needs. */
  needed: number;
  minRarity: Rarity | null;
}

export interface NextStep {
  merchant: string;
  questId: string;
  questTitle: string;
  action: "turnIn" | "bring";
  items: StepItem[];
}

export interface QuestsOverview {
  merchants: MerchantSummary[];
  nextSteps: NextStep[];
  /** When the game last reported anything about quests (Unix seconds). */
  lastUpdate: number | null;
  characters: number;
}

export interface MerchantQuests {
  merchant: MerchantSummary;
  /** In chain order. */
  quests: QuestView[];
}

export interface NeedQuest {
  questId: string;
  title: string;
  merchant: string;
  remaining: number;
  state: QuestState;
}

export interface ItemNeed {
  key: string;
  /** Used in the dungeon rather than handed in. */
  useInDungeon: boolean;
  item: ItemRef;
  minRarity: Rarity | null;
  owned: Owned;
  quests: NeedQuest[];
}

export interface QuestItems {
  items: ItemNeed[];
  characters: number;
}

export const questsApi = {
  overview: () => invoke<QuestsOverview>("quests_overview"),
  merchant: (merchant: string) => invoke<MerchantQuests>("quests_merchant", { merchant }),
  items: () => invoke<QuestItems>("quests_items"),
  /** The player's own progress on one objective; `submitted` null means all when done. */
  setObjective: (questId: string, index: number, submitted: number | null, done: boolean) =>
    invoke<void>("quests_set_objective", { questId, index, submitted, done }),
  setDone: (questId: string, done: boolean) => invoke<void>("quests_set_done", { questId, done }),
};
