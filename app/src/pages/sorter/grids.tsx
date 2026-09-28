import type { ReactNode } from "react";
import { LayoutGrid, RotateCw, TriangleAlert } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import type { SortPreview } from "@/lib/sorter-api";
import { CELL, GridSkeleton, StashGrid } from "@/pages/sorter/stash-grid";

const STASH_GRID: [number, number] = [12, 20];

interface GridsProps {
  preview: SortPreview | undefined;
  error: unknown;
  onRetry: () => void;
}

/** The stash now and once sorted, side by side; the bag under "Now" when items come over from it. */
export function Grids({ preview, error, onRetry }: GridsProps) {
  if (error && !preview) {
    return (
      <Empty className="border border-dashed py-10">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <TriangleAlert />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">The stash couldn't be read</EmptyTitle>
          <EmptyDescription>{String(error)}</EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button variant="outline" size="sm" onClick={onRetry}>
            <RotateCw />
            Try again
          </Button>
        </EmptyContent>
      </Empty>
    );
  }
  const grid = preview?.grid ?? STASH_GRID;
  return (
    <div className="flex flex-wrap items-start gap-6">
      <Figure caption="Now">
        {preview ? <StashGrid items={preview.current} grid={grid} /> : <GridSkeleton grid={grid} />}
        {preview && preview.incoming > 0 && (
          <div className="flex flex-col gap-1.5 pt-2">
            <span className="text-xs text-muted-foreground">Bag</span>
            <StashGrid items={preview.bag} grid={preview.bagGrid} emphasis="incoming" />
          </div>
        )}
      </Figure>
      <Figure caption="Sorted">
        {!preview ? <GridSkeleton grid={grid} /> : <SortedGrid preview={preview} />}
      </Figure>
    </div>
  );
}

function Figure({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <figure className="flex flex-col gap-2">
      <figcaption className="font-display text-base">{caption}</figcaption>
      {children}
    </figure>
  );
}

function SortedGrid({ preview }: { preview: SortPreview }) {
  if (preview.sorted.length > 0) return <StashGrid items={preview.sorted} grid={preview.grid} emphasis="moving" />;
  const empty = preview.current.length === 0 && preview.incoming === 0;
  return (
    <Empty
      className="shrink-0 border border-dashed"
      style={{ width: preview.grid[0] * CELL + 2, height: preview.grid[1] * CELL + 2 }}
    >
      <EmptyHeader>
        <EmptyMedia variant="icon">{empty ? <LayoutGrid /> : <TriangleAlert />}</EmptyMedia>
        <EmptyTitle className="font-display text-base">{empty ? "Nothing to sort" : "No sorted layout"}</EmptyTitle>
        <EmptyDescription>{empty ? "This tab is empty." : preview.blocked}</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}
