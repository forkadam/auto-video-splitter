import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: (stage: string) => void
  /** Pre-selected stage when the dialog opens. Defaults to "detect". */
  defaultStage?: string
  /** What's being reset — affects the dialog title/description. */
  target: string
}

export function ResetStageDialog({ open, onOpenChange, onConfirm, defaultStage = "detect", target }: Props) {
  const [stage, setStage] = useState<string>(defaultStage)

  // Reset selection each time the dialog re-opens so the previous choice
  // doesn't carry over across unrelated invocations.
  useEffect(() => {
    if (open) setStage(defaultStage)
  }, [open, defaultStage])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Reset {target}</DialogTitle>
          <DialogDescription>
            Choose the earliest stage to re-run. Output for that stage and every later stage is deleted.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">from:</span>
          <Select value={stage} onValueChange={setStage}>
            <SelectTrigger className="flex-1">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STAGES.map((s) => (
                <SelectItem key={s} value={s}>{s}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <DialogFooter className="gap-2 sm:gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="destructive" onClick={() => { onConfirm(stage); onOpenChange(false) }}>
            Reset
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
