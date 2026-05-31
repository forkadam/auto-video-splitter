import { useEffect, useMemo, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle, DrawerFooter,
} from "@/components/ui/drawer"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { api, type CutsBody } from "@/lib/api"
import { TC_RE, parseTc, formatTc, snapPreceding } from "@/lib/timecode"

type Props = {
  slug: string | null
  onClose: () => void
  /** When set, the drawer's portal renders inside this element instead of <body>,
      so it stays confined to the workspace-list column and doesn't overlap the
      preview pane on the right. */
  container?: HTMLElement | null
}

type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
  keyframes: number[]
}

export function CutsDrawer({ slug, onClose, container }: Props) {
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
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs, keyframes: r.keyframes })
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

  const keepCount = useMemo(
    () => draft?.scenes.reduce((n, s) => n + (s.keep ? 1 : 0), 0) ?? 0,
    [draft],
  )
  const keepAllState: boolean | "indeterminate" = (() => {
    if (!draft || draft.scenes.length === 0) return false
    if (keepCount === draft.scenes.length) return true
    if (keepCount === 0) return false
    return "indeterminate"
  })()
  const toggleKeepAll = () => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    const allKept = next.scenes.every((s) => s.keep)
    for (const s of next.scenes) s.keep = !allKept
    return next
  })

  const sceneErrors = useMemo<Record<number, string | null>>(() => {
    const errs: Record<number, string | null> = {}
    for (const s of draft?.scenes ?? []) {
      if (!TC_RE.test(s.start) || !TC_RE.test(s.end)) {
        errs[s.id] = "timestamps must be HH:MM:SS.fff"
      } else if (parseTc(s.start) >= parseTc(s.end)) {
        errs[s.id] = "end must be after start"
      } else {
        errs[s.id] = null
      }
    }
    return errs
  }, [draft])

  const hasErrors = useMemo(
    () => Object.values(sceneErrors).some(Boolean),
    [sceneErrors],
  )

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
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs, keyframes: loaded.keyframes })
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
      shouldScaleBackground={false}
      // Non-modal: this drawer lives beside the preview pane, so the rest of the
      // UI (preview video controls, list) must stay interactive. With modal the
      // default, vaul sets body pointer-events:none and treats any outside click
      // as a dismissal — clicking the preview would close the drawer and fire the
      // unsaved-changes prompt. modal={false} keeps the background interactive,
      // prevents outside-click dismissal, and drops the scroll-locking overlay.
      // Closing stays available via Cancel / Save & continue / Esc / drag-down.
      modal={false}
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
      <DrawerContent container={container ?? null}>
        <DrawerHeader>
          <DrawerTitle>{slug} · review cuts.json</DrawerTitle>
        </DrawerHeader>

        {error && (
          <div className="mx-4 mb-2 px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400">
            {error}
          </div>
        )}

        {draft && draft.scenes.length > 0 && (
          <label className="mx-4 mb-2 flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none w-fit">
            <Checkbox
              checked={keepAllState}
              onCheckedChange={toggleKeepAll}
              aria-label="Toggle keep on all scenes"
            />
            <span>
              {keepCount === 0
                ? `Keep all (${draft.scenes.length})`
                : keepCount === draft.scenes.length
                ? `Keep none (${draft.scenes.length})`
                : `${keepCount} of ${draft.scenes.length} kept`}
            </span>
          </label>
        )}

        <div className="px-4 pb-4 overflow-auto max-h-[60vh] space-y-2">
          {!loaded && !error && <div className="text-muted-foreground text-sm">Loading…</div>}
          {slug && loaded && draft && draft.scenes.map((s, i) => {
            const thumb = loaded.thumbs[String(s.id)]
            const isLast = i === draft.scenes.length - 1
            const startOk = TC_RE.test(s.start)
            const endOk = TC_RE.test(s.end)
            const snapped = startOk && loaded.keyframes.length > 0
              ? snapPreceding(parseTc(s.start), loaded.keyframes)
              : null
            const drift = snapped !== null ? parseTc(s.start) - snapped : null
            const err = sceneErrors[s.id]
            return (
              <div key={s.id} className="flex gap-3 p-2 rounded-md border border-border bg-card">
                {thumb ? (
                  <img
                    src={api.workspaces.thumbUrl(slug, thumb)}
                    alt=""
                    className="w-32 h-[4.5rem] rounded object-cover bg-muted ring-1 ring-border flex-shrink-0"
                    loading="lazy"
                  />
                ) : (
                  <div className="w-32 h-[4.5rem] rounded bg-muted ring-1 ring-border flex-shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 font-mono text-xs text-muted-foreground">
                    <span className="shrink-0">#{s.id.toString().padStart(3, "0")}</span>
                    <Input
                      value={s.start}
                      aria-label={`scene ${s.id} start`}
                      onChange={(e) => update(i, { start: e.target.value })}
                      className={`h-7 w-32 text-xs font-mono ${startOk ? "" : "border-red-500/60"}`}
                    />
                    <span>–</span>
                    <Input
                      value={s.end}
                      aria-label={`scene ${s.id} end`}
                      onChange={(e) => update(i, { end: e.target.value })}
                      className={`h-7 w-32 text-xs font-mono ${endOk ? "" : "border-red-500/60"}`}
                    />
                    {drift !== null && (
                      <span className="shrink-0 text-muted-foreground/70">
                        {drift < 0.001
                          ? "⤷ on keyframe"
                          : `⤷ cuts at ${formatTc(snapped!)} (−${drift.toFixed(2)}s)`}
                      </span>
                    )}
                  </div>
                  {err && <div className="mt-1 text-xs text-red-400">{err}</div>}
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
          <Button variant="secondary" onClick={save} disabled={busy || !isDirty || hasErrors}>
            Save
          </Button>
          <Button onClick={saveAndContinue} disabled={busy || hasErrors}>
            Save & continue
          </Button>
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}
