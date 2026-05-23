import { ChevronRight, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Checkbox } from "@/components/ui/checkbox"
import type { Workspace } from "@/lib/api"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

const STATUS_COLOR: Record<string, string> = {
  done: "bg-emerald-500",
  pending: "bg-zinc-700",
  running: "bg-blue-500 animate-pulse",
  failed: "bg-red-500",
  awaiting_user: "bg-amber-500",
  skipped: "bg-zinc-600",
}

type Props = {
  ws: Workspace
  selected: boolean
  onSelectChange: (slug: string, selected: boolean) => void
  expanded: boolean
  onToggle: (slug: string) => void
}

export function WorkspaceRow({ ws, selected, onSelectChange, expanded, onToggle }: Props) {
  return (
    <div className={cn(
      "rounded-md border border-zinc-800 bg-zinc-900/50",
      selected && "ring-1 ring-blue-500",
    )}>
      <button
        onClick={() => onToggle(ws.slug)}
        className="w-full px-3 py-2 flex items-center gap-3 text-left"
      >
        {expanded ? <ChevronDown className="h-4 w-4 text-zinc-500" /> : <ChevronRight className="h-4 w-4 text-zinc-500" />}
        <Checkbox
          checked={selected}
          onCheckedChange={(c) => onSelectChange(ws.slug, !!c)}
          onClick={(e) => e.stopPropagation()}
        />
        <div className="w-12 h-7 rounded bg-zinc-800" /> {/* thumb placeholder */}
        <div className="font-medium text-sm flex-1 truncate">{ws.slug}</div>
        <div className="flex gap-0.5">
          {STAGES.map((s) => (
            <div
              key={s}
              title={`${s}: ${ws.stages[s] ?? "pending"}`}
              className={cn("w-2.5 h-2.5 rounded-sm", STATUS_COLOR[ws.stages[s] ?? "pending"])}
            />
          ))}
        </div>
      </button>
    </div>
  )
}
