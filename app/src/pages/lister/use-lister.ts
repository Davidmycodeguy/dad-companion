import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { listerApi, type ListerRules, type ListerStatus } from "@/lib/lister-api";

/** How often the page asks what a run is doing while one is going, and otherwise. */
const RUNNING_POLL_MS = 500;
const IDLE_POLL_MS = 5_000;
/** Rules are saved this long after the last change, so typing a price saves once. */
const SAVE_RULES_DELAY_MS = 500;

export const STATUS_KEY = ["lister", "status"] as const;

export function useListerRules() {
  return useQuery({ queryKey: ["lister", "rules"], queryFn: listerApi.rules, staleTime: Infinity });
}

export function useListerSources(characterId: string | undefined) {
  return useQuery({
    queryKey: ["lister", "sources", characterId],
    queryFn: () => listerApi.sources(characterId!),
    enabled: !!characterId,
  });
}

export function useListerStatus() {
  return useQuery({
    queryKey: STATUS_KEY,
    queryFn: listerApi.status,
    refetchInterval: (query) => (query.state.data?.state === "running" ? RUNNING_POLL_MS : IDLE_POLL_MS),
  });
}

export function isRunning(status: ListerStatus | undefined): boolean {
  return status?.state === "running";
}

/** Saves the rules a moment after the last change; the server's cleaned-up copy comes back. */
export function useAutosaveRules(rules: ListerRules | null) {
  const client = useQueryClient();
  const first = useRef(true);
  useEffect(() => {
    if (!rules) return;
    if (first.current) {
      first.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      listerApi
        .saveRules(rules)
        .then((saved) => client.setQueryData(["lister", "rules"], saved))
        .catch((error: unknown) => toast.error("Could not save the rules", { description: String(error) }));
    }, SAVE_RULES_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [rules, client]);
}

/** Starts a lister run; errors show as a toast, and the status is refreshed either way. */
export function useListerAction<T>(action: (input: T) => Promise<unknown>, failure: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: action,
    onError: (error) => toast.error(failure, { description: String(error) }),
    onSettled: () => client.invalidateQueries({ queryKey: STATUS_KEY }),
  });
}
