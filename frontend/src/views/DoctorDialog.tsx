import { useCallback, useEffect, useState } from "react"
import { CheckCircle2, AlertTriangle, RefreshCw, FolderSearch } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { api, type DoctorReport } from "@/lib/api"
import {
  getAppConfigNative,
  isTauriRuntime,
  pickFileNative,
  repairCliShimNative,
  setAppConfigNative,
} from "@/lib/bootstrap"

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function StatusBadge({ ok }: { ok: boolean }) {
  return ok ? (
    <span className="inline-flex items-center gap-1 text-xs font-medium text-emerald-400">
      <CheckCircle2 className="h-3.5 w-3.5" /> OK
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 text-xs font-medium text-red-400">
      <AlertTriangle className="h-3.5 w-3.5" /> Missing
    </span>
  )
}

export function DoctorDialog({ open, onOpenChange }: Props) {
  const [report, setReport] = useState<DoctorReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const inTauri = isTauriRuntime()

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const r = await api.doctor()
      setReport(r)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    api.doctor()
      .then((r) => {
        if (cancelled) return
        setReport(r)
        setError(null)
        setNotice(null)
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setError(e instanceof Error ? e.message : String(e))
      })
    return () => { cancelled = true }
  }, [open])

  const pickTool = async (which: "ffmpeg_path" | "ffprobe_path") => {
    const picked = await pickFileNative(`Select ${which === "ffmpeg_path" ? "ffmpeg" : "ffprobe"} binary`)
    if (!picked) return
    const cfg = (await getAppConfigNative()) ?? {
      recent_folders: [],
      last_folder: null,
      max_recent_folders: 10,
      ffmpeg_path: null,
      ffprobe_path: null,
      video_player: null,
      wayland_disable_dmabuf: false,
    }
    await setAppConfigNative({ ...cfg, [which]: picked })
    setNotice(`Saved ${which} to app.toml. Re-checking…`)
    await load()
  }

  const repairShim = async () => {
    const msg = await repairCliShimNative()
    if (msg) setNotice(msg)
    await load()
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center justify-between pr-6">
            <span>System Diagnostics (Doctor)</span>
            <Button variant="ghost" size="sm" onClick={() => void load()} disabled={loading}>
              <RefreshCw className={`h-3.5 w-3.5 mr-1.5 ${loading ? "animate-spin" : ""}`} />
              Re-check
            </Button>
          </DialogTitle>
        </DialogHeader>

        {error && (
          <div className="p-2.5 text-xs rounded-md bg-red-500/10 border border-red-500/30 text-red-300">
            {error}
          </div>
        )}
        {notice && (
          <div className="p-2.5 text-xs rounded-md bg-emerald-500/10 border border-emerald-500/30 text-emerald-300">
            {notice}
          </div>
        )}

        {report && (
          <div className="space-y-3 text-xs">
            <div className="rounded-md border border-border p-3 space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-semibold">FFmpeg</span>
                <div className="flex items-center gap-2">
                  <StatusBadge ok={report.ffmpeg.ok} />
                  {inTauri && (
                    <Button size="sm" variant="outline" className="h-6 px-2 text-xs" onClick={() => void pickTool("ffmpeg_path")}>
                      <FolderSearch className="h-3 w-3 mr-1" /> Browse
                    </Button>
                  )}
                </div>
              </div>
              <div className="font-mono text-muted-foreground break-all">
                {report.ffmpeg.path ?? "Not found"} {report.ffmpeg.version ? `(v${report.ffmpeg.version})` : ""}
              </div>
              {report.ffmpeg.hint && (
                <div className="text-amber-300">{report.ffmpeg.hint}</div>
              )}
            </div>

            <div className="rounded-md border border-border p-3 space-y-2">
              <div className="flex items-center justify-between">
                <span className="font-semibold">FFprobe</span>
                <div className="flex items-center gap-2">
                  <StatusBadge ok={report.ffprobe.ok} />
                  {inTauri && (
                    <Button size="sm" variant="outline" className="h-6 px-2 text-xs" onClick={() => void pickTool("ffprobe_path")}>
                      <FolderSearch className="h-3 w-3 mr-1" /> Browse
                    </Button>
                  )}
                </div>
              </div>
              <div className="font-mono text-muted-foreground break-all">
                {report.ffprobe.path ?? "Not found"} {report.ffprobe.version ? `(v${report.ffprobe.version})` : ""}
              </div>
            </div>

            <div className="rounded-md border border-border p-3 flex items-center justify-between">
              <span className="font-semibold">Encoders</span>
              <span className="font-mono">
                libx264: {report.encoders.libx264 ? "yes" : "no"} · aac: {report.encoders.aac ? "yes" : "no"}
              </span>
            </div>

            {report.gstreamer.applicable && (
              <div className="rounded-md border border-border p-3 space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className="font-semibold">Linux WebKitGTK / AppImage</span>
                  <StatusBadge ok={report.gstreamer.ok} />
                </div>
                <div className="font-mono text-muted-foreground">
                  avdec_h264: {report.gstreamer.avdec_h264 ? "yes" : "no"} · fuse2: {report.gstreamer.fuse2 ? "yes" : "no"}
                </div>
                {report.gstreamer.hint && (
                  <code className="block bg-muted p-1.5 rounded text-[11px]">{report.gstreamer.hint}</code>
                )}
              </div>
            )}

            <div className="rounded-md border border-border p-3 space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="font-semibold">CLI &amp; Platform</span>
                {inTauri && report.platform.os === "linux" && (
                  <Button size="sm" variant="outline" className="h-6 px-2 text-xs" onClick={() => void repairShim()}>
                    Repair CLI shim
                  </Button>
                )}
              </div>
              <div className="font-mono text-muted-foreground">
                v{report.cli_shim.version} · {report.platform.os}/{report.platform.arch}
                {report.platform.wayland_session ? " · Wayland" : ""}
              </div>
              {report.path_shadowing.warning && (
                <div className="text-amber-300">{report.path_shadowing.warning}</div>
              )}
              <div className="text-muted-foreground">
                Active workspace locks: {Object.keys(report.locks).length}
              </div>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
