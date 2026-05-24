import { useEffect, useMemo, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle, DrawerFooter,
} from "@/components/ui/drawer"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { api, type CutsBody } from "@/lib/api"

type Props = {
  slug: string | null
  onClose: () => void
}

type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
}

export function CutsDrawer({ slug, onClose }: Props) {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [draft, setDraft] = useState<CutsBody | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [forceOpen, setForceOpen] = useState(false)

  useEffect(() => {
    if (!slug) {
      setLoaded(null)
      setDraft(null)
      setError(null)
      return
    }
    let cancelled = false
    api.workspaces.cuts.get(slug)
      .then((r) => {
        if (cancelled) return
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs })
        setDraft(structuredClone(r.cuts))
        setError(null)
      })
      .catch((e) => {
        if (cancelled) return
        setError(String(e.message ?? e))
      })
    return () => { cancelled = true }
  }, [slug])

  // Mirror slug presence into a local open flag so the confirm-cancel flow
  // can re-assert open=true if the user aborts the discard prompt.
  useEffect(() => { setForceOpen(slug !== null) }, [slug])

  const isDirty = useMemo(() => {
    if (!loaded || !draft) return false
    return JSON.stringify(loaded.cuts) !== JSON.stringify(draft)
  }, [loaded, draft])

  const update = (idx: number, patch: Partial<CutsBody["scenes"][number]>) => {
    setDraft((d) => {
      if (!d) return d
      const next = structuredClone(d)
      next.scenes[idx] = { ...next.scenes[idx], ...patch }
      return next
    })
  }

  const cancel = () => {
    if (isDirty && !window.confirm("Discard unsaved changes?")) return
    setForceOpen(false)
    onClose()
  }

  const save = async (): Promise<boolean> => {
    if (!slug || !loaded || !draft) return false
    setBusy(true)
    setError(null)
    try {
      const r = await api.workspaces.cuts.put(slug, {
        cuts: draft, expected_hash: loaded.hash,
      })
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs })
      return true
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      return false
    } finally {
      setBusy(false)
    }
  }

  const saveAndContinue = async () => {
    if (!slug) return
    if (isDirty) {
      const ok = await save()
      if (!ok) return  // error already surfaced by save()
    }
    setBusy(true)
    setError(null)
    try {
      await api.workspaces.approve(slug)
      await api.workspaces.run(slug, {})
      onClose()
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Drawer
      open={forceOpen}
      onOpenChange={(o) => {
        if (o) return
        if (isDirty && !window.confirm("Discard unsaved changes?")) {
          // User aborted the close: Radix has already flipped its internal
          // open=false, so re-assert open=true on the next paint to override.
          requestAnimationFrame(() => setForceOpen(true))
          return
        }
        setForceOpen(false)
        onClose()
      }}
    >
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{slug} · review cuts.json</DrawerTitle>
        </DrawerHeader>

        {error && (
          <div className="mx-4 mb-2 px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400">
            {error}
          </div>
        )}

        <div className="px-4 pb-4 overflow-auto max-h-[60vh] space-y-2">
          {!loaded && !error && <div className="text-muted-foreground text-sm">Loading…</div>}
          {loaded && draft && draft.scenes.map((s, i) => {
            const thumb = loaded.thumbs[String(s.id)]
            const isLast = i === draft.scenes.length - 1
            return (
              <div key={s.id} className="flex gap-3 p-2 rounded-md border border-border bg-card">
                {thumb ? (
                  <img
                    src={api.workspaces.thumbUrl(slug!, thumb)}
                    alt=""
                    className="w-32 h-[4.5rem] rounded object-cover bg-muted ring-1 ring-border flex-shrink-0"
                    loading="lazy"
                  />
                ) : (
                  <div className="w-32 h-[4.5rem] rounded bg-muted ring-1 ring-border flex-shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="font-mono text-xs text-muted-foreground">
                    #{s.id.toString().padStart(3, "0")}  ·  {s.start} – {s.end}
                  </div>
                  <div className="mt-1.5 flex items-center gap-3 flex-wrap">
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox checked={s.keep}
                                onCheckedChange={(c) => update(i, { keep: !!c })} />
                      keep
                    </label>
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox
                        checked={s.merge_with_next}
                        disabled={isLast}
                        onCheckedChange={(c) => update(i, { merge_with_next: !!c })}
                      />
                      merge with next
                    </label>
                    <Input
                      placeholder="label (optional)"
                      value={s.label ?? ""}
                      onChange={(e) => update(i, { label: e.target.value || null })}
                      className="h-7 w-48 text-sm"
                    />
                  </div>
                </div>
              </div>
            )
          })}
        </div>

        <DrawerFooter className="flex-row justify-end gap-2">
          <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel</Button>
          <Button variant="secondary" onClick={save} disabled={busy || !isDirty}>
            Save
          </Button>
          <Button onClick={saveAndContinue} disabled={busy}>
            Save & continue
          </Button>
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}
