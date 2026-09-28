import { describe, expect, it } from "vitest";

import type { ItemNeed, NeedQuest, ObjectiveView, QuestState, QuestView } from "@/lib/quests-api";
import {
  checklistRows,
  countQuests,
  filterQuests,
  hasEnough,
  merchantPortrait,
  objectiveHint,
  remaining,
  secondsSince,
  stateLabel,
} from "@/lib/quests";

function objective(overrides: Partial<ObjectiveView> = {}): ObjectiveView {
  return {
    index: 0,
    kind: "Fetch",
    label: "Bring 3 Bandage",
    count: 3,
    submitted: 1,
    done: false,
    source: null,
    item: { id: "Bandage_1001", name: "Bandage", rarity: "Unknown", icon: null },
    minRarity: null,
    owned: { usable: 2, other: 1, places: [] },
    ...overrides,
  };
}

function quest(id: string, state: QuestState, objectives: ObjectiveView[] = []): QuestView {
  return {
    id,
    title: id,
    text: null,
    dungeons: [],
    state,
    source: null,
    timeLimit: null,
    prerequisite: null,
    objectives,
    rewards: [],
    unmatched: null,
    seenAt: null,
  };
}

function need(name: string, usable: number, quests: Partial<NeedQuest>[]): ItemNeed {
  return {
    key: `bring|${name}||`,
    useInDungeon: false,
    item: { id: name, name, rarity: "Unknown", icon: null },
    minRarity: null,
    owned: { usable, other: 0, places: [] },
    quests: quests.map((q, i) => ({ questId: `${name}_${i}`, title: `Quest ${i}`, merchant: "Alchemist", remaining: 1, state: "active", ...q })),
  };
}

describe("merchantPortrait", () => {
  it("finds the portrait by the catalog's merchant name", () => {
    expect(merchantPortrait("Tavern Master")).toBe("/merchants/tavern_master.png");
    expect(merchantPortrait("Skeleton Footman")).toBe("/merchants/skeleton_merchant.png");
    expect(merchantPortrait("Expressman")).toBeNull();
  });
});

describe("stateLabel", () => {
  it("names states plainly and leaves open quests the game hasn't shown unlabeled", () => {
    expect(stateLabel("ready", true)).toBe("Ready to turn in");
    expect(stateLabel("available", true)).toBe("Not accepted");
    expect(stateLabel("available", false)).toBeNull();
    expect(stateLabel("locked", false)).toBe("Locked");
  });
});

describe("filterQuests", () => {
  const quests = [quest("a", "done"), quest("b", "active"), quest("c", "locked"), quest("d", "ready")];

  it("shows open quests, without locked ones when asked, or finished ones", () => {
    expect(filterQuests(quests, "active", false).map((q) => q.id)).toEqual(["b", "c", "d"]);
    expect(filterQuests(quests, "active", true).map((q) => q.id)).toEqual(["b", "d"]);
    expect(filterQuests(quests, "completed", true).map((q) => q.id)).toEqual(["a"]);
  });
});

describe("objectives", () => {
  it("say how many more they need", () => {
    expect(remaining(objective())).toBe(2);
    expect(objectiveHint(objective())).toBe("Bring 2 more");
    expect(objectiveHint(objective({ kind: "Kill", owned: null }))).toBe("2 more to kill");
    expect(objectiveHint(objective({ kind: "Use Item" }))).toBe("Use 2 more");
    expect(objectiveHint(objective({ kind: "Explore", count: 1, submitted: 0 }))).toBeNull();
    expect(objectiveHint(objective({ done: true }))).toBeNull();
  });

  it("know when the player has looted enough", () => {
    expect(hasEnough(objective())).toBe(true);
    expect(hasEnough(objective({ submitted: 0 }))).toBe(false);
    expect(hasEnough(objective({ done: true }))).toBe(false);
  });

  it("add up over a list", () => {
    const counts = countQuests([
      quest("a", "active", [objective(), objective({ kind: "Kill", owned: null })]),
      quest("b", "locked", [objective({ done: true })]),
    ]);
    expect(counts).toEqual({ shown: 2, objectivesLeft: 2, itemsLeft: 2, locked: 1 });
  });
});

describe("checklistRows", () => {
  const items = [
    need("Bandage", 0, [{ remaining: 2 }, { remaining: 3, state: "locked" }]),
    need("Diamond", 4, [{ remaining: 1, merchant: "Goldsmith", title: "Hands of Precision" }]),
    need("Ruby", 1, [{ state: "locked" }]),
  ];

  it("leaves locked quests out when hidden and adds up what's still needed", () => {
    const all = checklistRows(items, { search: "", hideLocked: false, ownedFirst: false });
    expect(all.map((row) => [row.need.item.name, row.needed])).toEqual([["Bandage", 5], ["Diamond", 1], ["Ruby", 1]]);

    const later = [need("Amber", 0, [{ state: "locked" }]), ...items];
    const ordered = checklistRows(later, { search: "", hideLocked: false, ownedFirst: false });
    expect(ordered.map((row) => row.need.item.name)).toEqual(["Bandage", "Diamond", "Amber", "Ruby"]);

    const open = checklistRows(items, { search: "", hideLocked: true, ownedFirst: false });
    expect(open.map((row) => [row.need.item.name, row.needed, row.hidden])).toEqual([["Bandage", 2, 1], ["Diamond", 1, 0]]);
  });

  it("finds items by item, merchant or quest name and can put owned items first", () => {
    const byMerchant = checklistRows(items, { search: "gold", hideLocked: false, ownedFirst: false });
    expect(byMerchant.map((row) => row.need.item.name)).toEqual(["Diamond"]);
    const byQuest = checklistRows(items, { search: "precision", hideLocked: false, ownedFirst: false });
    expect(byQuest.map((row) => row.need.item.name)).toEqual(["Diamond"]);

    const owned = checklistRows(items, { search: "", hideLocked: false, ownedFirst: true });
    expect(owned.map((row) => row.need.item.name)).toEqual(["Diamond", "Ruby", "Bandage"]);
  });
});

describe("secondsSince", () => {
  it("never goes negative", () => {
    expect(secondsSince(100, 160)).toBe(60);
    expect(secondsSince(200, 160)).toBe(0);
  });
});
