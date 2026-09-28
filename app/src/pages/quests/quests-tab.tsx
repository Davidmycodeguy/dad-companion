import { useMemo } from "react";
import { CircleCheck, Lock, ScrollText, Store, type LucideIcon } from "lucide-react";

import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { countQuests, filterQuests, type QuestFilter } from "@/lib/quests";
import type { MerchantSummary, QuestView } from "@/lib/quests-api";
import { MerchantPortrait } from "@/pages/quests/parts";
import { QuestCard } from "@/pages/quests/quest-card";
import { useMerchantQuests, useStoredState } from "@/pages/quests/use-quests";

const isFilter = (value: unknown): value is QuestFilter => value === "active" || value === "completed";

interface QuestsTabProps {
  merchants: MerchantSummary[];
  merchant: string | null;
  onMerchant: (merchant: string) => void;
  hideLocked: boolean;
  now: number;
}

export function QuestsTab({ merchants, merchant, onMerchant, hideLocked, now }: QuestsTabProps) {
  const [filter, setFilter] = useStoredState<QuestFilter>("quests.filter", "active", isFilter);
  const { data, isPending, isError, error } = useMerchantQuests(merchant);

  if (!merchant) {
    return (
      <Empty className="border border-dashed py-12">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Store />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">No merchants to show</EmptyTitle>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Select value={merchant} onValueChange={onMerchant}>
          <SelectTrigger className="h-9! w-64">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {merchants.map((entry) => (
              <SelectItem key={entry.name} value={entry.name}>
                <MerchantPortrait name={entry.name} size={20} className="rounded-sm" />
                <span className="flex-1">{entry.name}</span>
                <span className="font-num text-xs text-muted-foreground tabular">
                  {entry.done}/{entry.total}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ToggleGroup
          type="single"
          value={filter}
          onValueChange={(value) => isFilter(value) && setFilter(value)}
          variant="outline"
          size="sm"
          spacing={0}
          className="ml-auto"
        >
          <ToggleGroupItem value="active" className="px-3 text-xs">
            Active
          </ToggleGroupItem>
          <ToggleGroupItem value="completed" className="px-3 text-xs">
            Completed
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      {isPending ? (
        <ListSkeleton />
      ) : isError ? (
        <p className="text-sm text-muted-foreground">{String(error)}</p>
      ) : (
        <QuestList quests={data.quests} filter={filter} hideLocked={hideLocked} now={now} />
      )}
    </div>
  );
}

function QuestList({ quests, filter, hideLocked, now }: { quests: QuestView[]; filter: QuestFilter; hideLocked: boolean; now: number }) {
  const shown = useMemo(() => filterQuests(quests, filter, hideLocked), [quests, filter, hideLocked]);
  const positions = useMemo(() => new Map(quests.map((quest, index) => [quest.id, index + 1])), [quests]);
  const counts = countQuests(shown);
  const hiddenLocked = hideLocked && filter === "active" ? quests.filter((quest) => quest.state === "locked").length : 0;

  if (shown.length === 0) return <NothingShown filter={filter} hiddenLocked={hiddenLocked} />;
  return (
    <div className="flex flex-col gap-2">
      <p className="px-1 text-xs text-muted-foreground">
        <span className="font-num text-foreground tabular">{counts.shown}</span> quest{counts.shown === 1 ? "" : "s"}
        {filter === "active" && (
          <>
            {" · "}
            <span className="font-num text-foreground tabular">{counts.objectivesLeft}</span> objective
            {counts.objectivesLeft === 1 ? "" : "s"} left{" · "}
            <span className="font-num text-foreground tabular">{counts.itemsLeft}</span> item{counts.itemsLeft === 1 ? "" : "s"} to bring
          </>
        )}
        {hiddenLocked > 0 && ` · ${hiddenLocked} locked hidden`}
      </p>
      {shown.map((quest) => (
        <QuestCard key={quest.id} quest={quest} position={positions.get(quest.id) ?? 0} now={now} />
      ))}
    </div>
  );
}

function NothingShown({ filter, hiddenLocked }: { filter: QuestFilter; hiddenLocked: number }) {
  const { Icon, title, description }: { Icon: LucideIcon; title: string; description: string | null } =
    filter === "completed"
      ? { Icon: ScrollText, title: "Nothing done here yet", description: null }
      : hiddenLocked > 0
        ? {
            Icon: Lock,
            title: "Only locked quests left",
            description: `${hiddenLocked} quest${hiddenLocked === 1 ? " waits" : "s wait"} for earlier quests.`,
          }
        : { Icon: CircleCheck, title: "Every quest here is done", description: null };
  return (
    <Empty className="border border-dashed py-10">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <Icon />
        </EmptyMedia>
        <EmptyTitle className="font-display text-lg">{title}</EmptyTitle>
        {description && <EmptyDescription>{description}</EmptyDescription>}
      </EmptyHeader>
    </Empty>
  );
}

function ListSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      <Skeleton className="h-4 w-64" />
      <Skeleton className="h-44" />
      <Skeleton className="h-11" />
      <Skeleton className="h-11" />
    </div>
  );
}
