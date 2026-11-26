import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import {
  Eye,
  FileCode2,
  FolderClock,
  FolderSearch,
  Info,
  Monitor,
  Scissors,
  SlidersHorizontal,
  Trash2,
  Workflow,
  X,
} from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { api } from "@/lib/api"
import {
  getAppConfigNative,
  getBootstrap,
  isTauriRuntime,
  pickFileNative,
  setAppConfigNative,
  type AppConfig,
} from "@/lib/bootstrap"
import { cn } from "@/lib/utils"

const DETECTORS = ["content", "adaptive", "histogram", "hash"] as const
const CONTAINERS = ["mp4", "mkv", "mov"] as const
const COLLECT_MODES = ["copy", "move", "link"] as const
const RECENT_LIMITS = [3, 5, 10, 15, 20] as const
const PROXY_HEIGHTS = [
  { label: "480p", value: "480" },
  { label: "720p (default)", value: "720" },
  { label: "1080p", value: "1080" },
  { label: "Original (no resize)", value: "none" },
] as const

type TabId = "detection" | "pipeline" | "yaml" | "desktop"

type DetectorMeta = {
  defaultThreshold: number
  step: string
  range: string
  direction: string
  bestFor: string
  presets: Array<{ label: string; value: number }>
}

const DETECTOR_META: Record<string, DetectorMeta> = {
  content: {
    defaultThreshold: 27.0,
    step: "0.5",
    range: "15–35",
    direction: "lower = more cuts",
    bestFor: "General-purpose cuts (frame-to-frame HSV color & brightness delta, 0–255 scale).",
    presets: [
      { label: "Sensitive", value: 18 },
      { label: "Default", value: 27 },
      { label: "Strict", value: 35 },
    ],
  },
  adaptive: {
    defaultThreshold: 3.0,
    step: "0.1",
    range: "2.0–4.5",
    direction: "lower = more cuts",
    bestFor: "Handheld or fast-motion clips — compares each frame against neighbors to avoid pan false cuts.",
    presets: [
      { label: "Sensitive", value: 2.2 },
      { label: "Default", value: 3.0 },
      { label: "Strict", value: 4.0 },
    ],
  },
  histogram: {
    defaultThreshold: 0.05,
    step: "0.01",
    range: "0.05–0.25",
    direction: "lower = more cuts",
    bestFor: "Lighting, exposure, or brightness jumps (1 − luma histogram correlation, 0–1 scale).",
    presets: [
      { label: "Sensitive", value: 0.05 },
      { label: "Medium", value: 0.1 },
      { label: "Strict", value: 0.2 },
    ],
  },
  hash: {
    defaultThreshold: 0.395,
    step: "0.01",
    range: "0.25–0.45",
    direction: "lower = more cuts",
    bestFor: "Compressed/noisy footage or subtle flicker (perceptual DCT hash distance, 0–1 scale).",
    presets: [
      { label: "Sensitive", value: 0.28 },
      { label: "Default", value: 0.395 },
      { label: "Strict", value: 0.45 },
    ],
  },
}

type PipelineCfg = {
  detect: {
    detector: string
    threshold: number
    min_scene_length: number
    luma_only: boolean
  }
  preview: {
    images_per_scene: number
    image_width: number
    image_quality: number
    proxy_height: number | null
  }
  review: {
    default_keep: boolean
    auto_apply_template: boolean
  }
  plan: {
    keyframe_snap: string
    min_segment_length: number
  }
  split: {
    output_container: string
  }
  collect: {
    destination: string
    mode: string
  }
  [key: string]: unknown
}

function folderBasename(p: string): string {
  const parts = p.replace(/[/\\]+$/, "").split(/[/\\]/)
  return parts[parts.length - 1] || p
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string
  description?: string
  children: ReactNode
}) {
  return (
    <div className="py-3.5 first:pt-0 last:pb-0 flex items-start justify-between gap-6">
      <div className="space-y-0.5 min-w-0">
        <div className="text-sm font-medium leading-tight">{label}</div>
        {description && (
          <div className="text-xs text-muted-foreground leading-relaxed">{description}</div>
        )}
      </div>
      <div className="shrink-0 flex flex-col items-end gap-1.5">{children}</div>
    </div>
  )
}

export function ConfigDrawer({
  open,
  onOpenChange,
  workingRoot,
  configRevision,
  recentFolders,
  onRecentFoldersChange,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  workingRoot: string | null
  configRevision: number
  recentFolders: string[]
  onRecentFoldersChange: (recent: string[]) => void
}) {
  const [selectedTab, setSelectedTab] = useState<TabId | null>(null)
  const [cfg, setCfg] = useState<PipelineCfg | null>(null)
  const [savedCfgJson, setSavedCfgJson] = useState<string>("")
  const [cfgRoot, setCfgRoot] = useState<string | null>(null)

  const [thresholdText, setThresholdText] = useState<string>("")
  const [minSceneText, setMinSceneText] = useState<string>("")
  const [minSegText, setMinSegText] = useState<string>("")
  const [imagesPerSceneText, setImagesPerSceneText] = useState<string>("")

  const [raw, setRaw] = useState<string>("")
  const [savedRaw, setSavedRaw] = useState<{ root: string | null; text: string }>({ root: null, text: "" })
  const loadedRawRef = useRef<{ root: string | null; text: string }>({ root: null, text: "" })
  const [rawConflict, setRawConflict] = useState(false)

  const [appCfg, setAppCfg] = useState<AppConfig | null>(null)
  const [savedAppCfgJson, setSavedAppCfgJson] = useState<string>("")
  const [customPlayerMode, setCustomPlayerMode] = useState(false)

  const [guideOpen, setGuideOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const inTauri = isTauriRuntime()
  const platformOs = getBootstrap().os
  const showWaylandToggle = platformOs === "linux"

  const activeTab: TabId = selectedTab ?? (workingRoot ? "detection" : "desktop")
  const activeCfg = workingRoot && cfgRoot === workingRoot ? cfg : null

  const applyLoadedProjectConfig = useCallback((parsed: PipelineCfg, root: string) => {
    setCfg(parsed)
    setSavedCfgJson(JSON.stringify(parsed))
    setCfgRoot(root)
    setThresholdText(String(parsed.detect.threshold))
    setMinSceneText(String(parsed.detect.min_scene_length))
    setMinSegText(String(parsed.plan.min_segment_length))
    setImagesPerSceneText(String(parsed.preview.images_per_scene))
  }, [])

  const reloadNow = useCallback(async () => {
    try {
      if (workingRoot) {
        const [c, r] = await Promise.all([api.config.get(), api.config.getRaw()])
        applyLoadedProjectConfig(c as PipelineCfg, workingRoot)
        setRaw(r.text)
        const nextLoaded = { root: workingRoot, text: r.text }
        loadedRawRef.current = nextLoaded
        setSavedRaw(nextLoaded)
        setRawConflict(false)
      }
      setError(null)
      if (inTauri) {
        const ac = await getAppConfigNative()
        if (ac) {
          setAppCfg(ac)
          setSavedAppCfgJson(JSON.stringify(ac))
          setCustomPlayerMode(!!ac.video_player)
        }
      }
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [applyLoadedProjectConfig, inTauri, workingRoot])

  useEffect(() => {
    if (!open) return
    let cancelled = false

    // Always load desktop settings (app.toml) independently so Config works with no folder open.
    if (inTauri) {
      getAppConfigNative()
        .then((ac) => {
          if (!cancelled && ac) {
            setAppCfg(ac)
            setSavedAppCfgJson(JSON.stringify(ac))
            setCustomPlayerMode(!!ac.video_player)
          }
        })
        .catch((e: unknown) => {
          if (!cancelled) setError(e instanceof Error ? e.message : String(e))
        })
    }

    if (!workingRoot) {
      return () => { cancelled = true }
    }

    Promise.all([api.config.get(), api.config.getRaw()])
      .then(([c, r]) => {
        if (cancelled) return
        applyLoadedProjectConfig(c as PipelineCfg, workingRoot)
        setError(null)
        setNotice(null)
        setRaw((prevRaw) => {
          const prev = loadedRawRef.current
          if (
            prev.root !== workingRoot
            || configRevision === 0
            || prevRaw === prev.text
            || prev.text === ""
          ) {
            const nextLoaded = { root: workingRoot, text: r.text }
            loadedRawRef.current = nextLoaded
            setSavedRaw(nextLoaded)
            setRawConflict(false)
            return r.text
          }
          if (r.text !== prev.text) {
            setRawConflict(true)
          }
          return prevRaw
        })
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })

    return () => { cancelled = true }
  }, [open, workingRoot, configRevision, inTauri, applyLoadedProjectConfig])

  const projectDirty = useMemo(() => {
    if (!activeCfg || !savedCfgJson) return false
    return JSON.stringify(activeCfg) !== savedCfgJson
  }, [activeCfg, savedCfgJson])

  const yamlDirty = useMemo(() => {
    if (!workingRoot || savedRaw.root !== workingRoot) return false
    return raw !== savedRaw.text
  }, [raw, savedRaw, workingRoot])

  const appDirty = useMemo(() => {
    if (!appCfg || !savedAppCfgJson) return false
    return JSON.stringify(appCfg) !== savedAppCfgJson
  }, [appCfg, savedAppCfgJson])

  const update = (path: string[], value: unknown) => {
    setNotice(null)
    setCfg((c) => {
      if (!c) return c
      const next = structuredClone(c) as Record<string, unknown>
      let cur = next
      for (let i = 0; i < path.length - 1; i++) {
        cur = cur[path[i]] as Record<string, unknown>
      }
      cur[path[path.length - 1]] = value
      return next as PipelineCfg
    })
  }

  const handleDetectorChange = (detector: string) => {
    const meta = DETECTOR_META[detector]
    setNotice(null)
    setCfg((c) => {
      if (!c) return c
      const next = structuredClone(c)
      next.detect.detector = detector
      if (meta) {
        next.detect.threshold = meta.defaultThreshold
      }
      return next
    })
    if (meta) {
      setThresholdText(String(meta.defaultThreshold))
    }
  }

  const handleThresholdInput = (val: string) => {
    setThresholdText(val)
    const parsed = parseFloat(val)
    if (Number.isFinite(parsed)) {
      update(["detect", "threshold"], parsed)
    }
  }

  const handleMinSceneInput = (val: string) => {
    setMinSceneText(val)
    const parsed = parseFloat(val)
    if (Number.isFinite(parsed) && parsed >= 0) {
      update(["detect", "min_scene_length"], parsed)
    }
  }

  const handleMinSegInput = (val: string) => {
    setMinSegText(val)
    const parsed = parseFloat(val)
    if (Number.isFinite(parsed) && parsed >= 0) {
      update(["plan", "min_segment_length"], parsed)
    }
  }

  const handleImagesPerSceneInput = (val: string) => {
    setImagesPerSceneText(val)
    const parsed = parseInt(val, 10)
    if (Number.isFinite(parsed) && parsed >= 1 && parsed <= 20) {
      update(["preview", "images_per_scene"], parsed)
    }
  }

  const saveStructured = async () => {
    if (!activeCfg || !workingRoot) return
    const parsedThreshold = parseFloat(thresholdText)
    const parsedMinScene = parseFloat(minSceneText)
    const parsedMinSeg = parseFloat(minSegText)
    const parsedImages = parseInt(imagesPerSceneText, 10)
    if (!Number.isFinite(parsedThreshold)) {
      setError("Threshold must be a valid number")
      return
    }
    if (!Number.isFinite(parsedMinScene) || parsedMinScene < 0) {
      setError("Min scene length must be 0 or greater")
      return
    }
    if (!Number.isFinite(parsedMinSeg) || parsedMinSeg < 0) {
      setError("Min segment length must be 0 or greater")
      return
    }
    if (!Number.isFinite(parsedImages) || parsedImages < 1) {
      setError("Thumbnails per scene must be at least 1")
      return
    }
    const payload = structuredClone(activeCfg)
    payload.detect.threshold = parsedThreshold
    payload.detect.min_scene_length = parsedMinScene
    payload.plan.min_segment_length = parsedMinSeg
    payload.preview.images_per_scene = parsedImages

    setSaving(true)
    setError(null)
    setNotice(null)
    try {
      await api.config.put(payload)
      const latestRaw = await api.config.getRaw()
      applyLoadedProjectConfig(payload, workingRoot)
      setRaw(latestRaw.text)
      const nextLoaded = { root: workingRoot, text: latestRaw.text }
      loadedRawRef.current = nextLoaded
      setSavedRaw(nextLoaded)
      setRawConflict(false)
      setNotice("Saved folder config to .vsplit/config.yaml")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const saveRaw = async () => {
    if (!workingRoot) return
    setSaving(true)
    setError(null)
    setNotice(null)
    try {
      await api.config.putRaw(raw)
      const latestCfg = await api.config.get()
      applyLoadedProjectConfig(latestCfg as PipelineCfg, workingRoot)
      const nextLoaded = { root: workingRoot, text: raw }
      loadedRawRef.current = nextLoaded
      setSavedRaw(nextLoaded)
      setRawConflict(false)
      setNotice("Saved raw YAML to .vsplit/config.yaml")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const saveDesktopSettings = async (next: AppConfig) => {
    setSaving(true)
    setError(null)
    setNotice(null)
    try {
      const saved = await setAppConfigNative(next)
      if (saved) {
        setAppCfg(saved)
        setSavedAppCfgJson(JSON.stringify(saved))
        onRecentFoldersChange(saved.recent_folders)
      }
      setNotice("Saved desktop settings to app.toml")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const browseTool = async (field: "ffmpeg_path" | "ffprobe_path" | "video_player") => {
    if (!appCfg) return
    const title =
      field === "ffmpeg_path"
        ? "Select ffmpeg"
        : field === "ffprobe_path"
        ? "Select ffprobe"
        : "Select external video player executable"
    const p = await pickFileNative(title)
    if (!p) return
    setNotice(null)
    setAppCfg({ ...appCfg, [field]: p })
    if (field === "video_player") {
      setCustomPlayerMode(true)
    }
  }

  const handleRemoveRecent = async (folder: string) => {
    try {
      setError(null)
      const res = await api.project.removeRecent(folder)
      onRecentFoldersChange(res.recent)
      setAppCfg((prev) => {
        if (!prev) return prev
        const next = {
          ...prev,
          recent_folders: res.recent,
          last_folder: res.recent[0] ?? null,
        }
        setSavedAppCfgJson(JSON.stringify(next))
        return next
      })
      setNotice("Removed folder from recent list")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const handleClearRecent = async () => {
    try {
      setError(null)
      const res = await api.project.clearRecent()
      onRecentFoldersChange(res.recent)
      setAppCfg((prev) => {
        if (!prev) return prev
        const next = {
          ...prev,
          recent_folders: [],
          last_folder: null,
        }
        setSavedAppCfgJson(JSON.stringify(next))
        return next
      })
      setNotice("Cleared recent folders")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      setSelectedTab(null)
      setNotice(null)
      setError(null)
    }
    onOpenChange(nextOpen)
  }

  const currentTabDirty =
    activeTab === "yaml"
      ? yamlDirty
      : activeTab === "desktop"
      ? appDirty
      : projectDirty

  const handlePrimarySave = () => {
    if (activeTab === "yaml") return void saveRaw()
    if (activeTab === "desktop") {
      if (appCfg) return void saveDesktopSettings(appCfg)
      return
    }
    return void saveStructured()
  }

  const saveLabel =
    activeTab === "yaml"
      ? "Save YAML"
      : activeTab === "desktop"
      ? "Save desktop settings"
      : "Save changes"

  const navBtnClass = (tab: TabId) =>
    cn(
      "w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs transition-colors text-left",
      activeTab === tab
        ? "bg-accent text-accent-foreground font-medium"
        : "text-muted-foreground hover:text-foreground hover:bg-accent/50",
    )

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-3xl w-[92vw] h-[580px] max-h-[85vh] p-0 gap-0 overflow-hidden flex flex-col">
        <DialogHeader className="px-5 py-3.5 border-b border-border flex-shrink-0">
          <DialogTitle className="text-base">Configuration</DialogTitle>
          <DialogDescription className="text-xs">
            {workingRoot
              ? `Folder: ${folderBasename(workingRoot)} (.vsplit/config.yaml) · App settings (app.toml)`
              : "App settings (app.toml) · Open a folder to edit detection & pipeline settings"}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-1 min-h-0">
          {/* Left sidebar navigation */}
          <aside className="w-48 shrink-0 border-r border-border bg-muted/20 p-3 flex flex-col gap-5 overflow-y-auto">
            <div className="space-y-1">
              <div className="px-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground flex items-center justify-between">
                <span>Folder</span>
                {!workingRoot && <span className="font-normal normal-case opacity-70">none</span>}
              </div>
              <button
                type="button"
                onClick={() => { setSelectedTab("detection"); setNotice(null) }}
                className={navBtnClass("detection")}
              >
                <SlidersHorizontal className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">Detection</span>
                {projectDirty && <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />}
              </button>
              <button
                type="button"
                onClick={() => { setSelectedTab("pipeline"); setNotice(null) }}
                className={navBtnClass("pipeline")}
              >
                <Workflow className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">Pipeline</span>
                {projectDirty && <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />}
              </button>
              <button
                type="button"
                onClick={() => { setSelectedTab("yaml"); setNotice(null) }}
                className={navBtnClass("yaml")}
              >
                <FileCode2 className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">Raw YAML</span>
                {(yamlDirty || rawConflict) && <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />}
              </button>
            </div>

            <div className="space-y-1">
              <div className="px-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                App
              </div>
              <button
                type="button"
                onClick={() => { setSelectedTab("desktop"); setNotice(null) }}
                className={navBtnClass("desktop")}
              >
                <Monitor className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">Desktop &amp; Tools</span>
                {appDirty && <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />}
              </button>
            </div>
          </aside>

          {/* Right content pane */}
          <div className="flex-1 min-w-0 flex flex-col">
            <div className="flex-1 overflow-y-auto px-6 py-5">
              {/* Empty state when on a Folder tab without an open folder */}
              {activeTab !== "desktop" && !workingRoot && (
                <div className="h-full flex flex-col items-center justify-center text-center space-y-2 py-8">
                  <div className="text-sm font-medium">No folder open</div>
                  <p className="text-xs text-muted-foreground max-w-sm">
                    Open a video folder first to configure folder-specific scene detection, pipeline defaults, or edit <code>.vsplit/config.yaml</code>.
                  </p>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="mt-2"
                    onClick={() => setSelectedTab("desktop")}
                  >
                    Go to Desktop &amp; Tools
                  </Button>
                </div>
              )}

              {/* Loading state for folder config */}
              {activeTab !== "desktop" && workingRoot && !activeCfg && !error && (
                <div className="text-xs text-muted-foreground py-4">Loading folder configuration…</div>
              )}

              {/* Tab: Detection */}
              {activeTab === "detection" && activeCfg && (
                <div className="space-y-4">
                  <div className="flex items-center justify-between pb-2 border-b border-border">
                    <div>
                      <h3 className="text-sm font-semibold">Scene Detection</h3>
                      <p className="text-xs text-muted-foreground">
                        Algorithm and cut sensitivity used during the detect stage.
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-7 px-2.5 text-xs"
                      onClick={() => setGuideOpen(true)}
                    >
                      <Info className="h-3.5 w-3.5 mr-1.5" />
                      Detector Guide
                    </Button>
                  </div>

                  <div className="divide-y divide-border/60">
                    <SettingRow
                      label="Detector"
                      description="Switching detectors automatically applies its recommended default threshold."
                    >
                      <Select value={activeCfg.detect.detector} onValueChange={handleDetectorChange}>
                        <SelectTrigger className="w-44 h-8 text-xs">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {DETECTORS.map((d) => (
                            <SelectItem key={d} value={d} className="text-xs">
                              {d}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow
                      label="Threshold"
                      description={
                        DETECTOR_META[activeCfg.detect.detector]
                          ? `Typical: ${DETECTOR_META[activeCfg.detect.detector].range} (${DETECTOR_META[activeCfg.detect.detector].direction})`
                          : undefined
                      }
                    >
                      <Input
                        type="number"
                        step={DETECTOR_META[activeCfg.detect.detector]?.step ?? "0.01"}
                        value={thresholdText}
                        onChange={(e) => handleThresholdInput(e.target.value)}
                        className="w-44 h-8 text-xs font-mono"
                      />
                      {DETECTOR_META[activeCfg.detect.detector] && (
                        <div className="flex items-center gap-1 flex-wrap justify-end">
                          {DETECTOR_META[activeCfg.detect.detector].presets.map((p) => {
                            const currentNum = parseFloat(thresholdText)
                            const isSelected = Number.isFinite(currentNum) && Math.abs(currentNum - p.value) < 1e-6
                            return (
                              <Button
                                key={p.label}
                                type="button"
                                size="sm"
                                variant={isSelected ? "secondary" : "outline"}
                                className="h-6 px-2 text-[11px] font-normal"
                                onClick={() => handleThresholdInput(String(p.value))}
                              >
                                {p.label} <span className="ml-1 font-mono opacity-80">{p.value}</span>
                              </Button>
                            )
                          })}
                        </div>
                      )}
                    </SettingRow>

                    <SettingRow
                      label="Min scene length (seconds)"
                      description="Suppresses rapid flash cuts shorter than this duration."
                    >
                      <Input
                        type="number"
                        min="0"
                        step="0.1"
                        value={minSceneText}
                        onChange={(e) => handleMinSceneInput(e.target.value)}
                        className="w-28 h-8 text-xs font-mono"
                      />
                    </SettingRow>

                    <SettingRow
                      label="Luma only"
                      description="Score brightness channel only for faster detection (content & adaptive)."
                    >
                      <Switch
                        checked={activeCfg.detect.luma_only}
                        onCheckedChange={(v) => update(["detect", "luma_only"], v)}
                      />
                    </SettingRow>
                  </div>
                </div>
              )}

              {/* Tab: Pipeline */}
              {activeTab === "pipeline" && activeCfg && (
                <div className="space-y-5">
                  <div className="rounded-lg border border-border bg-card/40 overflow-hidden">
                    <div className="px-4 py-2.5 bg-muted/50 border-b border-border flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Eye className="h-3.5 w-3.5 text-primary" />
                        <h3 className="text-xs font-semibold uppercase tracking-wider text-foreground">
                          Review &amp; Preview
                        </h3>
                      </div>
                      <span className="text-[11px] text-muted-foreground">
                        Scene review &amp; video proxy
                      </span>
                    </div>
                    <div className="px-4 py-2 divide-y divide-border/60">
                      <SettingRow
                        label="Keep scenes by default"
                        description="Mark newly detected scenes as kept in decisions/cuts.json."
                      >
                        <Switch
                          checked={activeCfg.review.default_keep}
                          onCheckedChange={(v) => update(["review", "default_keep"], v)}
                        />
                      </SettingRow>
                      <SettingRow
                        label="Thumbnails per scene"
                        description="Number of preview thumbnails extracted for each detected scene."
                      >
                        <Input
                          type="number"
                          min="1"
                          max="20"
                          step="1"
                          value={imagesPerSceneText}
                          onChange={(e) => handleImagesPerSceneInput(e.target.value)}
                          className="w-24 h-8 text-xs font-mono"
                        />
                      </SettingRow>
                      <SettingRow
                        label="Preview proxy height"
                        description="Downscale height for the in-app review video proxy."
                      >
                        <Select
                          value={activeCfg.preview.proxy_height == null ? "none" : String(activeCfg.preview.proxy_height)}
                          onValueChange={(v) =>
                            update(["preview", "proxy_height"], v === "none" ? null : parseInt(v, 10))
                          }
                        >
                          <SelectTrigger className="w-44 h-8 text-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {PROXY_HEIGHTS.map((opt) => (
                              <SelectItem key={opt.value} value={opt.value} className="text-xs">
                                {opt.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </SettingRow>
                    </div>
                  </div>

                  <div className="rounded-lg border border-border bg-card/40 overflow-hidden">
                    <div className="px-4 py-2.5 bg-muted/50 border-b border-border flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Scissors className="h-3.5 w-3.5 text-primary" />
                        <h3 className="text-xs font-semibold uppercase tracking-wider text-foreground">
                          Split &amp; Collect
                        </h3>
                      </div>
                      <span className="text-[11px] text-muted-foreground">
                        Output format &amp; export folder
                      </span>
                    </div>
                    <div className="px-4 py-2 divide-y divide-border/60">
                      <SettingRow
                        label="Output container"
                        description="File format for split scene clips."
                      >
                        <Select
                          value={activeCfg.split.output_container}
                          onValueChange={(v) => update(["split", "output_container"], v)}
                        >
                          <SelectTrigger className="w-32 h-8 text-xs font-mono">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {CONTAINERS.map((c) => (
                              <SelectItem key={c} value={c} className="text-xs font-mono">
                                .{c}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </SettingRow>
                      <SettingRow
                        label="Min segment length (seconds)"
                        description="Minimum duration for planned output segments."
                      >
                        <Input
                          type="number"
                          min="0"
                          step="0.1"
                          value={minSegText}
                          onChange={(e) => handleMinSegInput(e.target.value)}
                          className="w-28 h-8 text-xs font-mono"
                        />
                      </SettingRow>
                      <SettingRow
                        label="Collect destination"
                        description="Default subfolder (relative to the open folder) for collected clips."
                      >
                        <Input
                          value={activeCfg.collect.destination}
                          onChange={(e) => update(["collect", "destination"], e.target.value)}
                          className="w-44 h-8 text-xs font-mono"
                        />
                      </SettingRow>
                      <SettingRow
                        label="Collect mode"
                        description="Whether Collect copies, moves, or hard-links finished clips."
                      >
                        <Select
                          value={activeCfg.collect.mode}
                          onValueChange={(v) => update(["collect", "mode"], v)}
                        >
                          <SelectTrigger className="w-32 h-8 text-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {COLLECT_MODES.map((m) => (
                              <SelectItem key={m} value={m} className="text-xs">
                                {m}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </SettingRow>
                    </div>
                  </div>
                </div>
              )}

              {/* Tab: Raw YAML */}
              {activeTab === "yaml" && workingRoot && (
                <div className="h-full flex flex-col gap-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-sm font-semibold">Raw Folder Config</h3>
                      <p className="text-xs text-muted-foreground font-mono">.vsplit/config.yaml</p>
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="h-7 px-2.5 text-xs"
                      onClick={() => void reloadNow()}
                    >
                      Reload from disk
                    </Button>
                  </div>

                  {rawConflict && (
                    <div className="p-2.5 text-xs rounded-md bg-amber-500/15 border border-amber-500/40 text-amber-200 flex items-center justify-between gap-2">
                      <span>External edit detected in .vsplit/config.yaml.</span>
                      <div className="flex gap-1.5">
                        <Button size="sm" variant="outline" className="h-6 px-2 text-xs" onClick={() => void reloadNow()}>
                          Reload
                        </Button>
                        <Button size="sm" variant="secondary" className="h-6 px-2 text-xs" onClick={() => setRawConflict(false)}>
                          Keep Mine
                        </Button>
                      </div>
                    </div>
                  )}

                  <textarea
                    value={raw}
                    onChange={(e) => { setRaw(e.target.value); setNotice(null) }}
                    spellCheck={false}
                    className="flex-1 min-h-[320px] w-full bg-zinc-950/70 border border-border rounded-md p-3 font-mono text-xs leading-relaxed resize-none focus:outline-none focus:ring-1 focus:ring-ring"
                  />
                </div>
              )}

              {/* Tab: Desktop & Tools */}
              {activeTab === "desktop" && (
                <div className="space-y-4">
                  <div className="pb-2 border-b border-border">
                    <h3 className="text-sm font-semibold">Desktop &amp; Media Tools</h3>
                    <p className="text-xs text-muted-foreground">
                      Global app preferences stored in <code>app.toml</code>. Applies to all folders.
                    </p>
                  </div>

                  {!inTauri && (
                    <div className="p-3 text-xs rounded-md bg-muted/50 border border-border text-muted-foreground">
                      Desktop settings are managed by the native Tauri shell and are unavailable in browser dev mode.
                    </div>
                  )}

                  {inTauri && !appCfg && !error && (
                    <div className="text-xs text-muted-foreground py-2">Loading desktop settings…</div>
                  )}

                  {inTauri && appCfg && (
                    <div className="divide-y divide-border/60">
                      <SettingRow
                        label="External video player"
                        description="Used when clicking Open externally ↗ on a video preview."
                      >
                        <Select
                          value={customPlayerMode ? "custom" : "default"}
                          onValueChange={(v) => {
                            setNotice(null)
                            if (v === "default") {
                              setCustomPlayerMode(false)
                              setAppCfg({ ...appCfg, video_player: null })
                            } else {
                              setCustomPlayerMode(true)
                            }
                          }}
                        >
                          <SelectTrigger className="w-56 h-8 text-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="default" className="text-xs">
                              System default video player
                            </SelectItem>
                            <SelectItem value="custom" className="text-xs">
                              Custom player executable…
                            </SelectItem>
                          </SelectContent>
                        </Select>
                        {customPlayerMode && (
                          <div className="flex items-center gap-1.5">
                            <Input
                              value={appCfg.video_player ?? ""}
                              placeholder="Path to mpv, VLC, PotPlayer…"
                              onChange={(e) => {
                                setNotice(null)
                                setAppCfg({ ...appCfg, video_player: e.target.value || null })
                              }}
                              className="w-56 h-8 text-xs font-mono"
                            />
                            <Button
                              type="button"
                              size="icon"
                              variant="outline"
                              className="h-8 w-8 shrink-0"
                              onClick={() => void browseTool("video_player")}
                              title="Browse for video player executable"
                            >
                              <FolderSearch className="h-3.5 w-3.5" />
                            </Button>
                            {appCfg.video_player && (
                              <Button
                                type="button"
                                size="icon"
                                variant="ghost"
                                className="h-8 w-8 shrink-0"
                                onClick={() => {
                                  setNotice(null)
                                  setCustomPlayerMode(false)
                                  setAppCfg({ ...appCfg, video_player: null })
                                }}
                                title="Reset to system default player"
                              >
                                <X className="h-3.5 w-3.5" />
                              </Button>
                            )}
                          </div>
                        )}
                      </SettingRow>

                      <SettingRow
                        label="FFmpeg path"
                        description="Optional override. Leave blank to auto-discover from PATH."
                      >
                        <div className="flex items-center gap-1.5">
                          <Input
                            value={appCfg.ffmpeg_path ?? ""}
                            placeholder="Auto-discover from PATH"
                            onChange={(e) => {
                              setNotice(null)
                              setAppCfg({ ...appCfg, ffmpeg_path: e.target.value || null })
                            }}
                            className="w-56 h-8 text-xs font-mono"
                          />
                          <Button
                            type="button"
                            size="icon"
                            variant="outline"
                            className="h-8 w-8 shrink-0"
                            onClick={() => void browseTool("ffmpeg_path")}
                            title="Browse for ffmpeg"
                          >
                            <FolderSearch className="h-3.5 w-3.5" />
                          </Button>
                          {appCfg.ffmpeg_path && (
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-8 w-8 shrink-0"
                              onClick={() => {
                                setNotice(null)
                                setAppCfg({ ...appCfg, ffmpeg_path: null })
                              }}
                              title="Reset to auto-discover"
                            >
                              <X className="h-3.5 w-3.5" />
                            </Button>
                          )}
                        </div>
                      </SettingRow>

                      <SettingRow
                        label="FFprobe path"
                        description="Optional override. Leave blank to auto-discover from PATH."
                      >
                        <div className="flex items-center gap-1.5">
                          <Input
                            value={appCfg.ffprobe_path ?? ""}
                            placeholder="Auto-discover from PATH"
                            onChange={(e) => {
                              setNotice(null)
                              setAppCfg({ ...appCfg, ffprobe_path: e.target.value || null })
                            }}
                            className="w-56 h-8 text-xs font-mono"
                          />
                          <Button
                            type="button"
                            size="icon"
                            variant="outline"
                            className="h-8 w-8 shrink-0"
                            onClick={() => void browseTool("ffprobe_path")}
                            title="Browse for ffprobe"
                          >
                            <FolderSearch className="h-3.5 w-3.5" />
                          </Button>
                          {appCfg.ffprobe_path && (
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-8 w-8 shrink-0"
                              onClick={() => {
                                setNotice(null)
                                setAppCfg({ ...appCfg, ffprobe_path: null })
                              }}
                              title="Reset to auto-discover"
                            >
                              <X className="h-3.5 w-3.5" />
                            </Button>
                          )}
                        </div>
                      </SettingRow>

                      {showWaylandToggle && (
                        <SettingRow
                          label="Disable WebKitGTK DMABUF renderer"
                          description="Linux Wayland workaround if the window renders blank (restart required)."
                        >
                          <Switch
                            checked={appCfg.wayland_disable_dmabuf}
                            onCheckedChange={(v) => {
                              setNotice(null)
                              setAppCfg({ ...appCfg, wayland_disable_dmabuf: v })
                            }}
                          />
                        </SettingRow>
                      )}
                    </div>
                  )}

                  <div className="rounded-lg border border-border bg-card/40 overflow-hidden mt-4">
                    <div className="px-4 py-2.5 bg-muted/50 border-b border-border flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <FolderClock className="h-3.5 w-3.5 text-primary" />
                        <h4 className="text-xs font-semibold uppercase tracking-wider text-foreground">
                          Recent Folders
                        </h4>
                      </div>
                      <div className="flex items-center gap-2">
                        {inTauri && appCfg && (
                          <div className="flex items-center gap-1.5">
                            <span className="text-[11px] text-muted-foreground">Max</span>
                            <Select
                              value={String(appCfg.max_recent_folders ?? 10)}
                              onValueChange={(v) => {
                                setNotice(null)
                                setAppCfg({ ...appCfg, max_recent_folders: parseInt(v, 10) })
                              }}
                            >
                              <SelectTrigger className="h-6 w-16 px-2 text-[11px] font-mono">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {RECENT_LIMITS.map((n) => (
                                  <SelectItem key={n} value={String(n)} className="text-xs font-mono">
                                    {n}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                        )}
                        {recentFolders.length > 0 && (
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            className="h-6 px-2 text-[11px] text-muted-foreground hover:text-destructive"
                            onClick={() => void handleClearRecent()}
                          >
                            <Trash2 className="h-3 w-3 mr-1" />
                            Clear all
                          </Button>
                        )}
                      </div>
                    </div>
                    <div className="divide-y divide-border/60">
                      {recentFolders.length === 0 ? (
                        <div className="px-4 py-3 text-xs text-muted-foreground">
                          No recent folders saved.
                        </div>
                      ) : (
                        recentFolders.map((rf) => (
                          <div key={rf} className="px-4 py-2 flex items-center justify-between gap-2">
                            <span className="font-mono text-xs truncate text-foreground/90" title={rf}>
                              {rf}
                            </span>
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-6 w-6 shrink-0 text-muted-foreground hover:text-destructive"
                              onClick={() => void handleRemoveRecent(rf)}
                              title={`Remove ${rf} from recent folders`}
                            >
                              <X className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* Sticky scope-aware footer */}
            <div className="border-t border-border px-6 py-3 flex items-center justify-between gap-3 bg-muted/10 flex-shrink-0">
              <div className="text-xs min-w-0 truncate">
                {error ? (
                  <span className="text-red-400">{error}</span>
                ) : notice ? (
                  <span className="text-emerald-400">{notice}</span>
                ) : currentTabDirty ? (
                  <span className="inline-flex items-center gap-1.5 text-amber-300">
                    <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />
                    Unsaved changes
                  </span>
                ) : (
                  <span className="text-muted-foreground">
                    {activeTab === "desktop" ? "Applies to all folders (app.toml)" : "Applies to current folder"}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => handleOpenChange(false)}
                >
                  Close
                </Button>
                {(activeTab === "desktop" ? inTauri && !!appCfg : !!workingRoot && (activeTab === "yaml" || !!activeCfg)) && (
                  <Button
                    type="button"
                    size="sm"
                    disabled={saving || !currentTabDirty}
                    onClick={handlePrimarySave}
                  >
                    {saving ? "Saving…" : saveLabel}
                  </Button>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Nested Detector & Threshold Guide dialog */}
        <Dialog open={guideOpen} onOpenChange={setGuideOpen}>
          <DialogContent className="max-w-2xl">
            <DialogHeader>
              <DialogTitle>Detector &amp; Threshold Guide</DialogTitle>
              <DialogDescription>
                Each detector uses its own threshold scale. Click Use to apply a detector with its recommended default.
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2 text-xs max-h-[60vh] overflow-y-auto pr-1">
              {DETECTORS.map((name) => {
                const meta = DETECTOR_META[name]
                const isActive = activeCfg?.detect.detector === name
                return (
                  <div
                    key={name}
                    className={`rounded-md border p-3 flex items-start justify-between gap-3 ${
                      isActive ? "border-primary/50 bg-primary/5" : "border-border bg-card"
                    }`}
                  >
                    <div className="space-y-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="font-semibold font-mono text-sm">{name}</span>
                        <span className="px-1.5 py-0.5 rounded bg-muted font-mono text-[11px]">
                          default {meta.defaultThreshold}
                        </span>
                        <span className="text-muted-foreground">
                          range {meta.range} · {meta.direction}
                        </span>
                      </div>
                      <p className="text-muted-foreground leading-relaxed">{meta.bestFor}</p>
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant={isActive ? "secondary" : "outline"}
                      className="h-7 px-2.5 text-xs shrink-0"
                      onClick={() => {
                        handleDetectorChange(name)
                        setGuideOpen(false)
                      }}
                    >
                      {isActive ? "Reset default" : "Use"}
                    </Button>
                  </div>
                )
              })}

              <div className="rounded-md border border-border bg-muted/40 p-3 space-y-1 text-muted-foreground">
                <div className="font-semibold text-foreground">Tuning tip</div>
                <p>
                  After running detect, click the <span className="font-mono text-foreground">detect</span> stage dot on a video row to view its score summary (<span className="font-mono">scores=[min p50 p95 max]</span>). Pick a threshold between <span className="font-mono">p95</span> and <span className="font-mono">max</span>, then click <strong>Reset → detect</strong> to re-run.
                </p>
              </div>
            </div>
          </DialogContent>
        </Dialog>
      </DialogContent>
    </Dialog>
  )
}
