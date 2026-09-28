import { useState, type ReactNode } from "react";
import { ListChecks } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { CharacterSummary } from "@/lib/api";
import type { ListerRules, ListerSource, PriceSource } from "@/lib/lister-api";
import { MAX_ITEMS_PER_RUN, MAX_UNDERCUT_PCT, MIN_RARITY_OPTIONS, PRICE_SOURCE_OPTIONS, UNDERCUT_PRESETS } from "@/lib/lister";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";

interface RulesPanelProps {
  characters: CharacterSummary[];
  characterId: string | undefined;
  onCharacter: (id: string) => void;
  sources: ListerSource[];
  rules: ListerRules;
  onRules: (rules: ListerRules) => void;
  onBuild: () => void;
  building: boolean;
  disabled: boolean;
}

/** What to sell, how to price it, and the button that builds the plan. */
export function RulesPanel(props: RulesPanelProps) {
  const { rules, onRules } = props;
  const set = <K extends keyof ListerRules>(key: K, value: ListerRules[K]) => onRules({ ...rules, [key]: value });
  const toggleSource = (id: string, on: boolean) =>
    set("sourceStashIds", on ? [...rules.sourceStashIds, id] : rules.sourceStashIds.filter((s) => s !== id));

  return (
    <Card className="gap-4 lg:sticky lg:top-4">
      <CardHeader>
        <CardTitle className="font-display text-base">Rules</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <Field label="Character">
          <Select value={props.characterId} onValueChange={props.onCharacter} disabled={props.disabled}>
            <SelectTrigger className="w-full">
              <SelectValue placeholder="No characters yet" />
            </SelectTrigger>
            <SelectContent>
              {props.characters.map((c) => (
                <SelectItem key={c.id} value={c.id}>
                  {c.name} <span className="text-muted-foreground">· {c.class}</span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>

        <Field label="List from">
          <div className="flex flex-col gap-1.5">
            {props.sources.length === 0 && <p className="text-sm text-muted-foreground">No stash tabs read yet.</p>}
            {props.sources.map((source) => (
              <Label key={source.id} className="flex items-center gap-2 font-normal">
                <Checkbox
                  checked={rules.sourceStashIds.includes(source.id)}
                  onCheckedChange={(on) => toggleSource(source.id, on === true)}
                />
                <span className="flex-1">{source.label}</span>
                <span className="font-num text-xs text-muted-foreground tabular">{source.items}</span>
              </Label>
            ))}
          </div>
        </Field>

        <div className="grid grid-cols-2 gap-3">
          <Field label="Min rarity">
            <Select value={String(rules.minRarity)} onValueChange={(v) => set("minRarity", Number(v))}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {MIN_RARITY_OPTIONS.map((option) => (
                  <SelectItem key={option.id} value={String(option.id)}>
                    <span className={RARITY_TEXT[option.name]}>{option.name}</span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Min price" hint="gold">
            <NumberInput value={rules.minPrice} min={0} onChange={(v) => set("minPrice", v)} />
          </Field>
        </div>

        <Field label="Undercut" hint="% below comparable listings">
          <div className="flex items-center gap-2">
            <NumberInput
              value={rules.undercutPct}
              min={0}
              max={MAX_UNDERCUT_PCT}
              onChange={(v) => set("undercutPct", v)}
              className="w-16"
            />
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={String(rules.undercutPct)}
              onValueChange={(v) => v && set("undercutPct", Number(v))}
            >
              {UNDERCUT_PRESETS.map((preset) => (
                <ToggleGroupItem key={preset.label} value={String(preset.pct)} title={preset.hint} className="px-2.5 text-xs">
                  {preset.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        </Field>

        <Field label="Prices from">
          <RadioGroup value={rules.priceSource} onValueChange={(v) => set("priceSource", v as PriceSource)} className="gap-1.5">
            {PRICE_SOURCE_OPTIONS.map((option) => (
              <Label
                key={option.value}
                className={cn(
                  "flex items-start gap-2.5 rounded-md border px-3 py-2 font-normal transition-colors",
                  rules.priceSource === option.value ? "border-primary/60 bg-primary/8" : "hover:bg-accent/50",
                )}
              >
                <RadioGroupItem value={option.value} className="mt-0.5" />
                <span className="flex flex-col gap-0.5">
                  <span className="text-sm">{option.label}</span>
                  <span className="text-xs text-muted-foreground">{option.hint}</span>
                </span>
              </Label>
            ))}
          </RadioGroup>
        </Field>

        <div className="grid grid-cols-2 items-end gap-3">
          <Field label="Max items per run">
            <NumberInput value={rules.maxItemsPerRun} min={1} max={MAX_ITEMS_PER_RUN} onChange={(v) => set("maxItemsPerRun", v)} />
          </Field>
          <Label className="flex h-9 items-center gap-2 font-normal" title="Potions, bandages and other stacks, priced per unit">
            <Switch checked={rules.allowStacks} onCheckedChange={(v) => set("allowStacks", v)} />
            Include stacks
          </Label>
        </div>
      </CardContent>
      <CardFooter>
        <Button className="w-full" onClick={props.onBuild} disabled={props.disabled || props.building || !props.characterId}>
          {props.building ? <Spinner /> : <ListChecks />}
          Build plan
        </Button>
      </CardFooter>
    </Card>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-muted-foreground">
        {label}
        {hint && <span className="font-normal text-muted-foreground/70"> · {hint}</span>}
      </span>
      {children}
    </div>
  );
}

interface NumberInputProps {
  value: number;
  min: number;
  max?: number;
  onChange: (value: number) => void;
  className?: string;
}

/** A whole-number field: typing reports values inside its range, leaving the field settles it. */
function NumberInput({ value, min, max, onChange, className }: NumberInputProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const clamp = (n: number) => Math.min(max ?? Infinity, Math.max(min, Math.round(n)));
  return (
    <Input
      type="number"
      inputMode="numeric"
      className={cn("font-num tabular", className)}
      value={draft ?? value}
      min={min}
      max={max}
      onChange={(event) => {
        setDraft(event.target.value);
        const next = Number(event.target.value);
        if (event.target.value !== "" && Number.isFinite(next)) onChange(clamp(next));
      }}
      onBlur={() => setDraft(null)}
    />
  );
}
