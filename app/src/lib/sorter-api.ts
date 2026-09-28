import { invoke } from "@tauri-apps/api/core";

import type { Rarity } from "@/lib/rarity";

export type SortField = "width" | "height" | "slot" | "rarity" | "name";
export type SortDirection = "asc" | "desc";

/** One sort-order entry; the first entry decides first. */
export interface OrderEntry {
  field: SortField;
  direction: SortDirection;
}

/** What the player chooses before sorting (saved in settings). */
export interface SortOptions {
  /** Pack tightly. */
  pack: boolean;
  /** Merge partial stacks of the same item first. */
  stack: boolean;
  /** Bring the bag's items over into the stash. */
  fromBag: boolean;
  order: OrderEntry[];
}

export interface SorterOptionsView {
  options: SortOptions;
  /** The player confirmed the automated-input notice once already. */
  riskAccepted: boolean;
}

/** One item drawn in a grid. */
export interface GridItem {
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
  /** Sorted grid: ends somewhere else than now. Bag: comes over into the stash. */
  moves: boolean;
}

export interface SortPreview {
  stashLabel: string;
  /** [width, height] in cells. */
  grid: [number, number];
  bagGrid: [number, number];
  current: GridItem[];
  bag: GridItem[];
  /** Empty when no plan could be made (see `blocked`). */
  sorted: GridItem[];
  /** Drags the sort makes, of which merges and trips into or out of the bag. */
  moves: number;
  merges: number;
  bagMoves: number;
  /** Items that end up elsewhere, and items coming over from the bag. */
  moving: number;
  incoming: number;
  /** Why Sort can't start right now. */
  blocked: string | null;
  /** Seconds since the game last sent this character. */
  dataAgeS: number | null;
}

export type SorterState = "idle" | "running" | "done" | "stopped";

/** The current or last run. */
export interface SorterStatus {
  state: SorterState;
  characterId: string | null;
  stashId: number | null;
  stashLabel: string | null;
  /** What the run is doing between moves. */
  phase: string | null;
  /** The item being moved right now. */
  current: string | null;
  done: number;
  total: number;
  verified: number;
  failed: number;
  retried: number;
  stoppedReason: string | null;
  /** The learning session, for feedback; null when learning is off. */
  sessionId: string | null;
  feedbackSent: boolean;
  /** Whether Ctrl+F12 stops this run. */
  hotkey: boolean;
  /** Another feature drives the mouse right now ("The auto lister"). */
  busyWith: string | null;
  /** Unix seconds when the run ended. */
  finishedAt: number | null;
}

export const sorterApi = {
  options: () => invoke<SorterOptionsView>("sorter_options"),
  saveOptions: (options: SortOptions) => invoke<SortOptions>("sorter_save_options", { options }),
  preview: (characterId: string, stashId: number, options: SortOptions) =>
    invoke<SortPreview>("sorter_preview", { characterId, stashId, options }),
  start: (characterId: string, stashId: number, options: SortOptions, acceptRisk: boolean) =>
    invoke<void>("sorter_start", { characterId, stashId, options, acceptRisk }),
  stop: () => invoke<boolean>("sorter_stop"),
  status: () => invoke<SorterStatus>("sorter_status"),
  feedback: (sessionId: string, success: boolean, note: string | null) =>
    invoke<boolean>("sorter_feedback", { sessionId, success, note }),
};
