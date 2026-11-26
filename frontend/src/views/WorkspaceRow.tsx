import { useEffect, useRef, useState } from "react"
import { ChevronRight, ChevronDown, ExternalLink, Loader2, Film, Lock } from "lucide-react"
import { cn, formatDuration } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  HoverCard, HoverCardContent, HoverCardTrigger,
} from "@/components/ui/hover-card"
import { api, type Workspace } from "@/lib/api"
import { nextAction } from "@/lib/nextAction"
import { ResetStageDialog } from "./ResetStageDialog"

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
  onSelectChange: (slug: string, selected: boolean, shiftKey: boolean) => void
  expanded: boolean
  onToggle: (slug: string) => void
  onOpenLog: (slug: string, stage: string) => void
  onOpenCuts: (slug: string) => void
  onOpenPreview: (slug: string) => void
  onBeforeMutateWorkspace: (slugs: string[]) => void
  previewSlug: string | null
}

type Detail = {
  workspace_path?: string
  thumb_names?: string[]
  stages?: Record<string, string>
  manifest?: {
    stages?: Record<string, { artifacts?: Record<string, unknown> }>
    metadata?: {
      duration_seconds?: number | null
      width?: number | null
      height?: number | null
      fps?: number | null
      video_codec?: string | null
    }
  }
}

function NextPill({
  ws, onOpenCuts,
}: {
  ws: Workspace
  onOpenCuts: (s: string) => void
}) {
  if (ws.locked_by && ws.locked_by.actor === "cli") {
    return (
      <span
        className="px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5 bg-violet-500/15 text-violet-300 ring-1 ring-inset ring-violet-500/30"
        title={`Locked by CLI pid ${ws.locked_by.pid}`}
      >
        <Lock className="h-3 w-3 animate-pulse" />
        Running {ws.locked_by.stage} via CLI
      </span>
    )
  }

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
  if (a.kind === "ignored") {
    return <span className={cn(base, "bg-zinc-500/40 text-zinc-100 ring-1 ring-inset ring-zinc-400/50")}>{a.label}</span>
  }
  const onClick = () => {
    if (a.kind === "retry") return void api.workspaces.run(ws.slug, { only: a.stage })
    if (a.kind === "review") return onOpenCuts(ws.slug)
    if (a.kind === "empty") return onOpenCuts(ws.slug)
    if (a.kind === "run") return void api.workspaces.run(ws.slug, {})
  }
  const tone =
    a.kind === "retry" ? "bg-red-500/15 text-red-300 hover:bg-red-500/25" :
    a.kind === "review" ? "bg-amber-500/15 text-amber-300 hover:bg-amber-500/25" :
    a.kind === "empty" ? "bg-orange-500/20 text-orange-300 hover:bg-orange-500/30 ring-1 ring-inset ring-orange-500/30" :
    "bg-primary/15 text-primary hover:bg-primary/25"
  return (
    <button type="button" onClick={onClick}
            title={a.kind === "empty"
              ? "Detector found 0 scenes — click to open the cut dialog: cut the whole clip or enter time ranges manually"
              : undefined}
            className={cn(base, "cursor-pointer transition-colors", tone)}>
      {a.label}
    </button>
  )
}

export function WorkspaceRow({
  ws, selected, onSelectChange, expanded, onToggle, onOpenLog, onOpenCuts,
  onOpenPreview, onBeforeMutateWorkspace, previewSlug,
}: Props) {
  const [detail, setDetail] = useState<Detail | null>(null)
  const [resetOpen, setResetOpen] = useState(false)
  const shiftHeldRef = useRef(false)

  useEffect(() => {
    if (!expanded) return
    api.workspaces.detail(ws.slug).then((d) => setDetail(d as unknown as Detail))
  }, [expanded, ws.slug])

  const thumbNames = detail?.thumb_names ?? []
  const previewReady = ws.stages.preview === "done"
  const isLocked = ws.locked_by !== null

  const md = detail?.manifest?.metadata
  const metaBits = [
    formatDuration(md?.duration_seconds ?? ws.duration_seconds),
    md?.width && md?.height ? `${md.width}×${md.height}` : null,
    md?.fps ? `${Math.round(md.fps)} fps` : null,
    md?.video_codec ?? null,
  ].filter(Boolean) as string[]

  return (
    <div className={cn(
      "rounded-lg border bg-card text-card-foreground transition-colors",
      "hover:bg-accent/40",
      ws.ignored && "bg-zinc-700/40 border-zinc-500 border-l-4 border-l-zinc-300",
      selected && "ring-2 ring-primary/60 border-primary/40",
      previewSlug === ws.slug && "ring-2 ring-blue-400/60 border-blue-400/40",
    )}>
      <div className="w-full px-4 py-2.5 flex items-center gap-3 select-none">
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
          <div className="min-w-0 flex-1">
            <div className="font-medium text-sm truncate">{ws.slug}</div>
            {(formatDuration(ws.duration_seconds) || ws.scenes_found !== null) && (
              <div className="text-xs text-muted-foreground tabular-nums leading-tight flex items-center gap-1.5">
                {formatDuration(ws.duration_seconds) && (
                  <span>{formatDuration(ws.duration_seconds)}</span>
                )}
                {formatDuration(ws.duration_seconds) && ws.scenes_found !== null && (
                  <span className="opacity-50">·</span>
                )}
                {ws.scenes_found !== null && (
                  <span className={ws.scenes_found === 0 ? "text-orange-300/90" : undefined}>
                    {ws.scenes_found} scene{ws.scenes_found === 1 ? "" : "s"}
                  </span>
                )}
              </div>
            )}
          </div>
        </button>
        {ws.stages.split === "done" && ws.output_count > 0 && (
          <span
            className="flex items-center gap-1 px-2 py-1 rounded-md text-xs font-medium bg-muted text-muted-foreground flex-shrink-0"
            title={`${ws.output_count} output clip${ws.output_count === 1 ? "" : "s"} in output/`}
          >
            <Film className="h-3 w-3" />
            {ws.output_count}
          </span>
        )}
        <NextPill ws={ws} onOpenCuts={onOpenCuts} />
        <Checkbox
          checked={selected}
          onPointerDown={(e) => { shiftHeldRef.current = e.shiftKey }}
          onClick={(e) => { shiftHeldRef.current = e.shiftKey }}
          onCheckedChange={(c) => {
            const shift = shiftHeldRef.current
            shiftHeldRef.current = false
            onSelectChange(ws.slug, !!c, shift)
          }}
        />
        <div className="flex gap-0.5">
          {STAGES.map((s) => {
            const status = ws.stages[s] ?? "pending"
            const hasLog = status !== "pending" && status !== "skipped"
            return (
              <button
                key={s}
                type="button"
                title={`${s}: ${status}${hasLog ? " (click to view log)" : ""}`}
                onClick={hasLog ? () => onOpenLog(ws.slug, s) : undefined}
                disabled={!hasLog}
                className={cn(
                  "w-2.5 h-2.5 rounded-sm transition-transform",
                  STATUS_COLOR[status],
                  hasLog && "cursor-pointer hover:scale-125 hover:ring-1 hover:ring-foreground/60",
                  !hasLog && "cursor-default",
                )}
              />
            )
          })}
        </div>
      </div>

      {expanded && (
        <div className="px-4 pb-3.5 pt-2 border-t border-border space-y-3">
          {metaBits.length > 0 && (
            <div className="font-mono text-xs text-muted-foreground">
              {metaBits.join("  ·  ")}
            </div>
          )}
          <div className="flex items-center gap-1.5 flex-wrap">
            {STAGES.map((s) => {
              const status = ws.stages[s] ?? "pending"
              const hasLog = status !== "pending" && status !== "skipped"
              return (
                <button
                  key={s}
                  type="button"
                  disabled={!hasLog}
                  onClick={hasLog ? () => onOpenLog(ws.slug, s) : undefined}
                  title={hasLog ? `View ${s} log (${status})` : `${s}: ${status}`}
                  className={cn(
                    "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-[11px] font-mono border transition-colors",
                    hasLog
                      ? "border-border bg-muted/40 text-foreground/90 hover:bg-accent cursor-pointer"
                      : "border-border/50 bg-transparent text-muted-foreground/60 cursor-default",
                  )}
                >
                  <span className={cn("w-1.5 h-1.5 rounded-full shrink-0", STATUS_COLOR[status])} />
                  <span>{s}</span>
                </button>
              )
            })}
          </div>
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
            <Button size="sm" disabled={isLocked} onClick={() => void api.workspaces.run(ws.slug, {})}>
              Run
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={isLocked}
              title={isLocked ? "Video is currently locked" : undefined}
              onClick={() => setResetOpen(true)}
            >
              Reset…
            </Button>
            {ws.ignored ? (
              <Button size="sm" variant="ghost" disabled={isLocked}
                      onClick={() => { void api.workspaces.unignore(ws.slug) }}>
                Unignore
              </Button>
            ) : (
              <Button size="sm" variant="outline" disabled={isLocked}
                      onClick={() => { void api.workspaces.ignore(ws.slug) }}>
                Ignore
              </Button>
            )}
            <Button
              size="sm"
              variant="destructive"
              disabled={isLocked}
              title={isLocked ? "Video is currently locked" : undefined}
              onClick={() => {
                if (window.confirm(`Clean vsplit data for ${ws.slug}? (Source video file is never deleted.)`)) {
                  onBeforeMutateWorkspace([ws.slug])
                  void api.workspaces.clean(ws.slug)
                }
              }}
            >
              Clean
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!previewReady}
              onClick={() => onOpenPreview(ws.slug)}
            >
              Open preview
            </Button>
            <Button
              size="sm"
              variant="ghost"
              title="Open source video in external video player"
              onClick={() =>
                api.workspaces.openPreview(ws.slug).catch((e: unknown) =>
                  alert(`Failed to open external player: ${e instanceof Error ? e.message : e}`),
                )
              }
            >
              <ExternalLink className="h-3.5 w-3.5 mr-1.5" />
              Play externally
            </Button>
          </div>
        </div>
      )}
      <ResetStageDialog
        open={resetOpen}
        onOpenChange={setResetOpen}
        target={ws.slug}
        onConfirm={(stage) => {
          onBeforeMutateWorkspace([ws.slug])
          void api.workspaces.reset(ws.slug, { from: stage })
        }}
      />
    </div>
  )
}
