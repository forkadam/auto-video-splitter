import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings } from "lucide-react"
import { Button } from "@/components/ui/button"
import { WorkspaceRow } from "./views/WorkspaceRow"

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])
  const [expanded, setExpanded] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())

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

  return (
    <div className="min-h-full">
      <header className="border-b border-zinc-800 px-4 py-3 flex items-center gap-4">
        <h1 className="text-lg font-semibold">vsplit dashboard</h1>
        <div className="text-sm text-zinc-400">{workspaces.length} workspace{workspaces.length === 1 ? "" : "s"}</div>
        <Button variant="ghost" size="icon" className="ml-auto"><Settings className="h-4 w-4" /></Button>
      </header>
      <main className="p-4">
        {workspaces.length === 0 ? (
          <div className="text-zinc-500">No workspaces yet. Run <code className="text-zinc-300">vsplit scan</code> to adopt files.</div>
        ) : (
          <div className="space-y-1">
            {workspaces.map((ws) => (
              <WorkspaceRow
                key={ws.slug}
                ws={ws}
                selected={selected.has(ws.slug)}
                onSelectChange={toggleSelect}
                expanded={expanded === ws.slug}
                onToggle={toggleExpand}
              />
            ))}
          </div>
        )}
      </main>
    </div>
  )
}
