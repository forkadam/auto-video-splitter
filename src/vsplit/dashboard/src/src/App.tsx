import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings } from "lucide-react"
import { Button } from "@/components/ui/button"

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])

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
          <div className="text-zinc-500">[ workspace list goes here ]</div>
        )}
      </main>
    </div>
  )
}
