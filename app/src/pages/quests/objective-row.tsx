import { Minus, Plus } from "lucide-react";

import { ItemSlot } from "@/components/item-slot";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { hasEnough, objectiveHint } from "@/lib/quests";
import type { ObjectiveView, Owned } from "@/lib/quests-api";
import { cn } from "@/lib/utils";
import { ObjectiveIcon } from "@/pages/quests/parts";
import { useSetObjective } from "@/pages/quests/use-quests";

interface ObjectiveRowProps {
  questId: string;
  objective: ObjectiveView;
  /** The game (or a later quest of the chain) says the quest is finished: nothing to edit. */
  settled: boolean;
}

export function ObjectiveRow({ questId, objective, settled }: ObjectiveRowProps) {
  const setObjective = useSetObjective();
  const fromGame = objective.done && objective.source !== "manual";
  const editable = !settled && !fromGame;
  const counted = objective.count > 1;
  const hint = objectiveHint(objective);
  const save = (submitted: number | null, done: boolean) =>
    setObjective.mutate({ questId, index: objective.index, submitted, done });

  return (
    <li className="group flex items-center gap-3 py-2">
      {objective.item ? (
        <ItemSlot icon={objective.item.icon} rarity={objective.item.rarity} size={36} className={cn(objective.done && "opacity-60")} />
      ) : (
        <ObjectiveIcon kind={objective.kind} done={objective.done} />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className={cn("truncate text-sm", objective.done && "text-muted-foreground")}>{objective.label}</span>
        {!objective.done && (hint || objective.owned) && (
          <span className="flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
            {hint && <span>{hint}</span>}
            {objective.owned && <OwnedNote owned={objective.owned} enough={hasEnough(objective)} />}
          </span>
        )}
        {counted && !objective.done && objective.submitted > 0 && (
          <Progress value={(objective.submitted / objective.count) * 100} className="h-0.5 max-w-56 [&>div]:bg-roll" />
        )}
      </div>
      {counted && editable && (
        <div className="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="One less"
            disabled={objective.source !== "manual" || objective.submitted === 0}
            onClick={() => save(objective.submitted - 1, false)}
          >
            <Minus />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="One more"
            disabled={objective.submitted >= objective.count}
            onClick={() => save(objective.submitted + 1, objective.submitted + 1 >= objective.count)}
          >
            <Plus />
          </Button>
        </div>
      )}
      <span className={cn("w-11 shrink-0 text-right font-num text-sm tabular", objective.done ? "text-profit" : "text-foreground")}>
        {objective.submitted}/{objective.count}
      </span>
      <Checkbox
        checked={objective.done}
        disabled={!editable}
        onCheckedChange={(checked) => save(null, checked === true)}
        aria-label={objective.done ? `${objective.label}: done` : `Mark ${objective.label} done`}
        title={fromGame ? "From the game" : undefined}
      />
    </li>
  );
}

/** What the player owns of an objective's item, with where it is on hover. */
function OwnedNote({ owned, enough }: { owned: Owned; enough: boolean }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="cursor-default">
          <span className={cn(enough && "text-profit")}>
            Have <span className="font-num tabular">{owned.usable}</span>
          </span>
          {owned.other > 0 && (
            <span className="text-muted-foreground/80">
              {" "}
              (+<span className="font-num tabular">{owned.other}</span> not looted)
            </span>
          )}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-80">
        <Places owned={owned} />
      </TooltipContent>
    </Tooltip>
  );
}

/** Where the player's items are, most first. */
export function Places({ owned }: { owned: Owned }) {
  if (owned.places.length === 0) return <p>None in your stashes.</p>;
  return (
    <ul className="flex flex-col gap-0.5">
      {owned.places.map((place) => (
        <li key={`${place.character}:${place.stash}`} className="flex gap-3">
          <span className="flex-1">
            {place.character} · {place.stash}
          </span>
          <span className="font-num tabular">{place.usable}</span>
          {place.other > 0 && <span className="text-muted-foreground">+{place.other} not looted</span>}
        </li>
      ))}
    </ul>
  );
}
