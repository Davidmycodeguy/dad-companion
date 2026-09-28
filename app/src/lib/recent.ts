import { useSyncExternalStore } from "react";

const KEY = "recentItems";
const MAX_RECENT = 10;
const listeners = new Set<() => void>();

function read(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((name): name is string => typeof name === "string") : [];
  } catch {
    return [];
  }
}

let snapshot = read();

function write(names: string[]) {
  snapshot = names;
  try {
    localStorage.setItem(KEY, JSON.stringify(names));
  } catch {
    // Storage can be unavailable; the list then only lasts until the app closes.
  }
  listeners.forEach((listener) => listener());
}

/** Puts `name` first in the recently viewed items. */
export function rememberRecent(name: string) {
  if (snapshot[0] === name) return;
  write([name, ...snapshot.filter((other) => other !== name)].slice(0, MAX_RECENT));
}

/** Drops a name that no longer matches an item (renamed in a game update). */
export function forgetBrokenRecent(name: string) {
  if (snapshot.includes(name)) write(snapshot.filter((other) => other !== name));
}

export function useRecentItems(): string[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
  );
}
