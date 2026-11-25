import { useCallback, useEffect, useMemo, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle, DrawerFooter,
} from "@/components/ui/drawer"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { api, ApiHttpError, type CutsBody } from "@/lib/api"
import { TC_RE, parseTc, formatTc, snapPreceding } from "@/lib/timecode"

type Props = {
  slug: string | null
  externalCutsEvent: { slug: string; hash: string } | null
  onClose: () => void
  container?: HTMLElement | null
}

type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
  keyframes: number[]
  detectedCount: number
  durationSeconds: number | null
}

export function CutsDrawer({ slug, externalCutsEvent, onClose, container }: Props) {
  const [loaded, setLoaded] = useState<(Loaded & { slug: string }) | null>(null)
  const [draft, setDraft] = useState<CutsBody | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saveConflictHash, setSaveConflictHash] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [keepOpenNonce, setKeepOpenNonce] = useState(0)

  const activeLoaded = slug && loaded?.slug === slug ? loaded : null
  const activeDraft = slug && loaded?.slug === slug ? draft : null

  const fetchFromServer = useCallback(async (targetSlug: string) => {
    const r = await api.workspaces.cuts.get(targetSlug)
    setLoaded({
      slug: targetSlug,
      cuts: r.cuts,
      hash: r.hash,
      thumbs: r.scene_thumbs,
      keyframes: r.keyframes,
      detectedCount: r.detected_count,
      durationSeconds: r.duration_seconds,
    })
    setDraft(structuredClone(r.cuts))
    setSaveConflictHash(null)
    setError(null)
  }, [])

  useEffect(() => {
    if (!slug) return
    let cancelled = false
    api.workspaces.cuts.get(slug)
      .then((r) => {
        if (cancelled) return
        setLoaded({
          slug,
          cuts: r.cuts,
          hash: r.hash,
          thumbs: r.scene_thumbs,
          keyframes: r.keyframes,
          detectedCount: r.detected_count,
          durationSeconds: r.duration_seconds,
        })
        setDraft(structuredClone(r.cuts))
        setSaveConflictHash(null)
        setError(null)
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setError(e instanceof Error ? e.message : String(e))
      })
    return () => { cancelled = true }
  }, [slug])

  const isDirty = useMemo(() => {
    if (!activeLoaded || !activeDraft) return false
    return JSON.stringify(activeLoaded.cuts) !== JSON.stringify(activeDraft)
  }, [activeLoaded, activeDraft])

  // Auto-reload when an external cuts_changed SSE event arrives while the editor is clean.
  useEffect(() => {
    if (!slug || !activeLoaded || !externalCutsEvent) return
    if (externalCutsEvent.slug !== slug) return
    if (externalCutsEvent.hash === activeLoaded.hash) return
    if (isDirty) return
    let cancelled = false
    api.workspaces.cuts.get(slug)
      .then((r) => {
        if (cancelled) return
        setLoaded({
          slug,
          cuts: r.cuts,
          hash: r.hash,
          thumbs: r.scene_thumbs,
          keyframes: r.keyframes,
          detectedCount: r.detected_count,
          durationSeconds: r.duration_seconds,
        })
        setDraft(structuredClone(r.cuts))
        setSaveConflictHash(null)
        setError(null)
      })
      .catch(() => { /* ignore transient reload error */ })
    return () => { cancelled = true }
  }, [externalCutsEvent, slug, activeLoaded, isDirty])

  const externalConflictHash =
    slug && activeLoaded && externalCutsEvent && externalCutsEvent.slug === slug
    && externalCutsEvent.hash !== activeLoaded.hash && isDirty
      ? externalCutsEvent.hash
      : null
  const conflictHash = saveConflictHash ?? externalConflictHash

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

  const mergeableCount = Math.max(0, (draft?.scenes.length ?? 0) - 1)
  const mergedCount = useMemo(
    () => draft?.scenes.slice(0, -1).reduce((n, s) => n + (s.merge_with_next ? 1 : 0), 0) ?? 0,
    [draft],
  )
  const mergeAllState: boolean | "indeterminate" = (() => {
    if (mergeableCount === 0) return false
    if (mergedCount === mergeableCount) return true
    if (mergedCount === 0) return false
    return "indeterminate"
  })()
  const toggleMergeAll = () => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    const allMerged = next.scenes.slice(0, -1).every((s) => s.merge_with_next)
    next.scenes.forEach((s, i) => {
      if (i < next.scenes.length - 1) s.merge_with_next = !allMerged
    })
    return next
  })

  const setKeep = (idx: number, keep: boolean) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    const isLast = idx === next.scenes.length - 1
    next.scenes[idx] = {
      ...next.scenes[idx],
      keep,
      merge_with_next: isLast ? next.scenes[idx].merge_with_next : keep,
    }
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

  const nextSceneId = (scenes: CutsBody["scenes"]) =>
    scenes.length ? Math.max(...scenes.map((s) => s.id)) + 1 : 1

  const addScene = (end: string) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    next.scenes.push({
      id: nextSceneId(next.scenes),
      start: "00:00:00.000",
      end,
      keep: true,
      label: null,
      merge_with_next: false,
    })
    return next
  })

  const removeScene = (idx: number) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    next.scenes.splice(idx, 1)
    return next
  })

  const canEditRows = !!activeLoaded && activeLoaded.detectedCount === 0
  const wholeClipEnd =
    activeLoaded?.durationSeconds != null ? formatTc(activeLoaded.durationSeconds) : null

  const cancel = () => {
    if (isDirty && !window.confirm("Discard unsaved changes?")) return
    onClose()
  }

  const save = async (): Promise<boolean> => {
    if (!slug || !activeLoaded || !activeDraft) return false
    setBusy(true)
    setError(null)
    try {
      const r = await api.workspaces.cuts.put(slug, {
        cuts: activeDraft, expected_hash: activeLoaded.hash,
      })
      setLoaded({
        slug,
        cuts: structuredClone(activeDraft),
        hash: r.hash,
        thumbs: activeLoaded.thumbs,
        keyframes: activeLoaded.keyframes,
        detectedCount: activeLoaded.detectedCount,
        durationSeconds: activeLoaded.durationSeconds,
      })
      setSaveConflictHash(null)
      return true
    } catch (e: unknown) {
      if (e instanceof ApiHttpError && e.status === 409) {
        const currentHash = typeof e.payload.current_hash === "string" ? e.payload.current_hash : null
        if (currentHash) {
          setSaveConflictHash(currentHash)
          setError("cuts.json changed on disk — choose Reload or Keep Mine below.")
          return false
        }
      }
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      return false
    } finally {
      setBusy(false)
    }
  }

  const keepMine = () => {
    if (!activeLoaded || !conflictHash) return
    setLoaded({ ...activeLoaded, hash: conflictHash })
    setSaveConflictHash(null)
    setError(null)
  }

  const reloadFromDisk = () => {
    if (!slug) return
    void fetchFromServer(slug)
  }

  const saveAndContinue = async () => {
    if (!slug) return
    if (isDirty) {
      const ok = await save()
      if (!ok) return
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
      key={keepOpenNonce}
      open={slug !== null}
      shouldScaleBackground={false}
      modal={false}
      onOpenChange={(o) => {
        if (o) return
        if (isDirty && !window.confirm("Discard unsaved changes?")) {
          setKeepOpenNonce((n) => n + 1)
          return
        }
        onClose()
      }}
    >
      <DrawerContent container={container ?? null}>
        <DrawerHeader>
          <DrawerTitle>{slug} · review cuts.json</DrawerTitle>
        </DrawerHeader>

        {conflictHash && (
          <div className="mx-4 mb-2 px-3 py-2 text-xs rounded-md bg-amber-500/15 border border-amber-500/40 text-amber-200 flex items-center justify-between gap-2">
            <span>External edit detected in decisions/cuts.json.</span>
            <div className="flex gap-1.5 shrink-0">
              <Button size="sm" variant="outline" className="h-6 px-2 text-xs" onClick={reloadFromDisk}>
                Reload
              </Button>
              <Button size="sm" variant="secondary" className="h-6 px-2 text-xs" onClick={keepMine}>
                Keep Mine
              </Button>
            </div>
          </div>
        )}

        {error && (
          <div className="mx-4 mb-2 px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400">
            {error}
          </div>
        )}

        {draft && draft.scenes.length > 0 && (
          <div className="mx-4 mb-2 flex items-center gap-4 text-xs text-muted-foreground select-none">
            <label className="flex items-center gap-2 cursor-pointer w-fit">
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
            {mergeableCount > 0 && (
              <label className="flex items-center gap-2 cursor-pointer w-fit">
                <Checkbox
                  checked={mergeAllState}
                  onCheckedChange={toggleMergeAll}
                  aria-label="Toggle merge on all scenes"
                />
                <span>
                  {mergedCount === 0
                    ? `Merge all (${mergeableCount})`
                    : mergedCount === mergeableCount
                    ? `Merge none (${mergeableCount})`
                    : `${mergedCount} of ${mergeableCount} merged`}
                </span>
              </label>
            )}
          </div>
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
                                onCheckedChange={(c) => setKeep(i, !!c)} />
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
                {canEditRows && (
                  <button
                    type="button"
                    onClick={() => removeScene(i)}
                    aria-label={`remove scene ${s.id}`}
                    className="self-start text-muted-foreground hover:text-red-400 leading-none px-1"
                  >
                    ✕
                  </button>
                )}
              </div>
            )
          })}
          {canEditRows && draft && draft.scenes.length === 0 && (
            <div className="text-sm text-muted-foreground space-y-3 py-2">
              <p>No scenes were detected. Cut the whole clip, or add time ranges manually.</p>
              <div className="flex gap-2">
                <Button size="sm" onClick={() => addScene(wholeClipEnd!)} disabled={wholeClipEnd === null}>
                  Cut whole clip
                </Button>
                <Button size="sm" variant="outline" onClick={() => addScene(wholeClipEnd ?? "00:00:00.000")}>
                  + Add range
                </Button>
              </div>
            </div>
          )}
          {canEditRows && draft && draft.scenes.length > 0 && (
            <Button
              size="sm"
              variant="outline"
              className="w-fit"
              onClick={() => addScene(wholeClipEnd ?? "00:00:00.000")}
            >
              + Add range
            </Button>
          )}
        </div>

        <DrawerFooter className="flex-row justify-end gap-2">
          <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel</Button>
          <Button variant="secondary" onClick={() => void save()} disabled={busy || !isDirty || hasErrors || !!conflictHash}>
            Save
          </Button>
          <Button onClick={() => void saveAndContinue()} disabled={busy || hasErrors || keepCount === 0 || !!conflictHash}>
            Save &amp; continue
          </Button>
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}
