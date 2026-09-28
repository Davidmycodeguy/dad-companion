import { describe, expect, it } from "vitest";

import type { StashSummary } from "@/lib/api";
import {
  countLabel,
  defaultTab,
  directionLabel,
  isRunning,
  moveEntry,
  progressPercent,
  sortBlocker,
  stashTabs,
  toggleDirection,
} from "@/lib/sorter";
import type { OrderEntry, SortPreview, SorterStatus } from "@/lib/sorter-api";

const ORDER: OrderEntry[] = [
  { field: "rarity", direction: "desc" },
  { field: "name", direction: "asc" },
  { field: "width", direction: "desc" },
];

function status(overrides: Partial<SorterStatus> = {}): SorterStatus {
  return {
    state: "idle",
    characterId: null,
    stashId: null,
    stashLabel: null,
    phase: null,
    current: null,
    done: 0,
    total: 0,
    verified: 0,
    failed: 0,
    retried: 0,
    stoppedReason: null,
    sessionId: null,
    feedbackSent: false,
    hotkey: true,
    busyWith: null,
    finishedAt: null,
    ...overrides,
  };
}

function preview(overrides: Partial<SortPreview> = {}): SortPreview {
  return {
    stashLabel: "Stash",
    grid: [12, 20],
    bagGrid: [10, 5],
    current: [],
    bag: [],
    sorted: [],
    moves: 12,
    merges: 0,
    bagMoves: 0,
    moving: 8,
    incoming: 0,
    blocked: null,
    dataAgeS: 30,
    ...overrides,
  };
}

function tab(id: number, items: number, locked = false): StashSummary {
  return { id, label: `Tab ${id}`, items, locked };
}

describe("moveEntry", () => {
  it("moves an entry up or down and leaves the original alone", () => {
    const up = moveEntry(ORDER, 1, -1);
    expect(up.map((e) => e.field)).toEqual(["name", "rarity", "width"]);
    expect(ORDER.map((e) => e.field)).toEqual(["rarity", "name", "width"]);
    expect(moveEntry(ORDER, 0, 1).map((e) => e.field)).toEqual(["name", "rarity", "width"]);
  });

  it("does nothing past either end", () => {
    expect(moveEntry(ORDER, 0, -1)).toBe(ORDER);
    expect(moveEntry(ORDER, 2, 1)).toBe(ORDER);
    expect(moveEntry(ORDER, 7, -1)).toBe(ORDER);
  });
});

describe("toggleDirection", () => {
  it("flips one entry's direction in a copy", () => {
    const flipped = toggleDirection(ORDER, 0);
    expect(flipped[0]).toEqual({ field: "rarity", direction: "asc" });
    expect(flipped[1]).toBe(ORDER[1]);
    expect(ORDER[0].direction).toBe("desc");
  });
});

describe("directionLabel", () => {
  it("says what comes first in words", () => {
    expect(directionLabel({ field: "rarity", direction: "desc" })).toBe("Rarest first");
    expect(directionLabel({ field: "name", direction: "asc" })).toBe("A to Z");
    expect(directionLabel({ field: "height", direction: "desc" })).toBe("Tallest first");
  });
});

describe("stashTabs and defaultTab", () => {
  const stashes = [tab(2, 8), tab(3, 5), tab(4, 30), tab(5, 240), tab(21, 71), tab(30, 69, true), tab(101, 4)];

  it("offers only stash tabs, the locked one included", () => {
    expect(stashTabs(stashes).map((s) => s.id)).toEqual([4, 5, 21, 30]);
  });

  it("starts on the fullest tab that can be sorted, never the locked one", () => {
    expect(defaultTab(stashes)).toBe(5);
    expect(defaultTab([tab(30, 999, true), tab(4, 1)])).toBe(4);
    expect(defaultTab([tab(2, 3), tab(30, 9, true)])).toBeNull();
  });
});

describe("progressPercent", () => {
  it("is the share of moves done", () => {
    expect(progressPercent(status({ state: "running", done: 3, total: 12 }))).toBe(25);
  });

  it("is unknown while a run gets ready and full once one ends with nothing to do", () => {
    expect(progressPercent(status({ state: "running" }))).toBeUndefined();
    expect(progressPercent(status({ state: "done" }))).toBe(100);
  });
});

describe("sortBlocker", () => {
  it("lets a planned sort start", () => {
    expect(sortBlocker(preview(), status())).toBeNull();
  });

  it("names what stands in the way, most pressing first", () => {
    expect(sortBlocker(preview(), status({ state: "running" }))).toBe("A sort is running.");
    expect(sortBlocker(preview(), status({ busyWith: "The auto lister" }))).toBe("The auto lister is running.");
    expect(sortBlocker(undefined, status())).toBe("Getting the stash ready.");
    expect(sortBlocker(undefined, status(), true)).toBe("The stash couldn't be read.");
    expect(sortBlocker(preview({ blocked: "Reopen your character." }), status())).toBe("Reopen your character.");
    expect(sortBlocker(preview({ moves: 0 }), status())).toBe("Already sorted.");
  });
});

describe("isRunning and countLabel", () => {
  it("reads the run state and counts in words", () => {
    expect(isRunning(status({ state: "running" }))).toBe(true);
    expect(isRunning(status({ state: "stopped" }))).toBe(false);
    expect(isRunning(undefined)).toBe(false);
    expect(countLabel(1, "move")).toBe("1 move");
    expect(countLabel(1250, "move")).toBe("1,250 moves");
  });
});
