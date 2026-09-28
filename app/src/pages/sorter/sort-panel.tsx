import { LayoutGrid, LoaderCircle, Square } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { formatAge } from "@/lib/format";
import { countLabel } from "@/lib/sorter";
import type { SortPreview } from "@/lib/sorter-api";

interface SortPanelProps {
  preview: SortPreview | undefined;
  /** Why Sort can't start, or null. */
  blocker: string | null;
  running: boolean;
  planning: boolean;
  starting: boolean;
  stopping: boolean;
  onSort: () => void;
  onStop: () => void;
}

/** The planned moves for the chosen tab, and the button that starts or stops the sort. */
export function SortPanel({ preview, blocker, running, planning, starting, stopping, onSort, onStop }: SortPanelProps) {
  const details = preview
    ? [
        preview.moving > 0 && countLabel(preview.moving, "item") + " change place",
        preview.merges > 0 && countLabel(preview.merges, "stack") + " merged",
        preview.incoming > 0 && `${preview.incoming} from the bag`,
        preview.bagMoves > preview.incoming && `${preview.bagMoves - preview.incoming} parked in the bag`,
      ].filter(Boolean)
    : [];
  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-display text-base">
          {preview?.stashLabel ?? "Stash"}
          {planning && <LoaderCircle className="size-3.5 animate-spin text-muted-foreground" />}
        </CardTitle>
        {preview?.dataAgeS != null && <CardDescription>Stash read {formatAge(preview.dataAgeS)}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-baseline gap-2">
          <span className="font-num text-3xl leading-none tabular">{preview && !preview.blocked ? preview.moves.toLocaleString() : "–"}</span>
          <span className="text-sm text-muted-foreground">{preview?.moves === 1 ? "move" : "moves"}</span>
        </div>
        {details.length > 0 && <p className="text-xs text-muted-foreground">{details.join(" · ")}</p>}
        {blocker && !running && <p className="text-sm text-muted-foreground">{blocker}</p>}
        {running ? (
          <Button variant="destructive" onClick={onStop} disabled={stopping}>
            <Square />
            Stop
          </Button>
        ) : (
          <Button onClick={onSort} disabled={!!blocker || starting}>
            <LayoutGrid />
            Sort {preview?.stashLabel ?? ""}
          </Button>
        )}
      </CardContent>
    </Card>
  );
}
