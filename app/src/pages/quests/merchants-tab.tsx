import { useState } from "react";
import { ChevronRight, CircleCheck, Store } from "lucide-react";

import { ItemSlot } from "@/components/item-slot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Progress } from "@/components/ui/progress";
import { stateLabel } from "@/lib/quests";
import type { CurrentQuest, MerchantSummary, NextStep, QuestsOverview } from "@/lib/quests-api";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";
import { LiveMark, MerchantPortrait } from "@/pages/quests/parts";

/** Next steps shown before "Show all". */
const STEPS_SHOWN = 5;

export function MerchantsTab({ overview, now, onOpen }: { overview: QuestsOverview; now: number; onOpen: (merchant: string) => void }) {
  if (overview.merchants.length === 0) {
    return (
      <Empty className="border border-dashed py-12">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Store />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">No merchants to show</EmptyTitle>
          <EmptyDescription>Merchants appear here once the game lists them.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }
  return (
    <div className="flex flex-col gap-6">
      {overview.nextSteps.length > 0 && <NextSteps steps={overview.nextSteps} onOpen={onOpen} />}
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {overview.merchants.map((merchant) => (
          <MerchantCard key={merchant.name} merchant={merchant} now={now} onOpen={() => onOpen(merchant.name)} />
        ))}
      </div>
    </div>
  );
}

function NextSteps({ steps, onOpen }: { steps: NextStep[]; onOpen: (merchant: string) => void }) {
  const [all, setAll] = useState(false);
  const shown = all ? steps : steps.slice(0, STEPS_SHOWN);
  return (
    <section className="flex flex-col gap-2">
      <h2 className="font-display text-base">Next steps</h2>
      <ul className="divide-y divide-border/60 overflow-hidden rounded-lg border bg-card">
        {shown.map((step) => (
          <li key={`${step.action}:${step.questId}`}>
            <button
              type="button"
              onClick={() => onOpen(step.merchant)}
              className="group flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-accent/40"
            >
              <MerchantPortrait name={step.merchant} size={32} />
              <span className="w-32 shrink-0 truncate font-display text-[15px]">{step.merchant}</span>
              <StepText step={step} />
              <ChevronRight className="size-4 shrink-0 text-muted-foreground/50 group-hover:text-primary" />
            </button>
          </li>
        ))}
      </ul>
      {steps.length > STEPS_SHOWN && (
        <Button variant="ghost" size="sm" className="w-fit text-muted-foreground" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${steps.length}`}
        </Button>
      )}
    </section>
  );
}

function StepText({ step }: { step: NextStep }) {
  if (step.action === "turnIn") {
    return (
      <span className="flex min-w-0 flex-1 items-center gap-2 text-sm">
        <span className="shrink-0 text-gold">Turn in</span>
        <span className="truncate">{step.questTitle}</span>
      </span>
    );
  }
  return (
    <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1 text-sm">
      <span className="shrink-0 text-profit">Bring</span>
      {step.items.map((entry) => (
        <span key={entry.item.id ?? entry.item.name} className="flex items-center gap-1.5">
          <ItemSlot icon={entry.item.icon} rarity={entry.item.rarity} size={22} />
          <span className="font-num tabular">{entry.count}</span>
          <span className={RARITY_TEXT[entry.item.rarity]}>{entry.item.name}</span>
          {entry.count < entry.needed && <span className="text-xs text-muted-foreground">of {entry.needed}</span>}
        </span>
      ))}
      <span className="truncate text-xs text-muted-foreground">{step.questTitle}</span>
    </span>
  );
}

function MerchantCard({ merchant, now, onOpen }: { merchant: MerchantSummary; now: number; onOpen: () => void }) {
  const allDone = merchant.total > 0 && merchant.done === merchant.total;
  const percent = merchant.total > 0 ? (merchant.done / merchant.total) * 100 : 0;
  return (
    <button
      type="button"
      onClick={onOpen}
      className={cn(
        "group flex flex-col gap-3 rounded-lg border bg-card p-3 text-left transition-colors hover:border-primary/40 hover:bg-accent/30",
        allDone && "border-profit/30",
      )}
    >
      <div className="flex items-start gap-3">
        <MerchantPortrait name={merchant.name} size={60} />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex items-center gap-2">
            <span className="truncate font-display text-base">{merchant.name}</span>
            {merchant.tracked && <LiveMark seenAt={merchant.seenAt} now={now} className="ml-auto" />}
          </div>
          <span className="text-xs text-muted-foreground">
            {allDone ? (
              <span className="inline-flex items-center gap-1 text-profit">
                <CircleCheck className="size-3.5" />
                All {merchant.total} done
              </span>
            ) : (
              <>
                <span className="font-num text-foreground tabular">{merchant.done}</span> of{" "}
                <span className="font-num tabular">{merchant.total}</span> done
              </>
            )}
          </span>
          <Progress value={percent} className={cn("h-1", allDone && "[&>div]:bg-profit")} />
        </div>
      </div>
      {!allDone && <CardFooter merchant={merchant} />}
    </button>
  );
}

/** What the current quest waits on, in a few words. */
function currentNote(current: CurrentQuest): string {
  if (current.state === "locked") return current.waitsFor ? `Unlocks after ${current.waitsFor}` : "Locked";
  if (current.state === "available") return "Up next";
  return stateLabel(current.state, true) ?? "";
}

function CardFooter({ merchant }: { merchant: MerchantSummary }) {
  const current = merchant.current;
  return (
    <div className="flex flex-col gap-2 border-t border-border/60 pt-2.5 text-sm">
      {current && (
        <div className="flex min-w-0 items-baseline gap-2">
          <span className="truncate">{current.title}</span>
          <span
            className={cn(
              "ml-auto max-w-[55%] shrink-0 truncate text-xs",
              current.state === "ready" ? "text-gold" : "text-muted-foreground",
            )}
          >
            {currentNote(current)}
          </span>
        </div>
      )}
      {(merchant.turnIn || merchant.bring > 0 || merchant.active > 1) && (
        <div className="flex flex-wrap gap-1.5">
          {merchant.turnIn && <Badge className="border-gold/40 bg-gold/10 font-normal text-gold" variant="outline">Turn in</Badge>}
          {merchant.bring > 0 && (
            <Badge variant="outline" className="border-profit/30 font-normal text-profit">
              {merchant.bring} to bring
            </Badge>
          )}
          {merchant.active > 1 && (
            <Badge variant="outline" className="font-normal text-muted-foreground">
              {merchant.active} in progress
            </Badge>
          )}
        </div>
      )}
    </div>
  );
}
