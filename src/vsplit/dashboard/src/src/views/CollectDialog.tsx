import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"
import { api } from "@/lib/api"

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type Preview = {
  collected: number
  from_workspaces: number
  skipped_not_done: number
  skipped_no_output: number
  skipped_existing: number
  destination: string
  mode: string
}

const MODES = ["copy", "move", "link"] as const

export function CollectDialog({ open, onOpenChange }: Props) {
  // `null` for dest/mode means "use the configured default". We fetch the
  // dry-run result with no body so the server resolves both — and then we
  // populate the form fields from the response so the user can see (and tweak)
  // what's about to happen.
  const [dest, setDest] = useState<string>("")
  const [mode, setMode] = useState<string>("copy")
  const [force, setForce] = useState<boolean>(false)
  const [preview, setPreview] = useState<Preview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [committing, setCommitting] = useState(false)

  // Run a dry-run whenever the dialog opens, or when any of the inputs change.
  useEffect(() => {
    if (!open) return
    let cancelled = false
    setBusy(true)
    setError(null)
    const body: { dest?: string; mode?: string; force?: boolean; dry_run: boolean } = {
      dry_run: true, force,
    }
    if (dest.trim()) body.dest = dest.trim()
    if (mode) body.mode = mode
    api.batch.collect(body)
      .then((r) => {
        if (cancelled) return
        setPreview(r)
        // Seed form fields from the first response so the user sees what the
        // server actually resolved (configured destination + mode).
        if (!dest.trim()) setDest(r.destination)
        if (!mode) setMode(r.mode)
      })
      .catch((e) => { if (!cancelled) setError(String(e.message ?? e)) })
      .finally(() => { if (!cancelled) setBusy(false) })
    return () => { cancelled = true }
  // We intentionally do NOT include `dest` in deps for the initial open —
  // the initial render runs with empty dest and the response seeds it.
  // After that, mode/force changes refresh; explicit dest edits refresh too.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, mode, force, dest])

  const commit = async () => {
    setCommitting(true)
    setError(null)
    try {
      const body: { dest?: string; mode?: string; force?: boolean } = { force }
      if (dest.trim()) body.dest = dest.trim()
      if (mode) body.mode = mode
      const r = await api.batch.collect(body)
      const lines = [
        `Collected ${r.collected} file${r.collected === 1 ? "" : "s"} from ${r.from_workspaces} workspace${r.from_workspaces === 1 ? "" : "s"}.`,
        `→ ${r.destination}`,
      ]
      const skips: string[] = []
      if (r.skipped_not_done) skips.push(`${r.skipped_not_done} not done`)
      if (r.skipped_no_output) skips.push(`${r.skipped_no_output} no output`)
      if (r.skipped_existing) skips.push(`${r.skipped_existing} already at destination`)
      if (skips.length) lines.push(`Skipped: ${skips.join(", ")}`)
      alert(lines.join("\n"))
      onOpenChange(false)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setCommitting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!committing) onOpenChange(o) }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Collect outputs</DialogTitle>
          <DialogDescription>
            Gather every split=done workspace's clips into one folder. Preview shows what would happen with the current settings.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div>
            <label className="text-xs text-muted-foreground block mb-1">Destination</label>
            <Input
              value={dest}
              onChange={(e) => setDest(e.target.value)}
              placeholder="(loading…)"
            />
          </div>
          <div className="flex items-center gap-3">
            <div className="flex-1">
              <label className="text-xs text-muted-foreground block mb-1">Mode</label>
              <Select value={mode} onValueChange={setMode}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {MODES.map((m) => <SelectItem key={m} value={m}>{m}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            <label className="flex items-center gap-2 text-sm pt-5 cursor-pointer">
              <Checkbox checked={force} onCheckedChange={(c) => setForce(!!c)} />
              <span>Overwrite existing</span>
            </label>
          </div>

          {error && (
            <div className="px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400">
              {error}
            </div>
          )}

          <div className="rounded-md border border-border bg-muted/30 p-3 text-sm space-y-1">
            {busy && !preview && <div className="text-muted-foreground">Calculating preview…</div>}
            {preview && (
              <>
                <div>
                  <span className="text-foreground font-medium">{preview.collected}</span>
                  {" "}file{preview.collected === 1 ? "" : "s"} would be{" "}
                  <span className="text-foreground font-medium">{mode === "copy" ? "copied" : mode === "move" ? "moved" : "linked"}</span>
                  {" "}from{" "}
                  <span className="text-foreground font-medium">{preview.from_workspaces}</span>
                  {" "}workspace{preview.from_workspaces === 1 ? "" : "s"}.
                </div>
                {(preview.skipped_existing || preview.skipped_not_done || preview.skipped_no_output) ? (
                  <ul className="text-xs text-muted-foreground list-disc list-inside">
                    {preview.skipped_existing > 0 && (
                      <li>
                        {preview.skipped_existing} already at destination
                        {!force && " (check 'Overwrite existing' to replace)"}
                      </li>
                    )}
                    {preview.skipped_not_done > 0 && (
                      <li>{preview.skipped_not_done} workspace{preview.skipped_not_done === 1 ? "" : "s"} skipped — split not done</li>
                    )}
                    {preview.skipped_no_output > 0 && (
                      <li>{preview.skipped_no_output} workspace{preview.skipped_no_output === 1 ? "" : "s"} skipped — no output files</li>
                    )}
                  </ul>
                ) : null}
                {busy && <div className="text-xs text-muted-foreground italic">refreshing…</div>}
              </>
            )}
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={committing}>
            Cancel
          </Button>
          <Button onClick={commit} disabled={committing || !preview || preview.collected === 0}>
            {committing ? "Collecting…" : `Collect ${preview?.collected ?? ""}`.trim()}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
