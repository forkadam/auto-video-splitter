import { Button } from "@/components/ui/button"
import { api } from "@/lib/api"

export function BatchActionBar({
  selected, onCleared,
}: { selected: Set<string>; onCleared: () => void }) {
  if (selected.size === 0) return null
  const slugs = Array.from(selected)
  return (
    <div className="sticky top-0 z-30 backdrop-blur-md bg-primary/15 border-b border-primary/30 px-6 py-2.5 flex items-center gap-2 shadow-lg shadow-primary/10">
      <div className="text-sm font-medium">
        <span className="text-primary">{selected.size}</span> selected
      </div>
      <div className="flex-1" />
      <Button size="sm" onClick={() => api.batch.run({ slugs }).then(onCleared)}>Run</Button>
      <Button size="sm" variant="secondary" onClick={() => api.batch.approve({ slugs }).then(onCleared)}>Approve</Button>
      <Button size="sm" variant="outline" onClick={() => {
        const stage = window.prompt("Reset from which stage?", "detect")
        if (stage) api.batch.reset({ slugs, from: stage }).then(onCleared)
      }}>Reset…</Button>
      <Button size="sm" variant="destructive" onClick={() => {
        if (window.confirm(`Delete ${selected.size} workspaces?`))
          api.batch.clean({ slugs }).then(onCleared)
      }}>Clean</Button>
      <Button size="sm" variant="ghost" onClick={onCleared}>Cancel</Button>
    </div>
  )
}
