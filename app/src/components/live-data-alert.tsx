import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery } from "@tanstack/react-query";
import { Download, RadioTower } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";

const NPCAP_DOWNLOAD = "https://npcap.com/#download";

export function useLiveStatus() {
  return useQuery({ queryKey: ["live"], queryFn: api.liveStatus });
}

/** Explains why live game data is off, and offers Npcap when that's what is missing. */
export function LiveDataAlert() {
  const { data: live } = useLiveStatus();
  if (!live || live.state !== "off") return null;
  return (
    <Alert className="border-gold/30 bg-gold/5">
      <RadioTower className="text-gold" />
      <AlertTitle>Live game data is off</AlertTitle>
      <AlertDescription className="flex flex-col gap-3">
        <p>
          {live.needsNpcap
            ? "The app reads the game's traffic through Npcap, a free packet capture driver. Install it, then restart the app: stashes, listings and quests then update as you play."
            : live.error}
        </p>
        {live.needsNpcap && (
          <Button size="sm" variant="outline" className="w-fit" onClick={() => void openUrl(NPCAP_DOWNLOAD)}>
            <Download />
            Get Npcap
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}
