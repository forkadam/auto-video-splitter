import { ArrowUpDown, RotateCcw, Search, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"
import type { Workspace } from "@/lib/api"
import { matchesStatusBucket, type FilterState, type SortKey } from "@/lib/workspaceFilter"
import { cn } from "@/lib/utils"

const STAGES = ["any", "probe", "detect", "preview", "review", "plan", "split", "verify"] as const

type StatusBucket = {
  key: string
  label: string
  dot: string
  activeCls: string
}

const BUCKETS: StatusBucket[] = [
  { key: "any", label: "All", dot: "bg-foreground/60", activeCls: "bg-accent text-foreground border-foreground/30" },
  { key: "failed", label: "Failed", dot: "bg-red-500", activeCls: "bg-red-500/20 text-red-200 border-red-500/50" },
  { key: "running", label: "Running", dot: "bg-blue-500", activeCls: "bg-blue-500/20 text-blue-200 border-blue-500/50" },
  { key: "awaiting_user", label: "Needs review", dot: "bg-amber-500", activeCls: "bg-amber-500/20 text-amber-200 border-amber-500/50" },
  { key: "no_scenes", label: "No scenes", dot: "bg-orange-500", activeCls: "bg-orange-500/20 text-orange-200 border-orange-500/50" },
  { key: "pending", label: "Pending", dot: "bg-zinc-400", activeCls: "bg-zinc-500/25 text-zinc-100 border-zinc-400/50" },
  { key: "done", label: "Done", dot: "bg-emerald-500", activeCls: "bg-emerald-500/20 text-emerald-200 border-emerald-500/50" },
  { key: "ignored", label: "Ignored", dot: "bg-zinc-600", activeCls: "bg-zinc-600/30 text-zinc-200 border-zinc-500/50" },
]

export function FilterBar({
  value,
  onChange,
  workspaces,
}: {
  value: FilterState
  onChange: (v: FilterState) => void
  workspaces: Workspace[]
}) {
  const counts: Record<string, number> = {}
  for (const b of BUCKETS) {
    counts[b.key] = b.key === "any"
      ? workspaces.length
      : workspaces.reduce((n, w) => n + (matchesStatusBucket(w, b.key) ? 1 : 0), 0)
  }

  const hasActiveFilter = value.status !== "any" || value.stage !== "any" || value.slug.trim() !== ""

  return (
    <div className="space-y-2.5 mb-4">
      {/* Clickable status summary strip */}
      <div className="flex items-center gap-1.5 flex-wrap">
        {BUCKETS.map((b) => {
          const count = counts[b.key] ?? 0
          const active = value.status === b.key
          if (b.key !== "any" && count === 0 && !active) return null
          return (
            <button
              key={b.key}
              type="button"
              onClick={() => onChange({ ...value, status: active && b.key !== "any" ? "any" : b.key })}
              className={cn(
                "inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs border transition-colors cursor-pointer select-none",
                active
                  ? b.activeCls
                  : "border-border/80 bg-card/50 text-muted-foreground hover:text-foreground hover:bg-accent/50",
              )}
            >
              <span className={cn("w-1.5 h-1.5 rounded-full shrink-0", b.dot)} />
              <span>{b.label}</span>
              <span className="tabular-nums opacity-75 font-mono text-[11px]">{count}</span>
            </button>
          )
        })}
      </div>

      {/* Search, stage filter, and sort controls */}
      <div className="flex gap-2 items-center flex-wrap">
        <div className="relative flex-1 min-w-[200px] max-w-sm">
          <Search className="h-3.5 w-3.5 text-muted-foreground absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
          <Input
            placeholder="Filter videos… (supports * glob)"
            value={value.slug}
            onChange={(e) => onChange({ ...value, slug: e.target.value })}
            className="h-8 pl-8 pr-7 text-xs"
          />
          {value.slug && (
            <button
              type="button"
              onClick={() => onChange({ ...value, slug: "" })}
              aria-label="Clear search"
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        <Select value={value.stage} onValueChange={(v) => onChange({ ...value, stage: v })}>
          <SelectTrigger className="w-36 h-8 text-xs">
            <span className="text-muted-foreground mr-1">next:</span>
            <SelectValue placeholder="stage" />
          </SelectTrigger>
          <SelectContent>
            {STAGES.map((s) => (
              <SelectItem key={s} value={s} className="text-xs">
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select value={value.sort} onValueChange={(v) => onChange({ ...value, sort: v as SortKey })}>
          <SelectTrigger className="w-40 h-8 text-xs">
            <ArrowUpDown className="h-3 w-3 text-muted-foreground mr-1 shrink-0" />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="name" className="text-xs">Name (A–Z)</SelectItem>
            <SelectItem value="status" className="text-xs">Status priority</SelectItem>
            <SelectItem value="duration_desc" className="text-xs">Longest first</SelectItem>
            <SelectItem value="duration_asc" className="text-xs">Shortest first</SelectItem>
          </SelectContent>
        </Select>

        {hasActiveFilter && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-8 px-2 text-xs text-muted-foreground hover:text-foreground"
            onClick={() => onChange({ ...value, status: "any", stage: "any", slug: "" })}
          >
            <RotateCcw className="h-3 w-3 mr-1" />
            Clear
          </Button>
        )}
      </div>
    </div>
  )
}
