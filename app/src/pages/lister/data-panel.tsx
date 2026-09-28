import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BrainCircuit, MousePointer2, RefreshCw, ScanSearch } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { listerApi } from "@/lib/lister-api";
import { formatAge } from "@/lib/format";

interface DataPanelProps {
  running: boolean;
  canRun: boolean;
  onCrawl: (deep: boolean) => void;
  onHoverTest: () => void;
}

/** The saved market data prices come from, and the value model trained on it. */
export function DataPanel({ running, canRun, onCrawl, onHoverTest }: DataPanelProps) {
  const client = useQueryClient();
  const { data: market } = useQuery({ queryKey: ["market", "summary"], queryFn: listerApi.marketData });
  const { data: worth } = useQuery({ queryKey: ["worth", "info"], queryFn: listerApi.worthInfo });
  const train = useMutation({
    mutationFn: listerApi.trainWorth,
    onSuccess: (info) => {
      client.setQueryData(["worth", "info"], info);
      client.invalidateQueries({ queryKey: ["stash"] });
      toast.success("Value model trained", { description: `Learned from ${info.listings.toLocaleString()} listings.` });
    },
    onError: (error) => toast.error("Training failed", { description: String(error) }),
  });

  return (
    <div className="grid gap-4 md:grid-cols-2">
      <Card className="gap-3">
        <CardHeader>
          <CardTitle className="font-display text-base">Market data</CardTitle>
          <CardDescription>Every Marketplace page the game shows is saved on this PC, including pages you browse yourself.</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-3 gap-3">
          <Stat label="Listings">{market?.listings.toLocaleString() ?? "…"}</Stat>
          <Stat label="Items">{market?.items.toLocaleString() ?? "…"}</Stat>
          <Stat label="Likely sold">{market?.vanished.toLocaleString() ?? "…"}</Stat>
        </CardContent>
        <CardFooter className="gap-2 border-t">
          <Button variant="outline" size="sm" disabled={!canRun || running} onClick={() => onCrawl(false)} title="Reads only listings that are new since the last update">
            <RefreshCw />
            Update
          </Button>
          <Button variant="outline" size="sm" disabled={!canRun || running} onClick={() => onCrawl(true)} title="Reads many pages of gear for every rarity: takes a while">
            <ScanSearch />
            Deep crawl
          </Button>
        </CardFooter>
      </Card>
      <Card className="gap-3">
        <CardHeader>
          <CardTitle className="font-display text-base">Value model</CardTitle>
          <CardDescription>Learns what each item is worth for its exact rolls from the saved market data.</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-3 gap-3">
          <Stat label="Learned from">{worth ? (worth.trained ? worth.listings.toLocaleString() : "—") : "…"}</Stat>
          <Stat label="Typical error">{worth?.mdape != null ? `${Math.round(worth.mdape * 100)}%` : "—"}</Stat>
          <Stat label="Trained">{worth?.trainedAt ? formatAge(Date.now() / 1000 - worth.trainedAt) : "—"}</Stat>
        </CardContent>
        <CardFooter className="border-t">
          <Button variant="outline" size="sm" disabled={train.isPending} onClick={() => train.mutate()}>
            {train.isPending ? <Spinner /> : <BrainCircuit />}
            Train now
          </Button>
        </CardFooter>
      </Card>
      <Card className="gap-3 md:col-span-2">
        <CardHeader>
          <CardTitle className="font-display text-base">Calibration</CardTitle>
          <CardDescription>
            With Trade → Marketplace open in the game, the hover test rests the mouse on each spot the lister clicks, one
            second each, without clicking. If a spot is off, don't run the lister at this resolution.
          </CardDescription>
        </CardHeader>
        <CardFooter className="border-t">
          <Button variant="outline" size="sm" disabled={!canRun || running} onClick={onHoverTest}>
            <MousePointer2 />
            Hover test
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-num text-lg tabular">{children}</span>
    </div>
  );
}
