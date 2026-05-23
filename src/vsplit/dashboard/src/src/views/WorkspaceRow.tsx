import { useEffect, useState } from "react"
import { ChevronRight, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { api, type Workspace } from "@/lib/api"

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

type Detail = {
  workspace_path?: string
  manifest?: {
    stages?: Record<string, { artifacts?: Record<string, unknown> }>
  }
}

export function WorkspaceRow({ ws, selected, onSelectChange, expanded, onToggle }: Props) {
  const [detail, setDetail] = useState<Detail | null>(null)

  useEffect(() => {
    if (!expanded) return
    api.workspaces.detail(ws.slug).then((d) => setDetail(d as unknown as Detail))
  }, [expanded, ws.slug])

  const imagesGenerated = Number(
    detail?.manifest?.stages?.preview?.artifacts?.images_generated ?? 0,
  )

  const previewHref = detail?.workspace_path
    ? `file:///${detail.workspace_path.replace(/\\/g, "/")}/preview/index.html`
    : "#"

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

      {expanded && (
        <div className="px-3 pb-3 pt-1 border-t border-zinc-800 space-y-2">
          {imagesGenerated > 0 && (
            <div className="flex gap-1 overflow-x-auto">
              {Array.from({ length: Math.min(imagesGenerated, 6) }).map((_, i) => (
                <div key={i} className="w-20 h-12 rounded bg-zinc-800 flex-shrink-0" />
              ))}
            </div>
          )}
          <div className="flex gap-2 flex-wrap">
            <Button size="sm" onClick={() => api.workspaces.run(ws.slug, {})}>
              Run
            </Button>
            {ws.any_awaiting && (
              <Button size="sm" variant="secondary"
                      onClick={() => api.workspaces.approve(ws.slug)}>
                Approve
              </Button>
            )}
            <Button size="sm" variant="outline" onClick={() => {
              const stage = window.prompt("Reset from which stage?", "detect")
              if (stage) api.workspaces.reset(ws.slug, { from: stage })
            }}>
              Reset…
            </Button>
            <Button size="sm" variant="destructive" onClick={() => {
              if (window.confirm(`Delete workspace ${ws.slug}?`)) {
                api.workspaces.clean(ws.slug)
              }
            }}>
              Clean
            </Button>
            <Button size="sm" variant="ghost" asChild>
              <a href={previewHref} target="_blank" rel="noreferrer">
                Open preview ↗
              </a>
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
