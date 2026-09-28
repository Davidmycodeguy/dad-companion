import { useState } from "react";
import {
  CircleCheck,
  CircleDashed,
  Compass,
  DoorOpen,
  Flame,
  Hand,
  Hourglass,
  LoaderCircle,
  Lock,
  RadioTower,
  Swords,
  Target,
  type LucideIcon,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatAge } from "@/lib/format";
import { merchantPortrait, secondsSince, stateLabel } from "@/lib/quests";
import type { QuestState, Source } from "@/lib/quests-api";
import { cn } from "@/lib/utils";

/** A merchant's portrait, or their initials when there is none. */
export function MerchantPortrait({ name, size = 56, className }: { name: string; size?: number; className?: string }) {
  const [broken, setBroken] = useState(false);
  const src = merchantPortrait(name);
  const style = { width: size, height: size };
  if (!src || broken) {
    const initials = name
      .split(" ")
      .map((word) => word[0])
      .join("")
      .slice(0, 2);
    return (
      <div style={style} className={cn("grid shrink-0 place-items-center rounded-md border bg-muted font-display text-muted-foreground", className)}>
        {initials}
      </div>
    );
  }
  return (
    <div style={style} className={cn("relative shrink-0 overflow-hidden rounded-md border border-[#3a3024] bg-[#0b0a08]", className)}>
      <img src={src} alt="" draggable={false} loading="lazy" onError={() => setBroken(true)} className="size-full object-cover" />
      <div className="pointer-events-none absolute inset-0 shadow-[inset_0_0_12px_rgb(0_0_0/0.7)]" />
    </div>
  );
}

const STATE_STYLE: Record<QuestState, { icon: LucideIcon; className: string }> = {
  locked: { icon: Lock, className: "text-muted-foreground" },
  available: { icon: CircleDashed, className: "text-muted-foreground" },
  active: { icon: LoaderCircle, className: "border-roll/30 text-roll" },
  ready: { icon: CircleCheck, className: "border-gold/40 bg-gold/10 text-gold" },
  done: { icon: CircleCheck, className: "border-profit/30 text-profit" },
};

/** A quest's state as a small badge (nothing for an open quest the game hasn't shown). */
export function StateBadge({ state, source }: { state: QuestState; source: Source | null }) {
  const label = stateLabel(state, source === "game");
  if (!label) return null;
  const { icon: Icon, className } = STATE_STYLE[state];
  return (
    <Badge variant="outline" className={cn("gap-1 font-normal", className)}>
      <Icon />
      {label}
    </Badge>
  );
}

/** The mark for data read from the game, with how long ago. */
export function LiveMark({ seenAt, now, className }: { seenAt: number | null; now: number; className?: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn("inline-flex text-profit", className)} aria-label="Read from the game">
          <RadioTower className="size-3.5" />
        </span>
      </TooltipTrigger>
      <TooltipContent>{seenAt ? `Read from the game ${formatAge(secondsSince(seenAt, now))}` : "Read from the game"}</TooltipContent>
    </Tooltip>
  );
}

const OBJECTIVE_ICONS: Record<string, LucideIcon> = {
  Kill: Swords,
  Explore: Compass,
  Props: Hand,
  Survive: DoorOpen,
  Hold: Hourglass,
  Damage: Flame,
};

/** A slot-sized square with the icon of an objective that takes no item. */
export function ObjectiveIcon({ kind, done, size = 36 }: { kind: string; done: boolean; size?: number }) {
  const Icon = OBJECTIVE_ICONS[kind] ?? Target;
  return (
    <div style={{ width: size, height: size }} className="item-slot">
      <Icon className={cn("size-1/2", done ? "text-profit/70" : "text-muted-foreground")} />
    </div>
  );
}
