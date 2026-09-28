import { CircleCheck, CircleSlash, CircleX, Eye, LoaderCircle } from "lucide-react";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import type { ItemResult, ListerStatus, RunMode } from "@/lib/lister-api";
import { cn } from "@/lib/utils";

const MODE_TITLE: Record<RunMode, string> = {
  list: "Listing",
  dryRun: "Dry run",
  price: "Pricing from the game",
  crawl: "Reading the market",
  collect: "Collecting sales",
  merchant: "Selling to the merchant",
  merchantDryRun: "Merchant dry run",
  hover: "Hover test",
};

/** What the current (or last) run is doing, item by item. */
export function RunPanel({ status }: { status: ListerStatus }) {
  if (!status.mode || (status.state === "idle" && status.results.length === 0)) return null;
  const running = status.state === "running";
  const done = status.results.length;
  const percent = status.total > 0 ? Math.min(100, (done / status.total) * 100) : running ? undefined : 100;
  return (
    <Card className="gap-3">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-display text-base">
          {running && <LoaderCircle className="size-4 animate-spin text-primary" />}
          {MODE_TITLE[status.mode]}
          {!running && <span className="text-sm font-normal text-muted-foreground">· finished</span>}
        </CardTitle>
        <CardDescription>
          {status.stoppedReason ?? (status.total > 0 ? `${done} of ${status.total}` : `${done} done`)}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {percent !== undefined && <Progress value={percent} className="h-1.5" />}
        {status.results.length > 0 && (
          <ol className="flex max-h-72 flex-col divide-y divide-border/60 overflow-y-auto rounded-md border text-sm">
            {status.results.map((result, index) => (
              <ResultRow key={`${result.uniqueId}-${index}`} result={result} />
            ))}
          </ol>
        )}
      </CardContent>
    </Card>
  );
}

function ResultRow({ result }: { result: ItemResult }) {
  const tone = resultTone(result.status);
  const Icon = { good: CircleCheck, bad: CircleX, skip: CircleSlash, dry: Eye }[tone];
  return (
    <li className="flex items-start gap-2.5 px-3 py-1.5">
      <Icon
        className={cn(
          "mt-0.5 size-4 shrink-0",
          tone === "good" && "text-profit",
          tone === "bad" && "text-loss",
          (tone === "skip" || tone === "dry") && "text-muted-foreground",
        )}
      />
      <span className="w-52 shrink-0 truncate">{result.name}</span>
      <span className="w-20 shrink-0 text-muted-foreground">{result.status}</span>
      <span className="flex-1 text-muted-foreground">{result.message}</span>
    </li>
  );
}

type Tone = "good" | "bad" | "skip" | "dry";

/** How a result reads at a glance: done, failed, skipped, or only rehearsed. */
export function resultTone(status: string): Tone {
  const s = status.toLowerCase();
  if (s.includes("dry")) return "dry";
  if (s.includes("fail") || s.includes("error")) return "bad";
  if (s.includes("skip") || s.includes("cancel") || s.includes("stop")) return "skip";
  return "good";
}
