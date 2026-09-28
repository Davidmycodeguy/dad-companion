import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Archive, LayoutGrid, Lock } from "lucide-react";

import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { stashApi, type CharacterSummary, type StashSummary } from "@/lib/api";
import { defaultTab, isRunning, sortBlocker, stashTabs } from "@/lib/sorter";
import { Grids } from "@/pages/sorter/grids";
import { OptionsPanel } from "@/pages/sorter/options-panel";
import { ConfirmFirstSort, RiskNotice } from "@/pages/sorter/risk-notice";
import { RunCard } from "@/pages/sorter/run-card";
import { SortPanel } from "@/pages/sorter/sort-panel";
import {
  useEditableOptions,
  useRefreshAfterRun,
  useSortFeedback,
  useSortPreview,
  useSorterStatus,
  useStartSort,
  useStopSort,
} from "@/pages/sorter/use-sorter";

export function SorterPage() {
  const { data: characters, isPending } = useQuery({ queryKey: ["characters"], queryFn: stashApi.characters });
  const { options, update, riskAccepted } = useEditableOptions();
  const { data: status } = useSorterStatus();
  const [chosenCharacter, setChosenCharacter] = useState<string>();
  const [chosenTab, setChosenTab] = useState<number | null>(null);
  const [confirming, setConfirming] = useState(false);
  const running = isRunning(status);
  useRefreshAfterRun(running);

  // While a run goes, the page follows the stash it sorts.
  const characterId = (running ? status?.characterId : null) ?? chosenCharacter;
  const character = characters?.find((c) => c.id === characterId) ?? characters?.[0];
  const tabs = character ? stashTabs(character.stashes) : [];
  const wanted = (running ? status?.stashId : null) ?? chosenTab;
  const stashId = tabs.some((t) => t.id === wanted && !t.locked) ? wanted : character ? defaultTab(character.stashes) : null;

  const preview = useSortPreview(character?.id, stashId, options);
  const start = useStartSort();
  const stop = useStopSort();
  const feedback = useSortFeedback();
  const blocker = sortBlocker(preview.data, status, preview.isError);

  const sort = (acceptRisk: boolean) => {
    if (!character || stashId === null || !options) return;
    start.mutate({ characterId: character.id, stashId, options, acceptRisk });
  };

  if (isPending || !options) return <Skeleton className="m-6 h-96" />;
  if (!character) return <NoCharacters />;

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-5 p-6">
      <RiskNotice />
      <header className="flex flex-wrap items-center justify-between gap-3">
        <CharacterPicker characters={characters ?? []} character={character} onPick={setChosenCharacter} disabled={running} />
        <TabPicker tabs={tabs} value={stashId} onPick={setChosenTab} disabled={running} />
      </header>
      {stashId === null ? (
        <NoTabs />
      ) : (
        <div className="grid items-start gap-5 lg:grid-cols-[300px_minmax(0,1fr)]">
          <div className="flex flex-col gap-5">
            <SortPanel
              preview={preview.data}
              blocker={blocker}
              running={running}
              planning={preview.isFetching}
              starting={start.isPending}
              stopping={stop.isPending}
              onSort={() => (riskAccepted ? sort(false) : setConfirming(true))}
              onStop={() => stop.mutate()}
            />
            <OptionsPanel options={options} onChange={update} disabled={running} />
          </div>
          <div className="flex min-w-0 flex-col gap-5">
            {status && (
              <RunCard
                status={status}
                onFeedback={(success) => status.sessionId && feedback.mutate({ sessionId: status.sessionId, success })}
                sendingFeedback={feedback.isPending}
              />
            )}
            <Grids preview={preview.data} error={preview.error} onRetry={() => void preview.refetch()} />
          </div>
        </div>
      )}
      <ConfirmFirstSort
        open={confirming}
        stashLabel={preview.data?.stashLabel ?? "this stash"}
        moves={preview.data?.moves ?? 0}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          sort(true);
        }}
      />
    </div>
  );
}

function CharacterPicker({ characters, character, onPick, disabled }: { characters: CharacterSummary[]; character: CharacterSummary; onPick: (id: string) => void; disabled: boolean }) {
  if (characters.length < 2) {
    return (
      <div>
        <h2 className="font-display text-xl">{character.name}</h2>
        <p className="text-sm text-muted-foreground">
          {character.class} · level {character.level}
        </p>
      </div>
    );
  }
  return (
    <Select value={character.id} onValueChange={onPick} disabled={disabled}>
      <SelectTrigger className="w-64">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {characters.map((c) => (
          <SelectItem key={c.id} value={c.id}>
            <span className="font-display">{c.name}</span> <span className="text-muted-foreground">· {c.class} {c.level}</span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function TabPicker({ tabs, value, onPick, disabled }: { tabs: StashSummary[]; value: number | null; onPick: (id: number) => void; disabled: boolean }) {
  return (
    <ToggleGroup
      type="single"
      value={value !== null ? String(value) : undefined}
      onValueChange={(next) => next && onPick(Number(next))}
      variant="outline"
      size="sm"
      spacing={1}
      className="flex-wrap justify-end"
      disabled={disabled}
    >
      {tabs.map((tab) => (
        <ToggleGroupItem key={tab.id} value={String(tab.id)} disabled={tab.locked} className="gap-1.5 px-2.5 text-xs">
          {tab.locked && <Lock className="size-3" />}
          {tab.label}
          <span className="font-num text-muted-foreground tabular">{tab.items}</span>
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

function NoCharacters() {
  return (
    <div className="grid h-full place-items-center p-8">
      <Empty className="max-w-md border border-dashed">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Archive />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">No characters yet</EmptyTitle>
          <EmptyDescription>Log in to the game with the app running: your stash tabs show up here, ready to sort.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    </div>
  );
}

function NoTabs() {
  return (
    <Empty className="border border-dashed py-10">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <LayoutGrid />
        </EmptyMedia>
        <EmptyTitle className="font-display text-lg">No stash tabs to sort</EmptyTitle>
        <EmptyDescription>This character's stash tabs show up here once the game has sent them.</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}
