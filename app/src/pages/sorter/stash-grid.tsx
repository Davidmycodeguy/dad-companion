import type { CSSProperties } from "react";

import { ItemSlot } from "@/components/item-slot";
import { Skeleton } from "@/components/ui/skeleton";
import type { GridItem } from "@/lib/sorter-api";
import { cn } from "@/lib/utils";

/** Side of one cell in the sorter's grids, in CSS pixels: two stash tabs fit side by side. */
export const CELL = 26;

interface StashGridProps {
  items: GridItem[];
  /** [width, height] in cells. */
  grid: [number, number];
  /** How items flagged `moves` stand out: "moving" dims everything else, "incoming" rings them. */
  emphasis?: "moving" | "incoming";
  className?: string;
}

/** A stash or bag laid out like the game's grid, items in rarity-lit slots. */
export function StashGrid({ items, grid, emphasis, className }: StashGridProps) {
  const [columns, rows] = grid;
  return (
    <div
      className={cn("relative shrink-0 rounded-md border border-[#2a241c] bg-[#0b0a08]", className)}
      style={{
        width: columns * CELL + 2,
        height: rows * CELL + 2,
        backgroundImage: "linear-gradient(#1a1712 1px, transparent 1px), linear-gradient(90deg, #1a1712 1px, transparent 1px)",
        backgroundSize: `${CELL}px ${CELL}px`,
        backgroundPosition: "1px 1px",
      }}
    >
      {items.map((item) => (
        <div
          key={item.uniqueId}
          title={`${item.name}${item.count > 1 ? ` ×${item.count}` : ""}`}
          className={cn(
            "absolute p-px transition-opacity",
            emphasis === "moving" && !item.moves && "opacity-45",
            emphasis === "incoming" && item.moves && "z-10",
          )}
          style={{ left: item.x * CELL + 1, top: item.y * CELL + 1, width: item.width * CELL, height: item.height * CELL } as CSSProperties}
        >
          <ItemSlot
            icon={item.icon}
            rarity={item.rarity}
            size={CELL - 2}
            cells={{ width: item.width, height: item.height }}
            className={cn("size-full!", emphasis === "incoming" && item.moves && "outline outline-1 outline-gold")}
          />
          {item.count > 1 && (
            <span className="absolute right-0.5 bottom-0 font-num text-[9px] leading-tight text-foreground tabular [text-shadow:0_1px_2px_#000]">
              {item.count.toLocaleString()}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

/** Placeholder with the grid's footprint while the stash loads. */
export function GridSkeleton({ grid }: { grid: [number, number] }) {
  return <Skeleton className="shrink-0 rounded-md" style={{ width: grid[0] * CELL + 2, height: grid[1] * CELL + 2 }} />;
}
