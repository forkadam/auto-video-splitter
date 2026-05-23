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
      <header className="border-b border-zinc-800 px-4 py-3 flex items-center gap-4">
        <h1 className="text-lg font-semibold">vsplit dashboard</h1>
        <div className="text-sm text-zinc-400">
          {visible.length} of {workspaces.length} workspace{workspaces.length === 1 ? "" : "s"}
        </div>
        <Button variant="ghost" size="icon" className="ml-auto" onClick={() => setConfigOpen(true)}>
          <Settings className="h-4 w-4" />
        </Button>
      </header>
      <main className="p-4">
        <FilterBar value={filter} onChange={setFilter} />
        {workspaces.length === 0 ? (
          <div className="text-zinc-500">No workspaces yet. Run <code className="text-zinc-300">vsplit scan</code> to adopt files.</div>
        ) : visible.length === 0 ? (
          <div className="text-zinc-500">No workspaces match the current filter.</div>
        ) : (
          <div className="space-y-1">
            {visible.map((ws) => (
              <WorkspaceRow
                key={ws.slug}
                ws={ws}
                selected={selected.has(ws.slug)}
                onSelectChange={toggleSelect}
                expanded={expanded === ws.slug}
                onToggle={toggleExpand}
                onOpenLog={(slug, stage) => setLogOpen({ slug, stage })}
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
      <ConfigDrawer open={configOpen} onOpenChange={setConfigOpen} />
    </div>
  )
}
