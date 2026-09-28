import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { sorterApi, type SortOptions } from "@/lib/sorter-api";
import { isRunning } from "@/lib/sorter";

/** How often the page asks what a run is doing while one is going, and otherwise. */
const RUNNING_POLL_MS = 400;
const IDLE_POLL_MS = 4_000;
/** Options are saved this long after the last change. */
const SAVE_DELAY_MS = 400;

export const STATUS_KEY = ["sorter", "status"] as const;
const OPTIONS_KEY = ["sorter", "options"] as const;

export function useSorterStatus() {
  return useQuery({
    queryKey: STATUS_KEY,
    queryFn: sorterApi.status,
    refetchInterval: (query) => (isRunning(query.state.data) ? RUNNING_POLL_MS : IDLE_POLL_MS),
  });
}

export function useSorterOptions() {
  return useQuery({ queryKey: OPTIONS_KEY, queryFn: sorterApi.options, staleTime: Infinity });
}

/** The stash now and sorted with `options`. Under the "stash" key, so new game data refreshes it. */
export function useSortPreview(characterId: string | undefined, stashId: number | null, options: SortOptions | null) {
  return useQuery({
    queryKey: ["stash", "sorter-preview", characterId, stashId, options],
    queryFn: () => sorterApi.preview(characterId!, stashId!, options!),
    enabled: !!characterId && stashId !== null && !!options,
    placeholderData: keepPreviousData,
  });
}

/** Local options, loaded once and saved a moment after each change. */
export function useEditableOptions() {
  const { data } = useSorterOptions();
  const client = useQueryClient();
  const [options, setOptions] = useState<SortOptions | null>(null);
  const edited = useRef(false);

  useEffect(() => {
    if (data && !options) setOptions(data.options);
  }, [data, options]);

  useEffect(() => {
    if (!options || !edited.current) return;
    const timer = window.setTimeout(() => {
      sorterApi
        .saveOptions(options)
        .then((saved) => client.setQueryData(OPTIONS_KEY, (view: typeof data) => (view ? { ...view, options: saved } : view)))
        .catch((error: unknown) => toast.error("Could not save the sorter options", { description: String(error) }));
    }, SAVE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [options, client]);

  const update = (next: SortOptions) => {
    edited.current = true;
    setOptions(next);
  };
  return { options, update, riskAccepted: data?.riskAccepted ?? false };
}

/** Starts a sort; errors show as a toast and the status refreshes either way. */
export function useStartSort() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ characterId, stashId, options, acceptRisk }: { characterId: string; stashId: number; options: SortOptions; acceptRisk: boolean }) =>
      sorterApi.start(characterId, stashId, options, acceptRisk),
    onError: (error) => toast.error("Could not start sorting", { description: String(error) }),
    onSettled: () => {
      client.invalidateQueries({ queryKey: STATUS_KEY });
      client.invalidateQueries({ queryKey: OPTIONS_KEY });
    },
  });
}

export function useStopSort() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: sorterApi.stop,
    onError: (error) => toast.error("Could not stop", { description: String(error) }),
    onSettled: () => client.invalidateQueries({ queryKey: STATUS_KEY }),
  });
}

export function useSortFeedback() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ sessionId, success }: { sessionId: string; success: boolean }) => sorterApi.feedback(sessionId, success, null),
    onError: (error) => toast.error("Could not send the feedback", { description: String(error) }),
    onSettled: () => client.invalidateQueries({ queryKey: STATUS_KEY }),
  });
}

/** Refreshes the preview once a run ends: its stash now waits for the game's fresh copy. */
export function useRefreshAfterRun(running: boolean) {
  const client = useQueryClient();
  const wasRunning = useRef(running);
  useEffect(() => {
    if (wasRunning.current && !running) client.invalidateQueries({ queryKey: ["stash"] });
    wasRunning.current = running;
  }, [running, client]);
}
