import { ArrowDownWideNarrow, ArrowUpNarrowWide, ChevronDown, ChevronUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { directionLabel, FIELD_LABEL, moveEntry, toggleDirection } from "@/lib/sorter";
import type { SortOptions } from "@/lib/sorter-api";

interface OptionsPanelProps {
  options: SortOptions;
  onChange: (options: SortOptions) => void;
  disabled: boolean;
}

/** How the stash gets sorted: packing, stacking, the bag, and the order of fields. */
export function OptionsPanel({ options, onChange, disabled }: OptionsPanelProps) {
  const toggles: { key: "pack" | "stack" | "fromBag"; label: string }[] = [
    { key: "pack", label: "Pack tightly" },
    { key: "stack", label: "Stack partial stacks" },
    { key: "fromBag", label: "Bring items over from the bag" },
  ];
  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle className="font-display text-base">Options</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <div className="flex flex-col gap-3">
          {toggles.map(({ key, label }) => (
            <div key={key} className="flex items-center justify-between gap-3">
              <Label htmlFor={`sorter-${key}`} className="font-normal">
                {label}
              </Label>
              <Switch
                id={`sorter-${key}`}
                checked={options[key]}
                disabled={disabled}
                onCheckedChange={(checked) => onChange({ ...options, [key]: checked })}
              />
            </div>
          ))}
        </div>
        <section className="flex flex-col gap-2">
          <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Sort order</h3>
          <ol className="flex flex-col divide-y divide-border/60 overflow-hidden rounded-md border">
            {options.order.map((entry, index) => {
              const Direction = entry.direction === "desc" ? ArrowDownWideNarrow : ArrowUpNarrowWide;
              return (
                <li key={entry.field} className="flex items-center gap-2 bg-card px-2.5 py-1.5">
                  <span className="w-4 font-num text-xs text-muted-foreground tabular">{index + 1}</span>
                  <span className="w-14 text-sm">{FIELD_LABEL[entry.field]}</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 flex-1 justify-start gap-1.5 px-2 text-xs text-muted-foreground"
                    disabled={disabled}
                    onClick={() => onChange({ ...options, order: toggleDirection(options.order, index) })}
                  >
                    <Direction className="size-3.5" />
                    {directionLabel(entry)}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Move ${FIELD_LABEL[entry.field]} up`}
                    disabled={disabled || index === 0}
                    onClick={() => onChange({ ...options, order: moveEntry(options.order, index, -1) })}
                  >
                    <ChevronUp />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Move ${FIELD_LABEL[entry.field]} down`}
                    disabled={disabled || index === options.order.length - 1}
                    onClick={() => onChange({ ...options, order: moveEntry(options.order, index, 1) })}
                  >
                    <ChevronDown />
                  </Button>
                </li>
              );
            })}
          </ol>
        </section>
      </CardContent>
    </Card>
  );
}
