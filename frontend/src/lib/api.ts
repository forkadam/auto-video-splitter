import { getBootstrap } from "./bootstrap"

type Body = Record<string, unknown> | undefined

export class ApiHttpError extends Error {
  status: number
  payload: Record<string, unknown>
  constructor(status: number, message: string, payload: Record<string, unknown> = {}) {
    super(message)
    this.status = status
    this.payload = payload
  }
}

function authHeaders(extra?: Record<string, string>): Record<string, string> {
  const { token } = getBootstrap()
  const headers: Record<string, string> = { ...extra }
  if (token) {
    headers["X-Vsplit-Token"] = token
  }
  return headers
}

async function jsonRequest<T>(
  method: "GET" | "POST" | "PUT",
  path: string,
  body?: Body,
): Promise<T> {
  const { baseUrl } = getBootstrap()
  const res = await fetch(`${baseUrl}${path}`, {
    method,
    headers: authHeaders(body ? { "Content-Type": "application/json" } : undefined),
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const payload = (await res.json().catch(() => ({}))) as Record<string, unknown>
    const detail = typeof payload?.detail === "string"
      ? payload.detail
      : `${res.status} ${res.statusText}`
    throw new ApiHttpError(res.status, detail, payload)
  }
  return res.json() as Promise<T>
}

export type LockLease = {
  pid: number
  actor: "desktop" | "cli"
  stage: string
  run_id: string
  started_at: string
  version: string
}

export type CutsScene = {
  id: number
  start: string
  end: string
  keep: boolean
  label: string | null
  merge_with_next: boolean
}

export type CutsBody = {
  schema_version: number
  scenes: CutsScene[]
}

export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
  keyframes: number[]
  detected_count: number
  duration_seconds: number | null
}

export type Workspace = {
  slug: string
  source: string
  stages: Record<string, string>
  any_failed: boolean
  any_awaiting: boolean
  all_done: boolean
  first_thumb: string | null
  scenes_found: number | null
  output_count: number
  ignored: boolean
  duration_seconds: number | null
  locked_by: LockLease | null
}

export type ScanReport = {
  adopted: Array<{ slug: string; relative_path: string; size_bytes: number }>
  skipped: number
  relocated: Array<{ slug: string; old_path: string; new_path: string }>
  pending_relocations: Array<{ slug: string; old_path: string; new_path: string }>
  warnings: string[]
  total_workspaces: number
  dry_run: boolean
}

export type DoctorReport = {
  ok: boolean
  ffmpeg: { ok: boolean; path: string | null; source: string | null; version: string | null; hint: string | null }
  ffprobe: { ok: boolean; path: string | null; source: string | null; version: string | null; hint: string | null }
  encoders: { libx264: boolean; aac: boolean }
  gstreamer: { applicable: boolean; ok: boolean; avdec_h264: boolean; fuse2: boolean; hint: string | null }
  cli_shim: { current_exe: string | null; version: string; shim_path: string | null; shim_exists: boolean; local_bin_on_path: boolean }
  path_shadowing: { shadowed: boolean; entries: string[]; warning: string | null }
  locks: Record<string, LockLease>
  platform: { os: string; arch: string; wayland_session: boolean; wayland_disable_dmabuf: boolean }
}

export const api = {
  project: {
    open: (path: string) =>
      jsonRequest<{ working_root: string; recent: string[] }>("POST", "/api/project/open", { path }),
    recent: () =>
      jsonRequest<{ working_root: string | null; recent: string[] }>("GET", "/api/project/recent"),
    removeRecent: (path: string) =>
      jsonRequest<{ working_root: string | null; recent: string[] }>("POST", "/api/project/recent/remove", { path }),
    clearRecent: () =>
      jsonRequest<{ working_root: string | null; recent: string[] }>("POST", "/api/project/recent/remove", { clear_all: true }),
    adoptFile: (path: string) =>
      jsonRequest<{ slug: string; working_root: string; recent: string[] }>("POST", "/api/project/adopt-file", { path }),
  },
  scan: (body: { path?: string; recursive?: boolean; relocate_policy?: "auto" | "skip" | "report"; dry_run?: boolean } = {}) =>
    jsonRequest<ScanReport>("POST", "/api/scan", body),
  jobs: {
    cancel: (slug?: string) =>
      jsonRequest<{ cancelled: number }>("POST", "/api/jobs/cancel", slug ? { slug } : {}),
    active: () =>
      jsonRequest<{ active_slugs: string[]; count: number }>("GET", "/api/jobs/active"),
  },
  doctor: () => jsonRequest<DoctorReport>("GET", "/api/doctor"),
  workspaces: {
    list: () => jsonRequest<{ workspaces: Workspace[] }>("GET", "/api/workspaces"),
    detail: (slug: string) =>
      jsonRequest<Workspace & { manifest: unknown }>("GET", `/api/workspaces/${slug}`),
    log: async (slug: string, stage: string) => {
      const { baseUrl } = getBootstrap()
      const res = await fetch(`${baseUrl}/api/workspaces/${slug}/logs/${stage}`, {
        headers: authHeaders(),
      })
      if (!res.ok) {
        throw new Error(`${res.status} ${res.statusText}`)
      }
      return res.text()
    },
    run: (slug: string, body: { only?: string; from?: string; through?: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/run`, body),
    reset: (slug: string, body: { from: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/reset`, body),
    clean: (slug: string) =>
      jsonRequest<{ cleaned: number }>("POST", `/api/workspaces/${slug}/clean`),
    approve: (slug: string) =>
      jsonRequest<{ approved: number }>("POST", `/api/workspaces/${slug}/approve`),
    ignore: (slug: string) =>
      jsonRequest<{ ignored: number }>("POST", `/api/workspaces/${slug}/ignore`),
    unignore: (slug: string) =>
      jsonRequest<{ unignored: number }>("POST", `/api/workspaces/${slug}/unignore`),
    openPreview: (slug: string) =>
      jsonRequest<{ opened: boolean; path: string }>("POST", `/api/workspaces/${slug}/open-preview`),
    cuts: {
      get: (slug: string) =>
        jsonRequest<CutsResponse>("GET", `/api/workspaces/${slug}/cuts`),
      put: (slug: string, body: { cuts: CutsBody; expected_hash: string | null }) =>
        jsonRequest<{ hash: string }>("PUT", `/api/workspaces/${slug}/cuts`, body as unknown as Record<string, unknown>),
    },
    thumbUrl: (slug: string, name: string) => {
      const { baseUrl, token } = getBootstrap()
      return `${baseUrl}/m/${token}/workspaces/${slug}/thumbs/${name}`
    },
    previewUrl: (slug: string) => {
      const { baseUrl, token } = getBootstrap()
      return `${baseUrl}/m/${token}/workspaces/${slug}/preview/index.html`
    },
  },
  batch: {
    run: (body: { slugs: string[]; only?: string; from?: string; through?: string }) =>
      jsonRequest<{ ok: true }>("POST", "/api/batch/run", body),
    reset: (body: { slugs: string[]; from: string }) =>
      jsonRequest<{ ok: true }>("POST", "/api/batch/reset", body),
    clean: (body: { slugs: string[] }) =>
      jsonRequest<{ cleaned: number }>("POST", "/api/batch/clean", body),
    approve: (body: { slugs: string[] }) =>
      jsonRequest<{ approved: number }>("POST", "/api/batch/approve", body),
    collect: (body: { slugs?: string[]; dest?: string; mode?: string; force?: boolean; dry_run?: boolean } = {}) =>
      jsonRequest<{
        collected: number
        from_workspaces: number
        skipped_not_done: number
        skipped_no_output: number
        skipped_existing: number
        destination: string
        mode: string
        dry_run: boolean
        files: string[]
      }>("POST", "/api/batch/collect", body),
  },
  config: {
    get: () => jsonRequest<Record<string, unknown>>("GET", "/api/config"),
    put: (cfg: Record<string, unknown>) =>
      jsonRequest<{ ok: true }>("PUT", "/api/config", cfg),
    getRaw: () => jsonRequest<{ text: string }>("GET", "/api/config/raw"),
    putRaw: (text: string) =>
      jsonRequest<{ ok: true }>("PUT", "/api/config/raw", { text }),
  },
}
