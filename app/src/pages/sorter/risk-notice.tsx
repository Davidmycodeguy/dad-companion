import { MousePointerClick } from "lucide-react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Kbd, KbdGroup } from "@/components/ui/kbd";

function StopKeys() {
  return (
    <KbdGroup>
      <Kbd>Ctrl</Kbd>
      <Kbd>F12</Kbd>
    </KbdGroup>
  );
}

/** The same notice as the Auto lister page: automated input, its risk, and the stop keys. */
export function RiskNotice() {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-gold/25 bg-gold/5 px-4 py-2.5 text-sm">
      <MousePointerClick className="mt-0.5 size-4 shrink-0 text-gold" />
      <p className="flex-1 text-muted-foreground">
        Sorting moves your mouse and clicks in the game. Automated input may break Dark and Darker's Terms of Service, so use
        it at your own risk. Stop at any time with <StopKeys />.
      </p>
    </div>
  );
}

interface ConfirmFirstSortProps {
  open: boolean;
  stashLabel: string;
  moves: number;
  onCancel: () => void;
  onConfirm: () => void;
}

/** The first sort ever asks once before the app takes the mouse. */
export function ConfirmFirstSort({ open, stashLabel, moves, onCancel, onConfirm }: ConfirmFirstSortProps) {
  return (
    <AlertDialog open={open} onOpenChange={(next) => !next && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-display">Sort {stashLabel}?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="flex flex-col gap-2">
              <p>
                The app brings the game to the front and drags items for you, <span className="font-num tabular">{moves}</span>{" "}
                move{moves === 1 ? "" : "s"} in all. Automated input may break Dark and Darker's Terms of Service.
              </p>
              <p>
                Keep your hands off the mouse until it's done. Moving it, leaving the game, or <StopKeys /> stops the sort at once.
              </p>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>Sort {stashLabel}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
