import { useCallback, useEffect, useMemo, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { initBootstrap, isTauriRuntime, pickFolderNative } from "@/lib/bootstrap"
import { useSSE, type DashboardEvent } from "@/lib/sse"
import {
  Settings,
  X,
  FolderDown,
  FolderOpen,
  Play,
  Radar,
  Square,
  Stethoscope,
  Download,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { WorkspaceRow } from "./views/WorkspaceRow"
import { FilterBar } from "./views/FilterBar"
import { matchesFilter, sortWorkspaces, type FilterState } from "./lib/workspaceFilter"
import { BatchActionBar } from "./views/BatchActionBar"
import { LogDrawer } from "./views/LogDrawer"
import { ConfigDrawer } from "./views/ConfigDrawer"
import { CutsDrawer } from "./views/CutsDrawer"
import { CollectDialog } from "./views/CollectDialog"
import { DoctorDialog } from "./views/DoctorDialog"
import { Logo } from "./components/Logo"

const VIDEO_EXT_RE = /\.(mp4|mkv|mov|avi|m4v|webm)$/i

function folderBasename(p: string): string {
  const parts = p.replace(/[/\\]+$/, "").split(/[/\\]/)
  return parts[parts.length - 1] || p
}

export default function App() {
  const [ready, setReady] = useState(false)
  const [workingRoot, setWorkingRoot] = useState<string | null>(null)
  const [recentFolders, setRecentFolders] = useState<string[]>([])
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])
  const [expanded, setExpanded] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [lastSelectedSlug, setLastSelectedSlug] = useState<string | null>(null)
  const [filter, setFilter] = useState<FilterState>({ status: "any", stage: "any", slug: "", sort: "name" })
  const [logOpen, setLogOpen] = useState<{ slug: string; stage: string } | null>(null)
  const [configOpen, setConfigOpen] = useState(false)
  const [configRevision, setConfigRevision] = useState(0)
  const [cutsSlug, setCutsSlug] = useState<string | null>(null)
  const [externalCutsEvent, setExternalCutsEvent] = useState<{ slug: string; hash: string } | null>(null)
  const [previewSlug, setPreviewSlug] = useState<string | null>(null)
  const [collectOpen, setCollectOpen] = useState(false)
  const [doctorOpen, setDoctorOpen] = useState(false)
  const [scanning, setScanning] = useState(false)
  const [dragActive, setDragActive] = useState(false)
  const [updateAvailable, setUpdateAvailable] = useState<string | null>(null)
  const [updating, setUpdating] = useState(false)
  const [banner, setBanner] = useState<string | null>(null)
  const [listCol, setListCol] = useState<HTMLDivElement | null>(null)

  const refetch = useCallback(() => {
    api.workspaces.list()
      .then((d) => setWorkspaces(d.workspaces))
      .catch(() => { /* wait for project open */ })
  }, [])

  const loadProjectState = useCallback(async () => {
    const info = await api.project.recent()
    setWorkingRoot(info.working_root)
    setRecentFolders(info.recent)
    if (info.working_root) {
      const list = await api.workspaces.list()
      setWorkspaces(list.workspaces)
    } else {
      setWorkspaces([])
    }
  }, [])

  // Bootstrap server URL + session token once via Tauri IPC (or Vite dev env).
  useEffect(() => {
    let active = true
    initBootstrap()
      .then(async () => {
        if (!active) return
        setReady(true)
        await loadProjectState()
        if (isTauriRuntime()) {
          try {
            const { invoke } = await import("@tauri-apps/api/core")
            const ver = await invoke<string | null>("check_update_available")
            if (active && ver) setUpdateAvailable(ver)
          } catch {
            // Updater check is best-effort and stays silent offline.
          }
        }
      })
      .catch((e: unknown) => {
        if (!active) return
        setBanner(e instanceof Error ? e.message : String(e))
        setReady(true)
      })
    return () => { active = false }
  }, [loadProjectState])

  // Tauri native drag-and-drop for folders and single video files.
  useEffect(() => {
    if (!ready || !isTauriRuntime()) return
    let unlisten: (() => void) | undefined
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview")
        unlisten = await getCurrentWebview().onDragDropEvent(async (ev) => {
          if (ev.payload.type === "over") {
            setDragActive(true)
          } else if (ev.payload.type === "leave") {
            setDragActive(false)
          } else if (ev.payload.type === "drop") {
            setDragActive(false)
            const dropped = ev.payload.paths?.[0]
            if (!dropped) return
            setPreviewSlug(null)
            try {
              if (VIDEO_EXT_RE.test(dropped)) {
                const res = await api.project.adoptFile(dropped)
                setWorkingRoot(res.working_root)
                setRecentFolders(res.recent)
                refetch()
                setBanner(`Adopted ${res.slug}`)
              } else {
                const res = await api.project.open(dropped)
                setWorkingRoot(res.working_root)
                setRecentFolders(res.recent)
                const scan = await api.scan({})
                refetch()
                setBanner(`Opened folder (${scan.adopted.length} newly adopted)`)
              }
            } catch (e: unknown) {
              setBanner(e instanceof Error ? e.message : String(e))
            }
          }
        })
      } catch {
        // Non-Tauri browser environment
      }
    })()
    return () => { if (unlisten) unlisten() }
  }, [ready, refetch])

  const handleSse = useCallback((ev: DashboardEvent) => {
    if (
      ev.type === "workspace_updated"
      || ev.type === "workspaces_added"
      || ev.type === "workspaces_removed"
      || ev.type === "run_started"
      || ev.type === "run_finished"
      || ev.type === "lock_changed"
      || ev.type === "sync_lagged"
    ) {
      refetch()
    } else if (ev.type === "project_changed") {
      setWorkingRoot(ev.working_root)
      setPreviewSlug(null)
      setCutsSlug(null)
      refetch()
    } else if (ev.type === "cuts_changed") {
      setExternalCutsEvent({ slug: ev.slug, hash: ev.hash })
    } else if (ev.type === "config_changed") {
      setConfigRevision((n) => n + 1)
    }
  }, [refetch])

  useSSE(handleSse, ready)

  const handleOpenFolder = async (folderPath?: string) => {
    let target: string | null = folderPath ?? null
    if (!target) {
      target = await pickFolderNative()
      if (!target && !isTauriRuntime()) {
        target = window.prompt("Enter working folder path:", workingRoot ?? "")
      }
    }
    if (!target) return
    setPreviewSlug(null)
    setCutsSlug(null)
    setSelected(new Set())
    setLastSelectedSlug(null)
    try {
      setBanner(null)
      const res = await api.project.open(target)
      setWorkingRoot(res.working_root)
      setRecentFolders(res.recent)
      refetch()
    } catch (e: unknown) {
      setBanner(e instanceof Error ? e.message : String(e))
    }
  }

  const handleScan = async () => {
    if (!workingRoot) return
    setScanning(true)
    setBanner(null)
    try {
      const res = await api.scan({})
      refetch()
      setBanner(
        res.adopted.length > 0
          ? `Adopted ${res.adopted.length} new clip${res.adopted.length === 1 ? "" : "s"} (${res.total_workspaces} total)`
          : `Scan complete — no new clips (${res.total_workspaces} total)`,
      )
    } catch (e: unknown) {
      setBanner(e instanceof Error ? e.message : String(e))
    } finally {
      setScanning(false)
    }
  }

  const handleCancelJobs = async () => {
    try {
      const res = await api.jobs.cancel()
      setBanner(`Cancelled ${res.cancelled} active job${res.cancelled === 1 ? "" : "s"}`)
      refetch()
    } catch (e: unknown) {
      setBanner(e instanceof Error ? e.message : String(e))
    }
  }

  const handleInstallUpdate = async () => {
    if (!isTauriRuntime()) return
    setUpdating(true)
    try {
      const { invoke } = await import("@tauri-apps/api/core")
      await invoke("check_and_install_update")
    } catch (e: unknown) {
      setBanner(e instanceof Error ? e.message : String(e))
      setUpdating(false)
    }
  }

  // Release Windows file handles on preview/<slug>/source-proxy.mp4 before clean/reset.
  const handleBeforeMutateWorkspace = useCallback((slugs: string[]) => {
    if (previewSlug && slugs.includes(previewSlug)) {
      setPreviewSlug(null)
    }
  }, [previewSlug])

  const visible = sortWorkspaces(
    workspaces.filter((w) => matchesFilter(w, filter)),
    filter.sort,
  )

  const pendingRunnableSlugs = workspaces
    .filter(
      (w) =>
        !w.ignored
        && w.locked_by === null
        && !w.all_done
        && !Object.values(w.stages).includes("running")
        && !Object.values(w.stages).includes("awaiting_user"),
    )
    .map((w) => w.slug)

  const toggleExpand = (slug: string) => setExpanded((e) => (e === slug ? null : slug))
  const toggleSelect = (slug: string, on: boolean, shiftKey = false) => {
    if (shiftKey && lastSelectedSlug) {
      const anchorIdx = visible.findIndex((w) => w.slug === lastSelectedSlug)
      const targetIdx = visible.findIndex((w) => w.slug === slug)
      if (anchorIdx !== -1 && targetIdx !== -1) {
        const [start, end] = anchorIdx < targetIdx ? [anchorIdx, targetIdx] : [targetIdx, anchorIdx]
        setSelected((s) => {
          const next = new Set(s)
          for (let i = start; i <= end; i++) {
            if (on) next.add(visible[i].slug); else next.delete(visible[i].slug)
          }
          return next
        })
        setLastSelectedSlug(slug)
        return
      }
    }
    setSelected((s) => {
      const next = new Set(s)
      if (on) next.add(slug); else next.delete(slug)
      return next
    })
    setLastSelectedSlug(slug)
  }

  const anyRunning = useMemo(
    () => workspaces.some((w) => Object.values(w.stages).includes("running") || w.locked_by?.actor === "desktop"),
    [workspaces],
  )

  const lockedSlugs = useMemo(
    () => new Set(workspaces.filter((w) => w.locked_by !== null).map((w) => w.slug)),
    [workspaces],
  )

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

  if (!ready) {
    return (
      <div className="h-screen flex items-center justify-center text-sm text-muted-foreground">
        Starting vsplit…
      </div>
    )
  }

  return (
    <div className="h-screen flex flex-col relative">
      {dragActive && (
        <div className="fixed inset-0 z-50 bg-background/85 backdrop-blur-sm flex items-center justify-center border-2 border-dashed border-primary m-4 rounded-xl pointer-events-none">
          <div className="text-center space-y-1">
            <div className="text-lg font-semibold">Drop folder or video clip</div>
            <div className="text-xs text-muted-foreground">
              Folders open &amp; scan · Video files adopt directly
            </div>
          </div>
        </div>
      )}

      <BatchActionBar
        selected={selected}
        lockedSlugs={lockedSlugs}
        onBeforeMutateWorkspaces={handleBeforeMutateWorkspace}
        onCleared={() => { setSelected(new Set()); refetch() }}
      />

      <header className="sticky top-0 z-20 backdrop-blur-md bg-background/80 border-b border-border px-4 py-3 flex items-center gap-2.5 flex-shrink-0 flex-wrap">
        <Logo className="size-7 rounded-md shadow-lg shadow-primary/20 shrink-0" />
        <h1 className="text-base font-semibold tracking-tight mr-1">vsplit</h1>

        <Button
          variant="outline"
          size="sm"
          onClick={() => void handleOpenFolder()}
          title={workingRoot ?? "Open a folder containing video clips"}
        >
          <FolderOpen className="h-3.5 w-3.5 mr-1.5" />
          {workingRoot ? folderBasename(workingRoot) : "Open folder…"}
        </Button>

        {recentFolders.length > 1 && (
          <Select value={workingRoot ?? ""} onValueChange={(v) => void handleOpenFolder(v)}>
            <SelectTrigger className="h-8 w-40 text-xs">
              <SelectValue placeholder="Recent…" />
            </SelectTrigger>
            <SelectContent>
              {recentFolders.map((rf) => (
                <SelectItem key={rf} value={rf} title={rf} className="text-xs">
                  {folderBasename(rf)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        {workingRoot && (
          <div className="inline-flex items-center gap-1.5 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-2.5 py-0.5 text-xs font-medium">
            {visible.length === workspaces.length ? (
              <>
                <span className="tabular-nums text-emerald-300">{workspaces.length}</span>
                <span className="text-emerald-500/80">
                  video{workspaces.length === 1 ? "" : "s"}
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
        )}

        <div className="ml-auto flex items-center gap-1.5">
          {updateAvailable && (
            <Button
              size="sm"
              className="bg-emerald-600 hover:bg-emerald-500 text-white"
              disabled={updating || anyRunning}
              onClick={() => void handleInstallUpdate()}
              title={anyRunning ? "Wait for active jobs to finish before updating" : `Install v${updateAvailable}`}
            >
              <Download className="h-3.5 w-3.5 mr-1.5" />
              {updating ? "Updating…" : `Update v${updateAvailable}`}
            </Button>
          )}
          {anyRunning && (
            <Button
              variant="destructive"
              size="sm"
              onClick={() => void handleCancelJobs()}
              title="Cancel active jobs and terminate child ffmpeg processes"
            >
              <Square className="h-3.5 w-3.5 mr-1.5" />
              Cancel
            </Button>
          )}
          {workingRoot && (
            <>
              <Button
                variant="secondary"
                size="sm"
                disabled={scanning}
                onClick={() => void handleScan()}
                title="Scan working folder for new or renamed video clips"
              >
                <Radar className={`h-3.5 w-3.5 mr-1.5 ${scanning ? "animate-spin" : ""}`} />
                {scanning ? "Scanning…" : "Scan"}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setCollectOpen(true)}
                title="Preview and confirm before collecting outputs"
              >
                <FolderDown className="h-4 w-4 mr-1.5" />
                Collect…
              </Button>
            </>
          )}
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setDoctorOpen(true)}
            title="System diagnostics (Doctor)"
          >
            <Stethoscope className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setConfigOpen(true)}
            title="Configuration"
          >
            <Settings className="h-4 w-4" />
          </Button>
        </div>
      </header>

      {banner && (
        <div className="px-6 py-2 bg-muted/60 border-b border-border text-xs flex items-center justify-between">
          <span>{banner}</span>
          <button onClick={() => setBanner(null)} className="text-muted-foreground hover:text-foreground">
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      <div className="flex-1 min-h-0 flex">
        <div
          ref={setListCol}
          className={
            "relative overflow-hidden " +
            (previewSlug ? "w-[55%] min-w-[480px] border-r border-border" : "w-full")
          }
        >
          <div className="absolute inset-0 overflow-y-auto">
            <main className="max-w-6xl mx-auto p-6">
              {!workingRoot ? (
                <div className="py-16 text-center space-y-4">
                  <div className="text-base font-medium">Choose a video folder to get started</div>
                  <p className="text-sm text-muted-foreground">
                    Pick a folder, drop a folder or video clip onto this window, or select a recent folder.
                  </p>
                  <div className="flex justify-center gap-2">
                    <Button onClick={() => void handleOpenFolder()}>
                      <FolderOpen className="h-4 w-4 mr-2" />
                      Open folder…
                    </Button>
                    <Button variant="outline" onClick={() => setDoctorOpen(true)}>
                      <Stethoscope className="h-4 w-4 mr-2" />
                      Run Doctor
                    </Button>
                  </div>
                </div>
              ) : (
                <>
                  <FilterBar value={filter} onChange={setFilter} workspaces={workspaces} />
                  {workspaces.length === 0 ? (
                    <div className="text-muted-foreground text-sm py-12 text-center space-y-3">
                      <div>No videos scanned in this folder yet.</div>
                      <Button size="sm" disabled={scanning} onClick={() => void handleScan()}>
                        <Radar className="h-3.5 w-3.5 mr-1.5" />
                        {scanning ? "Scanning…" : "Scan folder for videos"}
                      </Button>
                    </div>
                  ) : visible.length === 0 ? (
                    <div className="text-muted-foreground text-sm py-12 text-center">
                      No videos match the current filter.
                    </div>
                  ) : (
                    <div className="space-y-2">
                      <div className="flex items-center justify-between gap-2 px-4 py-1">
                        <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none w-fit">
                          <Checkbox
                            checked={selectAllState}
                            onCheckedChange={toggleSelectAll}
                            aria-label="Select all visible videos"
                          />
                          <span>
                            {visibleSelectedCount === 0
                              ? `Select all (${visible.length})`
                              : visibleSelectedCount === visible.length
                              ? `Deselect all (${visible.length})`
                              : `${visibleSelectedCount} of ${visible.length} selected`}
                          </span>
                        </label>
                        {pendingRunnableSlugs.length > 0 && (
                          <Button
                            size="sm"
                            variant="secondary"
                            className="h-7 px-2.5 text-xs"
                            disabled={anyRunning}
                            onClick={() =>
                              void api.batch
                                .run({ slugs: pendingRunnableSlugs })
                                .then(refetch)
                                .catch((e: unknown) => setBanner(e instanceof Error ? e.message : String(e)))
                            }
                            title="Run all pending or failed videos up to review/completion"
                          >
                            <Play className="h-3 w-3 mr-1.5" />
                            Run pending ({pendingRunnableSlugs.length})
                          </Button>
                        )}
                      </div>
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
                          onBeforeMutateWorkspace={handleBeforeMutateWorkspace}
                          previewSlug={previewSlug}
                        />
                      ))}
                    </div>
                  )}
                </>
              )}
            </main>
          </div>
          {cutsSlug && (
            <div className="absolute inset-0 z-40 bg-black/70" aria-hidden />
          )}
        </div>

        {previewSlug && (
          <div className="flex-1 min-w-0 flex flex-col bg-background pointer-events-auto">
            <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border flex-shrink-0">
              <div className="text-sm font-medium truncate flex-1">{previewSlug}</div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => api.workspaces.openPreview(previewSlug)
                  .catch((e: unknown) => alert(`Failed to open external player: ${e instanceof Error ? e.message : e}`))}
                title="Open video in external video player (configure in Settings → Desktop & Tools)"
              >
                Open externally ↗
              </Button>
              <Button variant="ghost" size="icon" onClick={() => setPreviewSlug(null)} title="Close preview">
                <X className="h-4 w-4" />
              </Button>
            </div>
            <iframe
              key={previewSlug}
              src={api.workspaces.previewUrl(previewSlug)}
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
        externalCutsEvent={externalCutsEvent}
        onClose={() => { setCutsSlug(null); refetch() }}
        container={listCol}
      />
      <ConfigDrawer
        open={configOpen}
        onOpenChange={setConfigOpen}
        workingRoot={workingRoot}
        configRevision={configRevision}
        recentFolders={recentFolders}
        onRecentFoldersChange={setRecentFolders}
      />
      <CollectDialog open={collectOpen} onOpenChange={setCollectOpen} />
      <DoctorDialog open={doctorOpen} onOpenChange={setDoctorOpen} />
    </div>
  )
}
