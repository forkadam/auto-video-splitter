import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings } from "lucide-react"
import { Button } from "@/components/ui/button"
import { WorkspaceRow } from "./views/WorkspaceRow"
import { FilterBar, type FilterState } from "./views/FilterBar"
import { BatchActionBar } from "./views/BatchActionBar"
import { LogDrawer } from "./views/LogDrawer"
import { ConfigDrawer } from "./views/ConfigDrawer"
import { CutsDrawer } from "./views/CutsDrawer"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"]

function matchesFilter(ws: Workspace, f: FilterState): boolean {
  if (f.status !== "any") {
    if (!Object.values(ws.stages).includes(f.status)) return false
  }
  if (f.stage !== "any") {
    const next = STAGES.find((s) => ws.stages[s] !== "done")
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

  return (
    <div className="min-h-full">
      <BatchActionBar
        selected={selected}
        onCleared={() => { setSelected(new Set()); refetch() }}
      />
      <header className="sticky top-0 z-20 backdrop-blur-md bg-background/80 border-b border-border px-6 py-3.5 flex items-center gap-4">
        <div className="size-7 rounded-md bg-gradient-to-br from-primary to-primary/40 shadow-lg shadow-primary/20" />
        <h1 className="text-base font-semibold tracking-tight">vsplit dashboard</h1>
        <div className="text-sm text-muted-foreground">
          {visible.length} of {workspaces.length} workspace{workspaces.length === 1 ? "" : "s"}
        </div>
        <Button variant="ghost" size="icon" className="ml-auto" onClick={() => setConfigOpen(true)}>
          <Settings className="h-4 w-4" />
        </Button>
      </header>
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
              />
            ))}
          </div>
        )}
      </main>
      <LogDrawer
        slug={logOpen?.slug ?? null}
        stage={logOpen?.stage ?? null}
        onClose={() => setLogOpen(null)}
      />
      <CutsDrawer slug={cutsSlug} onClose={() => { setCutsSlug(null); refetch() }} />
      <ConfigDrawer open={configOpen} onOpenChange={setConfigOpen} />
    </div>
  )
}
