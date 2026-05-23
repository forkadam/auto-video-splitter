import { Button } from "@/components/ui/button"
import { api } from "@/lib/api"

export function BatchActionBar({
  selected, onCleared,
}: { selected: Set<string>; onCleared: () => void }) {
  if (selected.size === 0) return null
  const slugs = Array.from(selected)
  return (
    <div className="sticky top-0 z-10 bg-zinc-950/95 backdrop-blur border-b border-zinc-800 px-4 py-2 flex items-center gap-2">
      <div className="text-sm">{selected.size} selected</div>
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
