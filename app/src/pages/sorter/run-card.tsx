import { CircleCheck, CircleSlash, LoaderCircle, ThumbsDown, ThumbsUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { Progress } from "@/components/ui/progress";
import { progressPercent } from "@/lib/sorter";
import type { SorterStatus } from "@/lib/sorter-api";
import { cn } from "@/lib/utils";

interface RunCardProps {
  status: SorterStatus;
  onFeedback: (success: boolean) => void;
  sendingFeedback: boolean;
}

/** The sort in progress, move by move, and afterwards what it did. */
export function RunCard({ status, onFeedback, sendingFeedback }: RunCardProps) {
  if (status.state === "idle") return null;
  const running = status.state === "running";
  const done = status.state === "done";
  const label = status.stashLabel ?? "the stash";
  const percent = progressPercent(status);
  const Icon = running ? LoaderCircle : done ? CircleCheck : CircleSlash;
  const title = running ? `Sorting ${label}` : done ? `${label} sorted` : "Sort stopped";
  const detail = running ? (status.current ? `Moving ${status.current}` : (status.phase ?? "Starting")) : status.stoppedReason;
  return (
    <Card className="gap-3">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-display text-base">
          <Icon className={cn("size-4", running && "animate-spin text-primary", done && "text-profit", !running && !done && "text-muted-foreground")} />
          {title}
        </CardTitle>
        {detail && <CardDescription>{detail}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {percent !== undefined && <Progress value={percent} className="h-1.5" />}
        <dl className="grid grid-cols-4 gap-px overflow-hidden rounded-md border bg-border text-sm">
          <Stat label="Moved" value={status.done} of={status.total} />
          <Stat label="Verified" value={status.verified} />
          <Stat label="Retried" value={status.retried} />
          <Stat label="Failed" value={status.failed} tone={status.failed > 0 ? "text-loss" : undefined} />
        </dl>
        {running && status.hotkey && (
          <p className="text-xs text-muted-foreground">
            <KbdGroup>
              <Kbd>Ctrl</Kbd>
              <Kbd>F12</Kbd>
            </KbdGroup>{" "}
            or moving the mouse stops it at once.
          </p>
        )}
        {running && !status.hotkey && <p className="text-xs text-muted-foreground">Moving the mouse or Stop ends it at once.</p>}
        {!running && status.done > 0 && (
          <p className="text-xs text-muted-foreground">
            The stash here refreshes when the game sends it again: reopen your character in the game.
          </p>
        )}
        {!running && status.sessionId && (
          <Feedback sent={status.feedbackSent} sending={sendingFeedback} onFeedback={onFeedback} />
        )}
      </CardContent>
    </Card>
  );
}

function Stat({ label, value, of, tone }: { label: string; value: number; of?: number; tone?: string }) {
  return (
    <div className="bg-card px-3 py-2">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className={cn("font-num text-base tabular", tone)}>
        {value.toLocaleString()}
        {of !== undefined && of > 0 && <span className="text-muted-foreground"> / {of.toLocaleString()}</span>}
      </dd>
    </div>
  );
}

function Feedback({ sent, sending, onFeedback }: { sent: boolean; sending: boolean; onFeedback: (success: boolean) => void }) {
  if (sent) return <p className="text-xs text-muted-foreground">Thanks. The sorter learns from it.</p>;
  return (
    <div className="flex flex-wrap items-center gap-2 border-t pt-3 text-sm">
      <span className="mr-auto text-muted-foreground">Did the stash come out right?</span>
      <Button variant="outline" size="sm" disabled={sending} onClick={() => onFeedback(true)}>
        <ThumbsUp />
        Yes
      </Button>
      <Button variant="outline" size="sm" disabled={sending} onClick={() => onFeedback(false)}>
        <ThumbsDown />
        No
      </Button>
    </div>
  );
}
