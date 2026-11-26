import { useCallback, useEffect, useRef, useState } from "react"
import { FolderSearch, Info } from "lucide-react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"
import { api } from "@/lib/api"
import {
  getAppConfigNative,
  isTauriRuntime,
  pickFileNative,
  setAppConfigNative,
  type AppConfig,
} from "@/lib/bootstrap"

const DETECTORS = ["content", "adaptive", "histogram", "hash"] as const

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
  detect: { detector: string; threshold: number; min_scene_length: number; luma_only: boolean }
  review: { default_keep: boolean; auto_apply_template: boolean }
  [key: string]: unknown
}

export function ConfigDrawer({
  open,
  onOpenChange,
  workingRoot,
  configRevision,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  workingRoot: string | null
  configRevision: number
}) {
  const [cfg, setCfg] = useState<PipelineCfg | null>(null)
  const [cfgRoot, setCfgRoot] = useState<string | null>(null)
  const [thresholdText, setThresholdText] = useState<string>("")
  const [raw, setRaw] = useState<string>("")
  const loadedRawRef = useRef<{ root: string | null; text: string }>({ root: null, text: "" })
  const [showRaw, setShowRaw] = useState(false)
  const [guideOpen, setGuideOpen] = useState(false)
  const [rawConflict, setRawConflict] = useState(false)
  const [appCfg, setAppCfg] = useState<AppConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const inTauri = isTauriRuntime()

  const activeCfg = workingRoot && cfgRoot === workingRoot ? cfg : null

  const reloadNow = useCallback(async () => {
    try {
      if (workingRoot) {
        const [c, r] = await Promise.all([api.config.get(), api.config.getRaw()])
        const parsed = c as PipelineCfg
        setCfg(parsed)
        setCfgRoot(workingRoot)
        setThresholdText(String(parsed.detect.threshold))
        setRaw(r.text)
        loadedRawRef.current = { root: workingRoot, text: r.text }
        setRawConflict(false)
      }
      setError(null)
      if (inTauri) {
        const ac = await getAppConfigNative()
        if (ac) setAppCfg(ac)
      }
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [inTauri, workingRoot])

  useEffect(() => {
    if (!open) return
    let cancelled = false

    // Always load desktop settings (app.toml) independently so Config opens even when no folder is open.
    if (inTauri) {
      getAppConfigNative()
        .then((ac) => {
          if (!cancelled && ac) setAppCfg(ac)
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
        const parsed = c as PipelineCfg
        setCfg(parsed)
        setCfgRoot(workingRoot)
        setThresholdText(String(parsed.detect.threshold))
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
            loadedRawRef.current = { root: workingRoot, text: r.text }
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
  }, [open, workingRoot, configRevision, inTauri])

  const update = (path: string[], value: unknown) => {
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

  const saveStructured = async () => {
    if (!activeCfg) return
    const parsedThreshold = parseFloat(thresholdText)
    if (!Number.isFinite(parsedThreshold)) {
      setError("Threshold must be a valid number")
      return
    }
    const payload = structuredClone(activeCfg)
    payload.detect.threshold = parsedThreshold
    try {
      setError(null)
      await api.config.put(payload)
      onOpenChange(false)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const saveRaw = async () => {
    try {
      setError(null)
      await api.config.putRaw(raw)
      loadedRawRef.current = { root: workingRoot, text: raw }
      setRawConflict(false)
      onOpenChange(false)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const saveDesktopSettings = async (next: AppConfig) => {
    try {
      setError(null)
      const saved = await setAppConfigNative(next)
      if (saved) setAppCfg(saved)
      setNotice("Saved desktop settings to app.toml")
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const browseTool = async (field: "ffmpeg_path" | "ffprobe_path") => {
    if (!appCfg) return
    const p = await pickFileNative(`Select ${field === "ffmpeg_path" ? "ffmpeg" : "ffprobe"}`)
    if (!p) return
    await saveDesktopSettings({ ...appCfg, [field]: p })
  }

  return (
    <Drawer open={open} onOpenChange={onOpenChange}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>Configuration</DrawerTitle>
        </DrawerHeader>
        <div className="px-4 pb-4 space-y-4 overflow-y-auto max-h-[75vh]">
          {!workingRoot && (
            <div className="p-2.5 text-xs rounded-md bg-muted/50 border border-border text-muted-foreground">
              Open a project folder to edit per-project pipeline configuration (<code>.vsplit/config.yaml</code>).
            </div>
          )}

          {workingRoot && !activeCfg && !error && (
            <div className="text-xs text-muted-foreground">Loading project configuration…</div>
          )}

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

          {activeCfg && (
            <>
              <section className="space-y-2.5">
                <div className="flex items-center gap-1.5">
                  <h3 className="text-sm font-semibold">Detect</h3>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-6 px-1.5 text-xs text-muted-foreground hover:text-foreground"
                    onClick={() => setGuideOpen(true)}
                    title="View recommended detector and threshold guide"
                  >
                    <Info className="h-3.5 w-3.5 mr-1" />
                    Guide
                  </Button>
                </div>
                <label className="text-xs flex items-center gap-2">detector
                  <Select value={activeCfg.detect.detector} onValueChange={handleDetectorChange}>
                    <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                    <SelectContent>{DETECTORS.map((d) => <SelectItem key={d} value={d}>{d}</SelectItem>)}</SelectContent>
                  </Select>
                </label>
                <div className="space-y-1.5">
                  <label className="text-xs flex items-center gap-2">threshold
                    <Input
                      type="number"
                      step={DETECTOR_META[activeCfg.detect.detector]?.step ?? "0.01"}
                      value={thresholdText}
                      onChange={(e) => handleThresholdInput(e.target.value)}
                      className="w-32"
                    />
                  </label>
                  {DETECTOR_META[activeCfg.detect.detector] && (
                    <>
                      <div className="flex items-center gap-1.5 flex-wrap">
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
                      <div className="text-[11px] text-muted-foreground">
                        Typical: {DETECTOR_META[activeCfg.detect.detector].range} ({DETECTOR_META[activeCfg.detect.detector].direction})
                      </div>
                    </>
                  )}
                </div>
              </section>

              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Review</h3>
                <label className="text-xs flex items-center gap-2">default_keep
                  <Switch checked={activeCfg.review.default_keep} onCheckedChange={(v) => update(["review", "default_keep"], v)} />
                </label>
              </section>

              <section>
                <button className="text-xs text-zinc-400" onClick={() => setShowRaw((s) => !s)}>
                  {showRaw ? "▾" : "▸"} Advanced (raw YAML)
                </button>
                {showRaw && (
                  <textarea
                    value={raw}
                    onChange={(e) => setRaw(e.target.value)}
                    className="w-full mt-2 h-48 bg-zinc-900 border border-zinc-800 rounded p-2 font-mono text-xs"
                  />
                )}
              </section>
            </>
          )}

          {inTauri && !appCfg && !error && (
            <div className="text-xs text-muted-foreground">Loading desktop settings…</div>
          )}

          {inTauri && appCfg && (
            <section className="space-y-2 pt-3 border-t border-border">
              <h3 className="text-sm font-semibold">Desktop Settings (app.toml)</h3>
              <div className="space-y-1.5">
                <label className="text-xs text-muted-foreground block">ffmpeg path (optional override)</label>
                <div className="flex gap-2">
                  <Input
                    value={appCfg.ffmpeg_path ?? ""}
                    placeholder="Auto-discover from PATH"
                    onChange={(e) => setAppCfg({ ...appCfg, ffmpeg_path: e.target.value || null })}
                    className="h-8 text-xs font-mono"
                  />
                  <Button size="sm" variant="outline" onClick={() => void browseTool("ffmpeg_path")}>
                    <FolderSearch className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
              <div className="space-y-1.5">
                <label className="text-xs text-muted-foreground block">ffprobe path (optional override)</label>
                <div className="flex gap-2">
                  <Input
                    value={appCfg.ffprobe_path ?? ""}
                    placeholder="Auto-discover from PATH"
                    onChange={(e) => setAppCfg({ ...appCfg, ffprobe_path: e.target.value || null })}
                    className="h-8 text-xs font-mono"
                  />
                  <Button size="sm" variant="outline" onClick={() => void browseTool("ffprobe_path")}>
                    <FolderSearch className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
              <label className="text-xs flex items-center gap-2 pt-1">
                <span>Disable WebKitGTK DMABUF renderer on Wayland (restart required)</span>
                <Switch
                  checked={appCfg.wayland_disable_dmabuf}
                  onCheckedChange={(v) => void saveDesktopSettings({ ...appCfg, wayland_disable_dmabuf: v })}
                />
              </label>
              <Button size="sm" variant="secondary" onClick={() => void saveDesktopSettings(appCfg)}>
                Save desktop settings
              </Button>
            </section>
          )}

          {notice && <div className="text-emerald-400 text-xs">{notice}</div>}
          {error && <div className="text-red-400 text-xs">{error}</div>}
          <div className="flex gap-2">
            {activeCfg && <Button onClick={() => void saveStructured()}>Save form</Button>}
            {activeCfg && showRaw && <Button variant="secondary" onClick={() => void saveRaw()}>Save raw YAML</Button>}
            <Button variant="ghost" onClick={() => onOpenChange(false)}>Close</Button>
          </div>
        </div>

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
      </DrawerContent>
    </Drawer>
  )
}
