import type { StashSummary } from "@/lib/api";
import type { OrderEntry, SortDirection, SortField, SortPreview, SorterStatus } from "@/lib/sorter-api";

/** The first stash tab id; lower ids are the bag and equipment. */
const FIRST_STASH_TAB = 4;
/** Stash tabs have ids up to here (the locked seasonal stash is 30); gear sets start at 100. */
const LAST_STASH_TAB = 30;

export const FIELD_LABEL: Record<SortField, string> = {
  width: "Width",
  height: "Height",
  slot: "Slot",
  rarity: "Rarity",
  name: "Name",
};

const DIRECTION_LABEL: Record<SortField, Record<SortDirection, string>> = {
  width: { desc: "Widest first", asc: "Narrowest first" },
  height: { desc: "Tallest first", asc: "Shortest first" },
  slot: { desc: "Z to A", asc: "A to Z" },
  rarity: { desc: "Rarest first", asc: "Commonest first" },
  name: { desc: "Z to A", asc: "A to Z" },
};

export function directionLabel(entry: OrderEntry): string {
  return DIRECTION_LABEL[entry.field][entry.direction];
}

/** A copy of `order` with the entry at `index` moved by `delta` places (clamped to the list). */
export function moveEntry(order: OrderEntry[], index: number, delta: number): OrderEntry[] {
  const target = Math.min(order.length - 1, Math.max(0, index + delta));
  if (index < 0 || index >= order.length || target === index) return order;
  const next = [...order];
  const [entry] = next.splice(index, 1);
  next.splice(target, 0, entry);
  return next;
}

/** A copy of `order` with the entry at `index` sorted the other way round. */
export function toggleDirection(order: OrderEntry[], index: number): OrderEntry[] {
  return order.map((entry, i) => (i === index ? { ...entry, direction: entry.direction === "asc" ? "desc" : "asc" } : entry));
}

/** The character's stash tabs the page offers, the locked seasonal stash included (shown locked). */
export function stashTabs(stashes: StashSummary[]): StashSummary[] {
  return stashes.filter((s) => s.id >= FIRST_STASH_TAB && s.id <= LAST_STASH_TAB);
}

/** The tab to show first: the fullest tab that can be sorted. */
export function defaultTab(stashes: StashSummary[]): number | null {
  const open = stashTabs(stashes).filter((s) => !s.locked);
  if (open.length === 0) return null;
  return open.reduce((best, s) => (s.items > best.items ? s : best)).id;
}

export function isRunning(status: SorterStatus | undefined): boolean {
  return status?.state === "running";
}

/** Share of moves done, 0-100; undefined while the run is still getting ready. */
export function progressPercent(status: SorterStatus): number | undefined {
  if (status.total <= 0) return status.state === "running" ? undefined : 100;
  return Math.min(100, (status.done / status.total) * 100);
}

/** Why the Sort button can't start a run right now, or null when it can. `failed`: the preview couldn't be built. */
export function sortBlocker(preview: SortPreview | undefined, status: SorterStatus | undefined, failed = false): string | null {
  if (isRunning(status)) return "A sort is running.";
  if (status?.busyWith) return `${status.busyWith} is running.`;
  if (!preview) return failed ? "The stash couldn't be read." : "Getting the stash ready.";
  if (preview.blocked) return preview.blocked;
  if (preview.moves === 0) return "Already sorted.";
  return null;
}

/** "12 moves", "1 move". */
export function countLabel(count: number, noun: string): string {
  return `${count.toLocaleString()} ${noun}${count === 1 ? "" : "s"}`;
}
