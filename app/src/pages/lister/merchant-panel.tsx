import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Eye, HandCoins, TriangleAlert } from "lucide-react";

import { Gold } from "@/components/gold";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { listerApi, type PlanResponse } from "@/lib/lister-api";
import { formatGold } from "@/lib/format";

interface MerchantPanelProps {
  characterId: string;
  response: PlanResponse;
  running: boolean;
  canRun: boolean;
  onSell: (uniqueIds: string[], dryRun: boolean) => void;
}

/** Items a merchant pays more for than the market would (or too cheap to list), sold in one go. */
export function MerchantPanel({ characterId, response, running, canRun, onSell }: MerchantPanelProps) {
  const candidates = useMemo(() => response.plan.skipped.filter((s) => s.merchant && s.uniqueId), [response]);
  const [unticked, setUnticked] = useState<Set<string>>(new Set());
  const ids = candidates.map((c) => c.uniqueId).filter((id) => !unticked.has(id));
  const [confirming, setConfirming] = useState(false);

  const { data: quote } = useQuery({
    queryKey: ["lister", "merchant", characterId, candidates.map((c) => c.uniqueId)],
    queryFn: () => listerApi.merchantPlan(characterId, candidates.map((c) => c.uniqueId)),
    enabled: candidates.length > 0,
  });
  if (candidates.length === 0) return null;

  const valueOf = new Map(quote?.entries.map((e) => [e.uniqueId, e.value]));
  const refused = new Map(quote?.refused.map((r) => [r.uniqueId, r.reason]));
  const sellable = ids.filter((id) => valueOf.has(id));
  const total = sellable.reduce((sum, id) => sum + (valueOf.get(id) ?? 0), 0);
  const merchant = quote?.merchant ?? "the merchant";

  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle className="font-display text-base">Sell to merchant</CardTitle>
        <CardDescription>
          These pay more at {merchant} than on the market, or are too cheap to list. Each sale is checked against the game's reply.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {quote && quote.warnings.length > 0 && (
          <Alert className="border-gold/30 bg-gold/5">
            <TriangleAlert className="text-gold" />
            <AlertDescription>{quote.warnings.join(" ")}</AlertDescription>
          </Alert>
        )}
        <ul className="flex flex-col divide-y divide-border/60 rounded-md border text-sm">
          {candidates.map((item) => {
            const reason = refused.get(item.uniqueId);
            return (
              <li key={item.uniqueId} className="flex items-center gap-3 px-3 py-1.5">
                <Checkbox
                  checked={!unticked.has(item.uniqueId) && !reason}
                  disabled={!!reason || running}
                  onCheckedChange={(on) => {
                    const next = new Set(unticked);
                    if (on === true) next.delete(item.uniqueId);
                    else next.add(item.uniqueId);
                    setUnticked(next);
                  }}
                  aria-label={`Sell ${item.name}`}
                />
                <span className="flex-1 truncate">{item.name}</span>
                <span className="text-xs text-muted-foreground">{reason ?? item.reason}</span>
                <span className="w-20 text-right">{valueOf.has(item.uniqueId) ? <Gold value={valueOf.get(item.uniqueId)!} /> : "—"}</span>
              </li>
            );
          })}
        </ul>
      </CardContent>
      <CardFooter className="flex items-center gap-3 border-t">
        <span className="mr-auto text-sm text-muted-foreground">
          {sellable.length} item{sellable.length === 1 ? "" : "s"} for <Gold value={total} />
        </span>
        <Button variant="outline" disabled={!canRun || running || sellable.length === 0} onClick={() => onSell(sellable, true)} title="Puts the items in the sell box and takes them back out: nothing is sold">
          <Eye />
          Dry run
        </Button>
        <Button disabled={!canRun || running || sellable.length === 0} onClick={() => setConfirming(true)}>
          <HandCoins />
          Sell to merchant
        </Button>
      </CardFooter>
      <AlertDialog open={confirming} onOpenChange={setConfirming}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="font-display">Sell {sellable.length} items to {merchant}?</AlertDialogTitle>
            <AlertDialogDescription>
              You get {formatGold(total)} gold. Sold items are gone for good. The app moves the mouse and clicks in the game; Ctrl+F12 stops it.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={() => onSell(sellable, false)}>Sell for {formatGold(total)} gold</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
