import { useEffect, useState } from "react"
import { ChevronRight, ChevronDown, Loader2 } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  HoverCard, HoverCardContent, HoverCardTrigger,
} from "@/components/ui/hover-card"
import { api, type Workspace } from "@/lib/api"
import { nextAction } from "@/lib/nextAction"

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
  onOpenLog: (slug: string, stage: string) => void
  onOpenCuts: (slug: string) => void
}

type Detail = {
  workspace_path?: string
  thumb_names?: string[]
  stages?: Record<string, string>
  manifest?: {
    stages?: Record<string, { artifacts?: Record<string, unknown> }>
  }
}

function NextPill({ ws, onOpenCuts }: { ws: Workspace; onOpenCuts: (s: string) => void }) {
  const a = nextAction(ws)
  const base = "px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5"
  if (a.kind === "done") {
    return <span className={cn(base, "bg-emerald-500/15 text-emerald-300")}>{a.label}</span>
  }
  if (a.kind === "running") {
    return (
      <span className={cn(base, "bg-blue-500/15 text-blue-300")}>
        <Loader2 className="h-3 w-3 animate-spin" />
        {a.label}
      </span>
    )
  }
  if (a.kind === "idle") {
    return <span className={cn(base, "bg-muted text-muted-foreground")}>{a.label}</span>
  }
  const onClick = () => {
    if (a.kind === "retry") return void api.workspaces.run(ws.slug, { only: a.stage })
    if (a.kind === "review") return onOpenCuts(ws.slug)
    if (a.kind === "run") return void api.workspaces.run(ws.slug, {})
  }
  const tone =
    a.kind === "retry" ? "bg-red-500/15 text-red-300 hover:bg-red-500/25" :
    a.kind === "review" ? "bg-amber-500/15 text-amber-300 hover:bg-amber-500/25" :
    "bg-primary/15 text-primary hover:bg-primary/25"
  return (
    <button type="button" onClick={onClick}
            className={cn(base, "cursor-pointer transition-colors", tone)}>
      {a.label}
    </button>
  )
}

export function WorkspaceRow({
  ws, selected, onSelectChange, expanded, onToggle, onOpenLog, onOpenCuts,
}: Props) {
  const [detail, setDetail] = useState<Detail | null>(null)

  useEffect(() => {
    if (!expanded) return
    api.workspaces.detail(ws.slug).then((d) => setDetail(d as unknown as Detail))
  }, [expanded, ws.slug])

  const thumbNames = detail?.thumb_names ?? []
  const previewReady = ws.stages.preview === "done"

  return (
    <div className={cn(
      "rounded-lg border bg-card text-card-foreground transition-colors",
      "hover:bg-accent/40",
      selected && "ring-2 ring-primary/60 border-primary/40",
    )}>
      <div className="w-full px-4 py-2.5 flex items-center gap-3">
        <button onClick={() => onToggle(ws.slug)} className="flex items-center gap-3 min-w-0 flex-1 text-left">
          {expanded ? <ChevronDown className="h-4 w-4 text-muted-foreground" /> : <ChevronRight className="h-4 w-4 text-muted-foreground" />}
          {ws.first_thumb ? (
            <img
              src={api.workspaces.thumbUrl(ws.slug, ws.first_thumb)}
              alt=""
              className="w-14 h-8 rounded-md object-cover bg-muted ring-1 ring-border"
              loading="lazy"
            />
          ) : (
            <div className="w-14 h-8 rounded-md bg-muted ring-1 ring-border" />
          )}
          <div className="font-medium text-sm min-w-0 truncate">{ws.slug}</div>
        </button>
        <NextPill ws={ws} onOpenCuts={onOpenCuts} />
        <Checkbox
          checked={selected}
          onCheckedChange={(c) => onSelectChange(ws.slug, !!c)}
        />
        <div className="flex gap-0.5">
          {STAGES.map((s) => {
            const status = ws.stages[s] ?? "pending"
            const isFailed = status === "failed"
            return (
              <button
                key={s}
                type="button"
                title={`${s}: ${status}${isFailed ? " (click to view log)" : ""}`}
                onClick={isFailed ? () => onOpenLog(ws.slug, s) : undefined}
                disabled={!isFailed}
                className={cn(
                  "w-2.5 h-2.5 rounded-sm",
                  STATUS_COLOR[status],
                  isFailed && "cursor-pointer hover:ring-1 hover:ring-red-300",
                  !isFailed && "cursor-default",
                )}
              />
            )
          })}
        </div>
      </div>

      {expanded && (
        <div className="px-4 pb-3.5 pt-2 border-t border-border space-y-3">
          {thumbNames.length > 0 && (
            <div className="flex gap-1.5 overflow-x-auto pb-1">
              {thumbNames.map((name) => (
                <HoverCard key={name} openDelay={100} closeDelay={0}>
                  <HoverCardTrigger asChild>
                    <img
                      src={api.workspaces.thumbUrl(ws.slug, name)}
                      alt={name}
                      className="h-16 rounded-md object-cover bg-muted ring-1 ring-border flex-shrink-0 cursor-zoom-in transition-transform hover:scale-[1.02]"
                      loading="lazy"
                    />
                  </HoverCardTrigger>
                  <HoverCardContent
                    side="top"
                    align="center"
                    className="w-auto p-1.5 shadow-2xl"
                  >
                    <img
                      src={api.workspaces.thumbUrl(ws.slug, name)}
                      alt={name}
                      className="max-w-[480px] max-h-[360px] rounded"
                    />
                    <div className="px-2 py-1 text-xs text-muted-foreground font-mono">
                      {name}
                    </div>
                  </HoverCardContent>
                </HoverCard>
              ))}
            </div>
          )}
          <div className="flex gap-2 flex-wrap">
            <Button size="sm" onClick={() => api.workspaces.run(ws.slug, {})}>
              Run
            </Button>
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
            <Button
              size="sm"
              variant="ghost"
              disabled={!previewReady}
              onClick={() => api.workspaces.openPreview(ws.slug)
                .catch((e) => alert(`Failed to open preview: ${e.message ?? e}`))}
            >
              Open preview ↗
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
