import { useState } from "react";
import { ChevronRight, Eye, Search, Square, Tag, TriangleAlert } from "lucide-react";

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
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { PlanEntry, PlanResponse } from "@/lib/lister-api";
import { planTotals, type PlanTotals } from "@/lib/lister";
import { formatGold } from "@/lib/format";
import { PlanTable } from "@/pages/lister/plan-table";

interface PlanPanelProps {
  response: PlanResponse;
  ticked: Set<string>;
  onTicked: (ticked: Set<string>) => void;
  onEntry: (entry: PlanEntry) => void;
  recheck: boolean;
  onRecheck: (on: boolean) => void;
  running: boolean;
  canRun: boolean;
  onPriceFromGame: () => void;
  onStart: (dryRun: boolean) => void;
  onStop: () => void;
}

/** The plan to review: what gets listed at which price, what was left out and why, and the run buttons. */
export function PlanPanel(props: PlanPanelProps) {
  const { response, ticked, onTicked, running } = props;
  const { plan } = response;
  const totals = planTotals(plan.entries, ticked);
  const unpriced = plan.entries.some((e) => e.price <= 0);
  const [confirming, setConfirming] = useState(false);

  const tick = (id: string, on: boolean) => {
    const next = new Set(ticked);
    if (on) next.add(id);
    else next.delete(id);
    onTicked(next);
  };

  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle className="font-display text-base">Plan</CardTitle>
        <CardDescription>
          {plan.entries.length === 0
            ? "Nothing to list with these rules."
            : `${plan.entries.length} item${plan.entries.length === 1 ? "" : "s"} to list, ${plan.skipped.length} skipped`}
        </CardDescription>
        {totals.count > 0 && !unpriced && (
          <CardAction>
            <TotalsSummary totals={totals} />
          </CardAction>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {plan.warnings.length > 0 && (
          <Alert className="border-gold/30 bg-gold/5">
            <TriangleAlert className="text-gold" />
            <AlertDescription>
              <ul className="flex flex-col gap-1">
                {plan.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            </AlertDescription>
          </Alert>
        )}
        {plan.entries.length > 0 && (
          <PlanTable
            entries={plan.entries}
            items={response.items}
            ticked={ticked}
            onTick={tick}
            onTickAll={(on) => onTicked(new Set(on ? plan.entries.map((e) => e.uniqueId) : []))}
            onEntry={props.onEntry}
            locked={running}
          />
        )}
        {plan.skipped.length > 0 && <SkippedList response={response} />}
      </CardContent>
      <CardFooter className="flex flex-wrap items-center gap-3 border-t">
        <Label className="mr-auto flex items-center gap-2 font-normal text-muted-foreground" title="Lowers a price if the market dropped, skips items no longer worth listing">
          <Switch checked={props.recheck} onCheckedChange={props.onRecheck} disabled={running} />
          Re-check each price right before listing
        </Label>
        {running ? (
          <Button variant="destructive" onClick={props.onStop}>
            <Square />
            Stop
          </Button>
        ) : (
          <>
            {unpriced && (
              <Button onClick={props.onPriceFromGame} disabled={!props.canRun || totals.count === 0}>
                <Search />
                Price from game
              </Button>
            )}
            <Button variant="outline" onClick={() => props.onStart(true)} disabled={!props.canRun || unpriced || totals.count === 0} title="Goes through every step without listing anything">
              <Eye />
              Dry run
            </Button>
            <Button onClick={() => setConfirming(true)} disabled={!props.canRun || unpriced || totals.count === 0}>
              <Tag />
              Start listing
            </Button>
          </>
        )}
      </CardFooter>
      <ConfirmListing
        open={confirming}
        totals={totals}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          props.onStart(false);
        }}
      />
    </Card>
  );
}

function TotalsSummary({ totals }: { totals: PlanTotals }) {
  return (
    <dl className="flex gap-5 text-right text-xs">
      <div>
        <dt className="text-muted-foreground">Asking</dt>
        <dd className="text-sm"><Gold value={totals.price} /></dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Fees now</dt>
        <dd className="font-num text-sm text-loss tabular">−{formatGold(totals.fee)}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">If all sell</dt>
        <dd className="text-sm"><Gold value={totals.net} /></dd>
      </div>
    </dl>
  );
}

function SkippedList({ response }: { response: PlanResponse }) {
  const { skipped } = response.plan;
  return (
    <Collapsible className="rounded-md border">
      <CollapsibleTrigger className="group flex w-full items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground">
        <ChevronRight className="size-4 transition-transform group-data-[state=open]:rotate-90" />
        Skipped ({skipped.length})
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ul className="divide-y divide-border/60 border-t text-sm">
          {skipped.map((skip) => (
            <li key={`${skip.uniqueId}-${skip.stashId}-${skip.slotId}`} className="flex items-center gap-3 px-3 py-1.5">
              <span className="w-56 truncate">{skip.name}</span>
              <span className="w-24 truncate text-xs text-muted-foreground">{response.items[skip.uniqueId]?.stashLabel ?? skip.stashId}</span>
              <span className="flex-1 text-muted-foreground">{skip.reason}</span>
              {skip.merchant && <span className="text-xs text-gold">merchant</span>}
            </li>
          ))}
        </ul>
      </CollapsibleContent>
    </Collapsible>
  );
}

interface ConfirmListingProps {
  open: boolean;
  totals: PlanTotals;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Listing costs fees the game never refunds, so a real run always asks first. */
function ConfirmListing({ open, totals, onCancel, onConfirm }: ConfirmListingProps) {
  const items = `${totals.count} item${totals.count === 1 ? "" : "s"}`;
  return (
    <AlertDialog open={open} onOpenChange={(next) => !next && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-display">List {items}?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="flex flex-col gap-2">
              <p>
                The game charges <span className="font-num text-loss">{formatGold(totals.fee)}</span> gold in listing fees now, and
                never refunds them. If everything sells you get <span className="font-num text-gold">{formatGold(totals.net)}</span> gold.
              </p>
              <p>The app moves the mouse and clicks in the game. Keep your hands off until it's done; Ctrl+F12 stops it.</p>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>List and pay {formatGold(totals.fee)} gold</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
