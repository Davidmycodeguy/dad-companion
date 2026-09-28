import type { ItemNeed, NeedQuest, ObjectiveView, QuestState, QuestView } from "@/lib/quests-api";

export type QuestFilter = "active" | "completed";

/** Portraits in public/merchants, by merchant name as the quest catalog spells it. */
const PORTRAITS: Record<string, string> = {
  "Tavern Master": "tavern_master.png",
  Alchemist: "alchemist.png",
  Armourer: "armourer.png",
  Cockatrice: "cockatrice_merchant.png",
  Dealmaker: "dealmaker.png",
  "Goblin Merchant": "goblin_merchant.png",
  Goldsmith: "goldsmith.png",
  Huntress: "huntress.png",
  "Jack O Lantern": "jack_o_lantern.png",
  Krampus: "krampus.png",
  Leathersmith: "leathersmith.png",
  Navigator: "navigator.png",
  Nicholas: "nicholas.png",
  "Nightmare Mummy": "nightmare_mummy.png",
  "Skeleton Footman": "skeleton_merchant.png",
  Squire: "squire.png",
  Tailor: "tailor.png",
  "The Collector": "the_collector.png",
  Valentine: "valentine.png",
  Weaponsmith: "weaponsmith.png",
  Woodsman: "woodsman.png",
};

/** The merchant's portrait URL, or null when there is none. */
export function merchantPortrait(name: string): string | null {
  const file = PORTRAITS[name];
  return file ? `/merchants/${file}` : null;
}

/** A quest state in plain words; open quests the game hasn't shown have no label. */
export function stateLabel(state: QuestState, fromGame: boolean): string | null {
  switch (state) {
    case "locked":
      return "Locked";
    case "available":
      return fromGame ? "Not accepted" : null;
    case "active":
      return "In progress";
    case "ready":
      return "Ready to turn in";
    case "done":
      return "Done";
  }
}

/** Open quests (without locked ones when asked), or finished ones. */
export function filterQuests(quests: QuestView[], filter: QuestFilter, hideLocked: boolean): QuestView[] {
  return quests.filter((quest) =>
    filter === "completed" ? quest.state === "done" : quest.state !== "done" && !(hideLocked && quest.state === "locked"),
  );
}

/** How many more an objective needs. */
export function remaining(objective: ObjectiveView): number {
  return objective.done ? 0 : Math.max(0, objective.count - objective.submitted);
}

/** What an unfinished objective still asks: "Bring 3 more", "Use 1 more", "2 more to kill". */
export function objectiveHint(objective: ObjectiveView): string | null {
  const left = remaining(objective);
  if (left === 0) return null;
  if (objective.kind === "Fetch") return `Bring ${left} more`;
  if (objective.kind === "Use Item") return `Use ${left} more`;
  if (objective.kind === "Kill") return `${left} more to kill`;
  return null;
}

/** The player has looted enough of an item objective's item to finish it. */
export function hasEnough(objective: ObjectiveView): boolean {
  const left = remaining(objective);
  return left > 0 && (objective.owned?.usable ?? 0) >= left;
}

export interface QuestCounts {
  shown: number;
  objectivesLeft: number;
  /** Items still to bring over the shown quests. */
  itemsLeft: number;
  locked: number;
}

/** Totals over the quests a list shows. */
export function countQuests(quests: QuestView[]): QuestCounts {
  let objectivesLeft = 0;
  let itemsLeft = 0;
  for (const quest of quests) {
    for (const objective of quest.objectives) {
      if (objective.done) continue;
      objectivesLeft += 1;
      if (objective.kind === "Fetch") itemsLeft += remaining(objective);
    }
  }
  return { shown: quests.length, objectivesLeft, itemsLeft, locked: quests.filter((q) => q.state === "locked").length };
}

export interface NeedRow {
  need: ItemNeed;
  /** The quests counted (without locked ones when they're hidden). */
  quests: NeedQuest[];
  needed: number;
  /** Locked quests left out. */
  hidden: number;
}

export interface ChecklistOptions {
  search: string;
  hideLocked: boolean;
  ownedFirst: boolean;
}

/** The checklist as shown: locked quests left out when hidden, filtered by item, merchant or quest
 * name. Items open quests need come before items only locked quests need (items the player has
 * looted first, when asked); otherwise the server's order (by name) stands. */
export function checklistRows(items: ItemNeed[], { search, hideLocked, ownedFirst }: ChecklistOptions): NeedRow[] {
  const query = search.trim().toLowerCase();
  const rows = items
    .map((need) => {
      const quests = hideLocked ? need.quests.filter((q) => q.state !== "locked") : need.quests;
      const needed = quests.reduce((sum, q) => sum + q.remaining, 0);
      return { need, quests, needed, hidden: need.quests.length - quests.length };
    })
    .filter((row) => row.quests.length > 0)
    .filter(
      (row) =>
        !query ||
        row.need.item.name.toLowerCase().includes(query) ||
        row.quests.some((q) => q.merchant.toLowerCase().includes(query) || q.title.toLowerCase().includes(query)),
    );
  const lockedOnly = (row: NeedRow) => (row.quests.every((q) => q.state === "locked") ? 1 : 0);
  rows.sort(
    (a, b) => (ownedFirst ? b.need.owned.usable - a.need.owned.usable : 0) || lockedOnly(a) - lockedOnly(b),
  );
  return rows;
}

/** Seconds since a Unix time (in seconds), never negative. */
export function secondsSince(unixS: number, nowS: number = Date.now() / 1000): number {
  return Math.max(0, nowS - unixS);
}
