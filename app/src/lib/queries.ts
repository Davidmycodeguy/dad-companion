import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { api, marketApi, type Settings } from "@/lib/api";

const STATUS_POLL_MS = 5_000;

export function useAppStatus() {
  return useQuery({ queryKey: ["status"], queryFn: api.status, refetchInterval: STATUS_POLL_MS });
}

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: api.settings });
}

/** Saves one setting, showing the new value at once and rolling back if saving fails. */
export function useSetSetting() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ key, value }: { key: string; value: unknown }) => api.setSetting(key, value),
    onMutate: async ({ key, value }) => {
      await client.cancelQueries({ queryKey: ["settings"] });
      const previous = client.getQueryData<Settings>(["settings"]);
      client.setQueryData<Settings>(["settings"], { ...previous, [key]: value });
      return { previous };
    },
    onError: (_error, _vars, context) => client.setQueryData(["settings"], context?.previous),
    onSettled: () => client.invalidateQueries({ queryKey: ["settings"] }),
  });
}

export function useItemSearch(query: string, limit = 30) {
  const trimmed = query.trim();
  return useQuery({
    queryKey: ["items", "search", trimmed, limit],
    queryFn: () => api.searchItems(trimmed, limit),
    enabled: trimmed.length > 0,
    placeholderData: keepPreviousData,
    staleTime: Infinity,
  });
}

const MARKET_STALE_MS = 60_000;

export function useItemMarket(itemId: string) {
  return useQuery({ queryKey: ["market", itemId], queryFn: () => marketApi.item(itemId), staleTime: MARKET_STALE_MS });
}

export function useOpenListingCounts(itemIds: string[]) {
  return useQuery({
    queryKey: ["market", "counts", itemIds],
    queryFn: () => marketApi.counts(itemIds),
    enabled: itemIds.length > 0,
    staleTime: MARKET_STALE_MS,
  });
}
