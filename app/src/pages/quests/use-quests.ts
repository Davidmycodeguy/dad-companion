import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

import { questsApi } from "@/lib/quests-api";

export const QUESTS_KEY = ["quests"] as const;

export function useQuestsOverview() {
  return useQuery({ queryKey: [...QUESTS_KEY, "overview"], queryFn: questsApi.overview });
}

export function useMerchantQuests(merchant: string | null) {
  return useQuery({
    queryKey: [...QUESTS_KEY, "merchant", merchant],
    queryFn: () => questsApi.merchant(merchant ?? ""),
    enabled: merchant !== null,
  });
}

export function useQuestItems() {
  return useQuery({ queryKey: [...QUESTS_KEY, "items"], queryFn: questsApi.items });
}

/** Refreshes the page when the game reports quest changes, or a character (what the player owns). */
export function useQuestLiveRefresh() {
  const client = useQueryClient();
  useEffect(() => {
    const stop = listen<string>("game-data", (event) => {
      if (event.payload === "quests" || event.payload === "characters") {
        void client.invalidateQueries({ queryKey: QUESTS_KEY });
      }
    });
    return () => {
      stop.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [client]);
}

interface ObjectiveChange {
  questId: string;
  index: number;
  /** null: all when done, else what was recorded. */
  submitted: number | null;
  done: boolean;
}

/** Saves the player's own progress on an objective; the page refreshes either way. */
export function useSetObjective() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ questId, index, submitted, done }: ObjectiveChange) => questsApi.setObjective(questId, index, submitted, done),
    onError: (error) => toast.error("Could not save the objective", { description: String(error) }),
    onSettled: () => client.invalidateQueries({ queryKey: QUESTS_KEY }),
  });
}

/** Ticks a whole quest done, or clears the player's own progress on it. */
export function useSetQuestDone() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ questId, done }: { questId: string; done: boolean }) => questsApi.setDone(questId, done),
    onError: (error) => toast.error("Could not save the quest", { description: String(error) }),
    onSettled: () => client.invalidateQueries({ queryKey: QUESTS_KEY }),
  });
}

/** A page setting remembered between visits; a stored value of the wrong shape is ignored. */
export function useStoredState<T>(key: string, initial: T, valid: (value: unknown) => value is T): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const stored: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
      return valid(stored) ? stored : initial;
    } catch {
      return initial;
    }
  });
  const store = (next: T) => {
    setValue(next);
    try {
      localStorage.setItem(key, JSON.stringify(next));
    } catch {
      // Storage can be unavailable; the setting then only lasts until the app closes.
    }
  };
  return [value, store];
}

export const isBoolean = (value: unknown): value is boolean => typeof value === "boolean";
export const isString = (value: unknown): value is string => typeof value === "string";

/** The current time in Unix seconds, updated every `everyMs` so ages stay current. */
export function useNowSeconds(everyMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}
