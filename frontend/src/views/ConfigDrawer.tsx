import { useCallback, useEffect, useState } from "react"
import { FolderSearch } from "lucide-react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
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

const DETECTORS = ["content", "adaptive", "histogram", "hash"]

type PipelineCfg = {
  detect: { detector: string; threshold: number; min_scene_length: number; luma_only: boolean }
  review: { default_keep: boolean; auto_apply_template: boolean }
  [key: string]: unknown
}

export function ConfigDrawer({
  open,
  onOpenChange,
  configRevision,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  configRevision: number
}) {
  const [cfg, setCfg] = useState<PipelineCfg | null>(null)
  const [raw, setRaw] = useState<string>("")
  const [loadedRaw, setLoadedRaw] = useState<string>("")
  const [showRaw, setShowRaw] = useState(false)
  const [rawConflict, setRawConflict] = useState(false)
  const [appCfg, setAppCfg] = useState<AppConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const inTauri = isTauriRuntime()

  const reloadNow = useCallback(async () => {
    try {
      const [c, r] = await Promise.all([api.config.get(), api.config.getRaw()])
      setCfg(c as PipelineCfg)
      setRaw(r.text)
      setLoadedRaw(r.text)
      setRawConflict(false)
      setError(null)
      if (inTauri) {
        const ac = await getAppConfigNative()
        if (ac) setAppCfg(ac)
      }
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [inTauri])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    Promise.all([api.config.get(), api.config.getRaw()])
      .then(async ([c, r]) => {
        if (cancelled) return
        setCfg(c as PipelineCfg)
        setError(null)
        setNotice(null)
        setRaw((prevRaw) => {
          if (configRevision === 0 || prevRaw === loadedRaw) {
            setLoadedRaw(r.text)
            setRawConflict(false)
            return r.text
          }
          if (r.text !== loadedRaw) {
            setRawConflict(true)
          }
          return prevRaw
        })
        if (inTauri) {
          const ac = await getAppConfigNative()
          if (!cancelled && ac) setAppCfg(ac)
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })
    return () => { cancelled = true }
  }, [open, configRevision, inTauri, loadedRaw])

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

  const saveStructured = async () => {
    if (!cfg) return
    try {
      setError(null)
      await api.config.put(cfg)
      onOpenChange(false)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const saveRaw = async () => {
    try {
      setError(null)
      await api.config.putRaw(raw)
      setLoadedRaw(raw)
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

  if (!cfg && !appCfg) return null

  return (
    <Drawer open={open} onOpenChange={onOpenChange}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>Configuration</DrawerTitle>
        </DrawerHeader>
        <div className="px-4 pb-4 space-y-4 overflow-y-auto max-h-[75vh]">
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

          {cfg && (
            <>
              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Detect</h3>
                <label className="text-xs flex items-center gap-2">detector
                  <Select value={cfg.detect.detector} onValueChange={(v) => update(["detect", "detector"], v)}>
                    <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                    <SelectContent>{DETECTORS.map((d) => <SelectItem key={d} value={d}>{d}</SelectItem>)}</SelectContent>
                  </Select>
                </label>
                <label className="text-xs flex items-center gap-2">threshold
                  <Input
                    type="number"
                    step="0.01"
                    value={cfg.detect.threshold}
                    onChange={(e) => update(["detect", "threshold"], parseFloat(e.target.value))}
                    className="w-32"
                  />
                </label>
              </section>

              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Review</h3>
                <label className="text-xs flex items-center gap-2">default_keep
                  <Switch checked={cfg.review.default_keep} onCheckedChange={(v) => update(["review", "default_keep"], v)} />
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
            {cfg && <Button onClick={() => void saveStructured()}>Save form</Button>}
            {cfg && showRaw && <Button variant="secondary" onClick={() => void saveRaw()}>Save raw YAML</Button>}
            <Button variant="ghost" onClick={() => onOpenChange(false)}>Close</Button>
          </div>
        </div>
      </DrawerContent>
    </Drawer>
  )
}
