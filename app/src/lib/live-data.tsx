import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

/** A crawl records many marketplace pages a second; the pages refresh at most this often. */
const MARKET_REFRESH_MS = 3_000;

/** Refreshes what the game just changed: stashes when a character arrives, prices when pages do. */
export function LiveDataRefresher() {
  const client = useQueryClient();

  useEffect(() => {
    let marketDue = 0;
    const stop = listen<string>("game-data", (event) => {
      if (event.payload === "characters") {
        client.invalidateQueries({ queryKey: ["characters"] });
        client.invalidateQueries({ queryKey: ["stash"] });
      } else if (event.payload === "sold") {
        client.invalidateQueries({ queryKey: ["lister", "status"] });
        toast.success("An item sold on the Marketplace", {
          id: "market-item-sold",
          description: "Collect the gold from the Auto lister page or in game: uncollected gold is destroyed after 7 days.",
          duration: 10_000,
        });
      } else if (event.payload === "live") {
        client.invalidateQueries({ queryKey: ["live"] });
      } else if (event.payload === "listings") {
        client.invalidateQueries({ queryKey: ["lister", "status"] });
      } else if (event.payload === "market" && Date.now() >= marketDue) {
        marketDue = Date.now() + MARKET_REFRESH_MS;
        client.invalidateQueries({ queryKey: ["market"] });
      }
    });
    return () => {
      stop.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [client]);

  return null;
}
