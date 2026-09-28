import { useState } from "react";
import { ChevronRight, Dices, Heart, MapPin, Sparkles } from "lucide-react";

import { Gold } from "@/components/gold";
import { ItemSlot } from "@/components/item-slot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import type { QuestView, RewardView } from "@/lib/quests-api";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";
import { ObjectiveRow } from "@/pages/quests/objective-row";
import { LiveMark, ObjectiveIcon, StateBadge } from "@/pages/quests/parts";
import { useSetQuestDone } from "@/pages/quests/use-quests";

interface QuestCardProps {
  quest: QuestView;
  /** Its place in the merchant's chain, from 1. */
  position: number;
  now: number;
}

/** One quest: open ones show their objectives and rewards; locked and finished ones fold to a line. */
export function QuestCard({ quest, position, now }: QuestCardProps) {
  const [open, setOpen] = useState(quest.state !== "locked" && quest.state !== "done");
  const settled = quest.state === "ready" || (quest.state === "done" && quest.source !== "manual");
  const done = quest.objectives.filter((objective) => objective.done).length;

  return (
    <Collapsible open={open} onOpenChange={setOpen} asChild>
      <article
        className={cn(
          "rounded-lg border bg-card",
          quest.state === "ready" && "border-gold/40",
          quest.state === "locked" && "bg-card/50",
        )}
      >
        <header className="flex items-center gap-2 px-3 py-2.5">
          <span className="w-6 shrink-0 text-right font-num text-xs text-muted-foreground tabular">{position}</span>
          <CollapsibleTrigger className="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
            <ChevronRight className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-90")} />
            <h3 className={cn("truncate font-display text-[15px]", quest.state === "locked" && "text-muted-foreground")}>
              {quest.title}
            </h3>
          </CollapsibleTrigger>
          {quest.timeLimit && (
            <Badge variant="outline" className="font-normal text-muted-foreground">
              {quest.timeLimit}
            </Badge>
          )}
          {quest.source === "game" && quest.seenAt !== null && <LiveMark seenAt={quest.seenAt} now={now} />}
          <StateBadge state={quest.state} source={quest.source} />
          <span className="w-9 shrink-0 text-right font-num text-xs text-muted-foreground tabular" title="Objectives done">
            {done}/{quest.objectives.length}
          </span>
          <DoneButton quest={quest} />
        </header>
        {!open && quest.state === "locked" && <LockedSummary quest={quest} />}
        <CollapsibleContent>
          <div className="flex flex-col gap-3 border-t border-border/60 px-4 pt-2.5 pb-3.5 sm:pl-11">
            <QuestMeta quest={quest} />
            {quest.text && (
              <p className="line-clamp-2 text-sm text-muted-foreground italic" title={quest.text}>
                {quest.text}
              </p>
            )}
            <ul className="divide-y divide-border/40">
              {quest.objectives.map((objective) => (
                <ObjectiveRow key={objective.index} questId={quest.id} objective={objective} settled={settled} />
              ))}
            </ul>
            {quest.unmatched && (
              <p className="text-xs text-muted-foreground">
                The game counts <span className="font-num text-foreground tabular">{quest.unmatched.submitted}</span> of{" "}
                <span className="font-num tabular">{quest.unmatched.needed}</span> handed in here, without saying which item.
              </p>
            )}
            {quest.rewards.length > 0 && <Rewards rewards={quest.rewards} />}
          </div>
        </CollapsibleContent>
      </article>
    </Collapsible>
  );
}

/** Tick a quest off (or undo that) when the game can't tell; nothing once the game has settled it. */
function DoneButton({ quest }: { quest: QuestView }) {
  const setDone = useSetQuestDone();
  if (quest.state === "done" && quest.source === "manual") {
    return (
      <Button variant="ghost" size="xs" className="text-muted-foreground" onClick={() => setDone.mutate({ questId: quest.id, done: false })}>
        Reopen
      </Button>
    );
  }
  if (quest.state === "done" || quest.state === "ready") return <span className="w-[4.5rem] shrink-0" />;
  return (
    <Button
      variant="ghost"
      size="xs"
      className="w-[4.5rem] text-muted-foreground"
      disabled={setDone.isPending}
      onClick={() => setDone.mutate({ questId: quest.id, done: true })}
    >
      Mark done
    </Button>
  );
}

function QuestMeta({ quest }: { quest: QuestView }) {
  const waiting = quest.prerequisite && !quest.prerequisite.done ? quest.prerequisite : null;
  if (quest.dungeons.length === 0 && !waiting) return null;
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {quest.dungeons.length > 0 && (
        <span className="inline-flex items-center gap-1">
          <MapPin className="size-3.5" />
          {quest.dungeons.join(", ")}
        </span>
      )}
      {waiting && (
        <span>
          Unlocks after <span className="text-foreground">{waiting.title}</span>
          {waiting.merchant && ` (${waiting.merchant})`}
        </span>
      )}
    </div>
  );
}

/** A folded locked quest: what it waits for and what it will ask. */
function LockedSummary({ quest }: { quest: QuestView }) {
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 px-3 pb-2.5 pl-[3.25rem] text-xs text-muted-foreground">
      {quest.prerequisite && !quest.prerequisite.done && <span>Unlocks after {quest.prerequisite.title}</span>}
      {quest.objectives.map((objective) => (
        <span key={objective.index} className="inline-flex items-center gap-1.5">
          {objective.item ? (
            <ItemSlot icon={objective.item.icon} rarity={objective.item.rarity} size={20} />
          ) : (
            <ObjectiveIcon kind={objective.kind} done={objective.done} size={20} />
          )}
          {objective.label}
        </span>
      ))}
    </div>
  );
}

function Rewards({ rewards }: { rewards: RewardView[] }) {
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border/40 pt-2.5 text-sm">
      {rewards.map((reward, index) => (
        <RewardChip key={`${reward.kind}:${reward.label}:${index}`} reward={reward} />
      ))}
    </div>
  );
}

function RewardChip({ reward }: { reward: RewardView }) {
  const times = reward.count > 1 ? `${reward.count} × ` : "";
  if (reward.gold) return <Gold value={reward.count} />;
  if (reward.item) {
    return (
      <span className="inline-flex items-center gap-1.5">
        <ItemSlot icon={reward.item.icon} rarity={reward.item.rarity} size={24} />
        <span className="font-num tabular">{times}</span>
        <span className={RARITY_TEXT[reward.item.rarity]}>{reward.label}</span>
      </span>
    );
  }
  if (reward.kind === "Experience") {
    return (
      <span className="text-muted-foreground">
        <span className="font-num text-foreground tabular">{reward.count}</span> XP
      </span>
    );
  }
  if (reward.kind === "Affinity") {
    return (
      <span className="inline-flex items-center gap-1 text-muted-foreground">
        <Heart className="size-3.5" />
        <span className="font-num text-foreground tabular">+{reward.count}</span> {reward.label}
      </span>
    );
  }
  const Icon = reward.kind === "Random" ? Dices : Sparkles;
  return (
    <span className="inline-flex items-center gap-1">
      <Icon className="size-3.5 text-muted-foreground" />
      <span className="font-num tabular">{times}</span>
      <span className={reward.rarity ? RARITY_TEXT[reward.rarity] : undefined}>{reward.label}</span>
    </span>
  );
}
