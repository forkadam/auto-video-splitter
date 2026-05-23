import { useEffect, useState } from "react"
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

const DETECTORS = ["content", "adaptive", "histogram", "hash"]

export function ConfigDrawer({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const [cfg, setCfg] = useState<any>(null)
  const [raw, setRaw] = useState<string>("")
  const [showRaw, setShowRaw] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    api.config.get().then(setCfg)
    api.config.getRaw().then((d) => setRaw(d.text))
  }, [open])

  const update = (path: string[], value: any) => {
    setCfg((c: any) => {
      const next = JSON.parse(JSON.stringify(c))
      let cur = next
      for (let i = 0; i < path.length - 1; i++) cur = cur[path[i]]
      cur[path[path.length - 1]] = value
      return next
    })
  }

  const saveStructured = async () => {
    try {
      setError(null)
      await api.config.put(cfg)
      onOpenChange(false)
    } catch (e: any) {
      setError(String(e.message ?? e))
    }
  }

  const saveRaw = async () => {
    try {
      setError(null)
      await api.config.putRaw(raw)
      onOpenChange(false)
    } catch (e: any) {
      setError(String(e.message ?? e))
    }
  }

  if (!cfg) return null

  return (
    <Drawer open={open} onOpenChange={onOpenChange}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>Configuration</DrawerTitle>
        </DrawerHeader>
        <div className="px-4 pb-4 space-y-4 overflow-y-auto max-h-[70vh]">
          <section className="space-y-2">
            <h3 className="text-sm font-semibold">Detect</h3>
            <label className="text-xs flex items-center gap-2">detector
              <Select value={cfg.detect.detector} onValueChange={(v) => update(["detect", "detector"], v)}>
                <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                <SelectContent>{DETECTORS.map(d => <SelectItem key={d} value={d}>{d}</SelectItem>)}</SelectContent>
              </Select>
            </label>
            <label className="text-xs flex items-center gap-2">threshold
              <Input type="number" step="0.01" value={cfg.detect.threshold}
                onChange={(e) => update(["detect", "threshold"], parseFloat(e.target.value))} className="w-32" />
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
              <textarea value={raw} onChange={(e) => setRaw(e.target.value)}
                className="w-full mt-2 h-48 bg-zinc-900 border border-zinc-800 rounded p-2 font-mono text-xs" />
            )}
          </section>
          {error && <div className="text-red-400 text-xs">{error}</div>}
          <div className="flex gap-2">
            <Button onClick={saveStructured}>Save form</Button>
            {showRaw && <Button variant="secondary" onClick={saveRaw}>Save raw YAML</Button>}
            <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          </div>
        </div>
      </DrawerContent>
    </Drawer>
  )
}
