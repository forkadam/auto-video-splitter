import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings, X, FolderDown } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { WorkspaceRow } from "./views/WorkspaceRow"
import { FilterBar, type FilterState } from "./views/FilterBar"
import { BatchActionBar } from "./views/BatchActionBar"
import { LogDrawer } from "./views/LogDrawer"
import { ConfigDrawer } from "./views/ConfigDrawer"
import { CutsDrawer } from "./views/CutsDrawer"
import { CollectDialog } from "./views/CollectDialog"
import { Logo } from "./components/Logo"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"]

function matchesFilter(ws: Workspace, f: FilterState): boolean {
  if (f.status !== "any") {
    // "done" means the workspace is fully complete — every stage. Using the
    // any-stage-matches rule here would match almost every workspace because
    // probe=done lands within seconds.
    if (f.status === "done") {
      if (!ws.all_done) return false
    } else if (f.status === "no_scenes") {
      // Synthetic status: the detector reported 0 scenes. Matches workspaces
      // wearing the orange "No scenes" NextPill, and also done/skipped
      // zero-scene workspaces in case the user is auditing them after the fact.
      if (ws.scenes_found !== 0) return false
    } else if (f.status === "awaiting_user") {
      // Mutually exclusive with "no_scenes" — the orange "No scenes" pill is
      // technically also a review/awaiting state on disk, but UX-wise it's a
      // different category so we strip it from this bucket.
      if (ws.scenes_found === 0) return false
      if (!Object.values(ws.stages).includes("awaiting_user")) return false
    } else if (!Object.values(ws.stages).includes(f.status)) {
      return false
    }
  }
  if (f.stage !== "any") {
    // A "skipped" stage was intentionally bypassed; treat it as complete when
    // computing the workspace's current stage, otherwise the filter would
    // route every workspace with a skipped stage back to it. Matches the
    // logic in nextAction.ts so this filter agrees with the NextPill.
    const next = STAGES.find(
      (s) => ws.stages[s] !== "done" && ws.stages[s] !== "skipped",
    )
    if (next !== f.stage) return false
  }
  if (f.slug) {
    const pattern = new RegExp("^" + f.slug.replace(/\*/g, ".*").replace(/\?/g, ".") + "$")
    if (!pattern.test(ws.slug)) return false
  }
  return true
}

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])
  const [expanded, setExpanded] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [filter, setFilter] = useState<FilterState>({ status: "any", stage: "any", slug: "" })
  const [logOpen, setLogOpen] = useState<{ slug: string; stage: string } | null>(null)
  const [configOpen, setConfigOpen] = useState(false)
  const [cutsSlug, setCutsSlug] = useState<string | null>(null)
  const [previewSlug, setPreviewSlug] = useState<string | null>(null)
  const [collectOpen, setCollectOpen] = useState(false)
  // Use a callback ref through state so the drawer re-renders once the column mounts.
  const [listCol, setListCol] = useState<HTMLDivElement | null>(null)

  const refetch = () => {
    api.workspaces.list().then((d) => setWorkspaces(d.workspaces))
  }
  useEffect(refetch, [])

  useSSE((ev) => {
    if (ev.type === "workspace_updated" || ev.type === "workspaces_added"
      || ev.type === "workspaces_removed" || ev.type === "run_finished") {
      refetch()
    }
  })

  const toggleExpand = (slug: string) => setExpanded((e) => e === slug ? null : slug)
  const toggleSelect = (slug: string, on: boolean) => setSelected((s) => {
    const next = new Set(s)
    if (on) next.add(slug); else next.delete(slug)
    return next
  })

  const visible = workspaces.filter((w) => matchesFilter(w, filter))
  const visibleSelectedCount = visible.reduce((n, w) => n + (selected.has(w.slug) ? 1 : 0), 0)
  const selectAllState: boolean | "indeterminate" =
    visible.length > 0 && visibleSelectedCount === visible.length ? true
    : visibleSelectedCount === 0 ? false
    : "indeterminate"
  const toggleSelectAll = () => setSelected((s) => {
    const next = new Set(s)
    if (visibleSelectedCount === visible.length) {
      for (const w of visible) next.delete(w.slug)
    } else {
      for (const w of visible) next.add(w.slug)
    }
    return next
  })

  return (
    <div className="h-screen flex flex-col">
      <BatchActionBar
        selected={selected}
        onCleared={() => { setSelected(new Set()); refetch() }}
      />
      <header className="sticky top-0 z-20 backdrop-blur-md bg-background/80 border-b border-border px-6 py-3.5 flex items-center gap-4 flex-shrink-0">
        <Logo className="size-7 rounded-md shadow-lg shadow-primary/20" />
        <h1 className="text-base font-semibold tracking-tight">vsplit dashboard</h1>
        <div className="inline-flex items-center gap-1.5 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-0.5 text-xs font-medium">
          {visible.length === workspaces.length ? (
            <>
              <span className="tabular-nums text-emerald-300">{workspaces.length}</span>
              <span className="text-emerald-500/80">
                workspace{workspaces.length === 1 ? "" : "s"}
              </span>
            </>
          ) : (
            <>
              <span className="tabular-nums text-emerald-300">{visible.length}</span>
              <span className="text-emerald-500/80">of</span>
              <span className="tabular-nums text-emerald-300">{workspaces.length}</span>
              <span className="text-emerald-500/80">shown</span>
            </>
          )}
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto"
          onClick={() => setCollectOpen(true)}
          title="Preview and confirm before collecting outputs"
        >
          <FolderDown className="h-4 w-4 mr-1.5" />
          Collect outputs…
        </Button>
        <Button variant="ghost" size="icon" onClick={() => setConfigOpen(true)}>
          <Settings className="h-4 w-4" />
        </Button>
      </header>
      <div className="flex-1 min-h-0 flex">
        {/* Left: workspace list column. Acts as the drawer container so CutsDrawer
            never overlaps the preview pane. */}
        <div
          ref={setListCol}
          className={
            "relative overflow-y-auto " +
            (previewSlug ? "w-[55%] min-w-[480px] border-r border-border" : "w-full")
          }
        >
          <main className="max-w-6xl mx-auto p-6">
            <FilterBar value={filter} onChange={setFilter} />
            {workspaces.length === 0 ? (
              <div className="text-muted-foreground text-sm py-12 text-center">
                No workspaces yet. Run <code className="text-foreground bg-muted px-1.5 py-0.5 rounded">vsplit scan</code> to adopt files.
              </div>
            ) : visible.length === 0 ? (
              <div className="text-muted-foreground text-sm py-12 text-center">No workspaces match the current filter.</div>
            ) : (
              <div className="space-y-2">
                <label className="flex items-center gap-2 px-4 py-1.5 text-xs text-muted-foreground cursor-pointer select-none w-fit">
                  <Checkbox
                    checked={selectAllState}
                    onCheckedChange={toggleSelectAll}
                    aria-label="Select all visible workspaces"
                  />
                  <span>
                    {visibleSelectedCount === 0
                      ? `Select all (${visible.length})`
                      : visibleSelectedCount === visible.length
                      ? `Deselect all (${visible.length})`
                      : `${visibleSelectedCount} of ${visible.length} selected`}
                  </span>
                </label>
                {visible.map((ws) => (
                  <WorkspaceRow
                    key={ws.slug}
                    ws={ws}
                    selected={selected.has(ws.slug)}
                    onSelectChange={toggleSelect}
                    expanded={expanded === ws.slug}
                    onToggle={toggleExpand}
                    onOpenLog={(slug, stage) => setLogOpen({ slug, stage })}
                    onOpenCuts={(slug) => setCutsSlug(slug)}
                    onOpenPreview={(slug) => setPreviewSlug(slug)}
                    previewSlug={previewSlug}
                  />
                ))}
              </div>
            )}
          </main>
        </div>
        {previewSlug && (
          <div className="flex-1 min-w-0 flex flex-col bg-background">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border flex-shrink-0">
              <div className="text-sm font-medium truncate flex-1">{previewSlug}</div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => api.workspaces.openPreview(previewSlug!)
                  .catch((e) => alert(`Failed to open external preview: ${e.message ?? e}`))}
                title="Open in new browser window"
              >
                Open externally ↗
              </Button>
              <Button variant="ghost" size="icon" onClick={() => setPreviewSlug(null)} title="Close preview">
                <X className="h-4 w-4" />
              </Button>
            </div>
            <iframe
              key={previewSlug}
              src={`/api/workspaces/${previewSlug}/preview/index.html`}
              title={`${previewSlug} preview`}
              className="flex-1 w-full border-0 bg-background"
            />
          </div>
        )}
      </div>
      <LogDrawer
        slug={logOpen?.slug ?? null}
        stage={logOpen?.stage ?? null}
        onClose={() => setLogOpen(null)}
      />
      <CutsDrawer
        slug={cutsSlug}
        onClose={() => { setCutsSlug(null); refetch() }}
        container={listCol}
      />
      <ConfigDrawer open={configOpen} onOpenChange={setConfigOpen} />
      <CollectDialog open={collectOpen} onOpenChange={setCollectOpen} />
    </div>
  )
}
