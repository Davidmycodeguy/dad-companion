import type { StatView } from "@/lib/api";
import { cn } from "@/lib/utils";

/** An item's random rolls as the game lists them: name, then the value in the roll colour. */
export function Rolls({ rolls, className }: { rolls: StatView[]; className?: string }) {
  if (rolls.length === 0) return <span className="text-muted-foreground/60">No rolls</span>;
  return (
    <span className={cn("flex flex-wrap gap-x-3 gap-y-0.5", className)}>
      {rolls.map((roll) => (
        <span key={roll.id} className="whitespace-nowrap">
          <span className="text-muted-foreground">{roll.label}</span>{" "}
          <span className="font-num tabular text-roll">{roll.text}</span>
        </span>
      ))}
    </span>
  );
}
