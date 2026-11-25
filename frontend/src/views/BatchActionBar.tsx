import { useState } from "react"
import { Button } from "@/components/ui/button"
import { api } from "@/lib/api"
import { ResetStageDialog } from "./ResetStageDialog"

export function BatchActionBar({
  selected,
  lockedSlugs,
  onBeforeMutateWorkspaces,
  onCleared,
}: {
  selected: Set<string>
  lockedSlugs: Set<string>
  onBeforeMutateWorkspaces: (slugs: string[]) => void
  onCleared: () => void
}) {
  const [resetOpen, setResetOpen] = useState(false)
  if (selected.size === 0) return null
  const slugs = Array.from(selected)
  const anyLocked = slugs.some((s) => lockedSlugs.has(s))

  return (
    <div className="sticky top-0 z-30 backdrop-blur-md bg-primary/15 border-b border-primary/30 px-6 py-2.5 flex items-center gap-2 shadow-lg shadow-primary/10">
      <div className="text-sm font-medium">
        <span className="text-primary">{selected.size}</span> selected
        {anyLocked && (
          <span className="ml-2 text-xs text-violet-300">(includes locked workspace)</span>
        )}
      </div>
      <div className="flex-1" />
      <Button size="sm" disabled={anyLocked} onClick={() => void api.batch.run({ slugs }).then(onCleared)}>
        Run
      </Button>
      <Button size="sm" variant="secondary" disabled={anyLocked} onClick={() => void api.batch.approve({ slugs }).then(onCleared)}>
        Approve
      </Button>
      <Button size="sm" variant="outline" disabled={anyLocked} onClick={() => setResetOpen(true)}>
        Reset…
      </Button>
      <Button
        size="sm"
        variant="destructive"
        disabled={anyLocked}
        onClick={() => {
          if (window.confirm(`Delete ${selected.size} workspaces?`)) {
            onBeforeMutateWorkspaces(slugs)
            void api.batch.clean({ slugs }).then(onCleared)
          }
        }}
      >
        Clean
      </Button>
      <Button size="sm" variant="ghost" onClick={onCleared}>Cancel</Button>
      <ResetStageDialog
        open={resetOpen}
        onOpenChange={setResetOpen}
        target={`${selected.size} workspace${selected.size === 1 ? "" : "s"}`}
        onConfirm={(stage) => {
          onBeforeMutateWorkspaces(slugs)
          void api.batch.reset({ slugs, from: stage }).then(onCleared)
        }}
      />
    </div>
  )
}
