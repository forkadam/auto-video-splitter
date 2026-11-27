import { useEffect, useId, useRef, useState } from "react"
import { ArrowRight, ChevronDown, ChevronRight, AlertTriangle } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import {
  api,
  type RenameReport,
  type RenameRequest,
  type RenameSkippedItem,
} from "@/lib/api"

type Props = {
  open: boolean
  workingRoot: string | null
  onOpenChange: (open: boolean) => void
  onBeforeMutateWorkspaces: (slugs: string[]) => void
  onRenamed: (report: RenameReport) => void
}

type BodyProps = {
  committing: boolean
  onCommittingChange: (committing: boolean) => void
  onClose: () => void
  onBeforeMutateWorkspaces: (slugs: string[]) => void
  onRenamed: (report: RenameReport) => void
}

const DEFAULT_FORMAT = "%Y-%m-%d %H-%M-%S"

function formatSourceTag(tag: string): string {
  return tag === "com.apple.quicktime.creationdate" ? "QuickTime" : tag
}

function formatSkipReason(reason: RenameSkippedItem["reason"]): string {
  switch (reason) {
    case "already_named":
      return "Already named"
    case "no_creation_date":
      return "No creation date"
    case "workspace_locked":
      return "Workspace locked"
    case "error":
      return "Error"
  }
}

function RenameDialogBody({
  committing,
  onCommittingChange,
  onClose,
  onBeforeMutateWorkspaces,
  onRenamed,
}: BodyProps) {
  const idPrefix = useId()
  const globId = `${idPrefix}-glob`
  const formatId = `${idPrefix}-format`

  const [glob, setGlob] = useState<string>("")
  const [format, setFormat] = useState<string>(DEFAULT_FORMAT)
  const [recursive, setRecursive] = useState<boolean>(false)
  const [preview, setPreview] = useState<RenameReport | null>(null)
  const [previewKey, setPreviewKey] = useState<string>("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showSkipped, setShowSkipped] = useState(false)

  const timerRef = useRef<number | null>(null)
  const requestSeqRef = useRef(0)

  const normalizedGlob = glob.trim()
  const currentKey = JSON.stringify([normalizedGlob, recursive, format])
  const isStale = preview !== null && previewKey !== currentKey
  const isRefreshing = busy || isStale

  useEffect(() => {
    const seq = ++requestSeqRef.current
    const requestKey = JSON.stringify([normalizedGlob, recursive, format])
    const body: RenameRequest = {
      glob: normalizedGlob ? normalizedGlob : undefined,
      recursive,
      format,
      dry_run: true,
    }
    const timer = window.setTimeout(() => {
      timerRef.current = null
      if (requestSeqRef.current !== seq) return
      setBusy(true)
      setError(null)
      api.rename(body)
        .then((report) => {
          if (requestSeqRef.current !== seq) return
          setPreview(report)
          setPreviewKey(requestKey)
        })
        .catch((e: unknown) => {
          if (requestSeqRef.current !== seq) return
          setPreview(null)
          setPreviewKey("")
          setError(e instanceof Error ? e.message : String(e))
        })
        .finally(() => {
          if (requestSeqRef.current === seq) setBusy(false)
        })
    }, 250)
    timerRef.current = timer

    return () => {
      if (requestSeqRef.current === seq) {
        requestSeqRef.current += 1
      }
      if (timerRef.current === timer) {
        timerRef.current = null
      }
      window.clearTimeout(timer)
    }
  }, [normalizedGlob, format, recursive])

  const commit = async () => {
    if (!preview || committing) return
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current)
      timerRef.current = null
    }
    requestSeqRef.current += 1
    setBusy(false)
    onCommittingChange(true)
    setError(null)
    try {
      const affectedSlugs = preview.renamed
        .map((item) => item.old_slug)
        .filter((slug): slug is string => slug !== null)
      if (affectedSlugs.length > 0) {
        onBeforeMutateWorkspaces(affectedSlugs)
      }
      const report = await api.rename({
        glob: normalizedGlob ? normalizedGlob : undefined,
        recursive,
        format,
        dry_run: false,
      })
      onRenamed(report)
      onClose()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      onCommittingChange(false)
    }
  }

  const skippedSummaryParts: string[] = []
  if (preview) {
    if (preview.counts.skipped_already_named > 0) {
      skippedSummaryParts.push(`${preview.counts.skipped_already_named} already named`)
    }
    if (preview.counts.skipped_no_date > 0) {
      skippedSummaryParts.push(`${preview.counts.skipped_no_date} no creation date`)
    }
    if (preview.counts.skipped_locked > 0) {
      skippedSummaryParts.push(`${preview.counts.skipped_locked} locked`)
    }
    if (preview.counts.skipped_error > 0) {
      skippedSummaryParts.push(`${preview.counts.skipped_error} error`)
    }
  }

  const hasDetails =
    preview !== null && (preview.skipped.length > 0 || preview.warnings.length > 0)

  const detailsLabelParts: string[] = []
  if (preview && preview.skipped.length > 0) {
    detailsLabelParts.push(`${preview.skipped.length} skipped`)
  }
  if (preview && preview.warnings.length > 0) {
    detailsLabelParts.push(
      `${preview.warnings.length} warning${preview.warnings.length === 1 ? "" : "s"}`,
    )
  }

  const confirmDisabled =
    committing ||
    isRefreshing ||
    !!error ||
    !preview ||
    preview.counts.renamed === 0

  return (
    <>
      <DialogHeader className="shrink-0">
        <DialogTitle>Rename videos by creation date</DialogTitle>
        <DialogDescription>
          Renames source video files on disk using embedded QuickTime or{" "}
          <span className="font-mono text-xs">creation_time</span> metadata while
          keeping <span className="font-mono text-xs">.vsplit/</span> workspaces in sync.
        </DialogDescription>
      </DialogHeader>

      <div className="space-y-3 min-h-0 flex-1 overflow-y-auto pr-1">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div>
            <label htmlFor={globId} className="text-xs text-muted-foreground block mb-1">
              Filename filter
            </label>
            <Input
              id={globId}
              value={glob}
              onChange={(e) => setGlob(e.target.value)}
              placeholder="All videos (e.g. IMG*)"
              disabled={committing}
            />
          </div>
          <div>
            <label htmlFor={formatId} className="text-xs text-muted-foreground block mb-1">
              Timestamp format
            </label>
            <Input
              id={formatId}
              value={format}
              onChange={(e) => setFormat(e.target.value)}
              placeholder={DEFAULT_FORMAT}
              className="font-mono text-xs"
              disabled={committing}
            />
            <p className="text-[11px] text-muted-foreground mt-1">
              Default: <span className="font-mono">%Y-%m-%d %H-%M-%S</span> (e.g. 2026-05-01 14-23-05)
            </p>
          </div>
        </div>

        <label className="flex items-center gap-2 text-sm cursor-pointer w-fit select-none">
          <Checkbox
            checked={recursive}
            onCheckedChange={(c) => setRecursive(!!c)}
            disabled={committing}
          />
          <span>Include subfolders</span>
        </label>

        {error && (
          <div
            role="alert"
            className="px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400"
          >
            {error}
          </div>
        )}

        {(preview || !error) && (
          <div className="rounded-md border border-border bg-muted/30 p-3 text-sm space-y-2.5">
            {!preview && (
              <div className="text-muted-foreground">Calculating preview…</div>
            )}

            {preview && (
              <>
                <div className="flex items-baseline justify-between gap-2 flex-wrap">
                  <div className="space-y-0.5">
                    <div>
                      <span className="text-foreground font-medium tabular-nums">
                        {preview.counts.renamed}
                      </span>{" "}
                      file{preview.counts.renamed === 1 ? "" : "s"} will be renamed
                      {preview.counts.workspaces_synced > 0 && (
                        <>
                          {" · "}
                          <span className="text-foreground font-medium tabular-nums">
                            {preview.counts.workspaces_synced}
                          </span>{" "}
                          workspace{preview.counts.workspaces_synced === 1 ? "" : "s"} synced
                        </>
                      )}
                    </div>
                    {skippedSummaryParts.length > 0 && (
                      <div className="text-xs text-muted-foreground">
                        Skipped: {skippedSummaryParts.join(" · ")}
                      </div>
                    )}
                  </div>
                  {isRefreshing && (
                    <div className="text-xs text-muted-foreground italic">refreshing…</div>
                  )}
                </div>

                {preview.renamed.length > 0 && (
                  <div className="max-h-60 overflow-y-auto rounded border border-border/70 bg-background/60 divide-y divide-border/50">
                    {preview.renamed.map((item) => (
                      <div
                        key={item.old_path}
                        className="px-2.5 py-2 text-xs space-y-1"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0 flex items-center gap-1.5 font-mono tabular-nums flex-wrap">
                            <span className="text-muted-foreground break-all">
                              {item.old_path}
                            </span>
                            <ArrowRight className="h-3 w-3 shrink-0 text-muted-foreground" />
                            <span className="text-foreground font-medium break-all">
                              {item.new_path}
                            </span>
                          </div>
                          <Badge
                            variant="secondary"
                            className="shrink-0 text-[10px] font-normal px-1.5 py-0"
                            title={item.source_tag}
                          >
                            {formatSourceTag(item.source_tag)}
                          </Badge>
                        </div>
                        {item.old_slug && (
                          <div className="text-[11px] text-muted-foreground font-mono flex items-center gap-1">
                            <span>workspace:</span>
                            <span>{item.old_slug}</span>
                            {item.new_slug && item.new_slug !== item.old_slug && (
                              <>
                                <ArrowRight className="h-2.5 w-2.5 shrink-0" />
                                <span className="text-foreground/90">{item.new_slug}</span>
                              </>
                            )}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}

                {hasDetails && (
                  <div className="pt-0.5">
                    <button
                      type="button"
                      aria-expanded={showSkipped}
                      onClick={() => setShowSkipped((s) => !s)}
                      className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
                    >
                      {showSkipped ? (
                        <ChevronDown className="h-3.5 w-3.5" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5" />
                      )}
                      <span>
                        {showSkipped ? "Hide" : "Show"} details ({detailsLabelParts.join(", ")})
                      </span>
                    </button>

                    {showSkipped && (
                      <div className="mt-2 max-h-44 overflow-y-auto space-y-2">
                        {preview.warnings.length > 0 && (
                          <div className="rounded border border-amber-500/30 bg-amber-500/10 p-2 space-y-1 text-xs text-amber-300">
                            {preview.warnings.map((w, idx) => (
                              <div key={idx} className="flex items-start gap-1.5">
                                <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
                                <span>{w}</span>
                              </div>
                            ))}
                          </div>
                        )}
                        {preview.skipped.length > 0 && (
                          <div className="rounded border border-border/70 bg-background/60 divide-y divide-border/50 text-xs">
                            {preview.skipped.map((item) => (
                              <div
                                key={`${item.path}-${item.reason}`}
                                className="px-2.5 py-1.5 flex items-start justify-between gap-2"
                              >
                                <div className="min-w-0">
                                  <div className="font-mono text-muted-foreground break-all">
                                    {item.path}
                                  </div>
                                  {item.detail && (
                                    <div className="text-[11px] text-muted-foreground/80 mt-0.5">
                                      {item.detail}
                                    </div>
                                  )}
                                </div>
                                <Badge
                                  variant="outline"
                                  className="shrink-0 text-[10px] font-normal px-1.5 py-0 text-muted-foreground"
                                >
                                  {formatSkipReason(item.reason)}
                                </Badge>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
        )}
      </div>

      <DialogFooter className="gap-2 sm:gap-2 shrink-0">
        <Button
          variant="ghost"
          onClick={onClose}
          disabled={committing}
        >
          Cancel
        </Button>
        <Button onClick={() => void commit()} disabled={confirmDisabled}>
          {committing
            ? "Renaming…"
            : preview
              ? `Rename ${preview.counts.renamed}`
              : "Rename"}
        </Button>
      </DialogFooter>
    </>
  )
}

export function RenameDialog({
  open,
  workingRoot,
  onOpenChange,
  onBeforeMutateWorkspaces,
  onRenamed,
}: Props) {
  const [committing, setCommitting] = useState(false)

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!committing) onOpenChange(nextOpen)
      }}
    >
      <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col">
        {open && (
          <RenameDialogBody
            key={workingRoot ?? ""}
            committing={committing}
            onCommittingChange={setCommitting}
            onClose={() => onOpenChange(false)}
            onBeforeMutateWorkspaces={onBeforeMutateWorkspaces}
            onRenamed={onRenamed}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}
