const BASE = ""  // same-origin in production; Vite proxies /api in dev

type Body = Record<string, unknown> | undefined

async function jsonRequest<T>(
  method: "GET" | "POST" | "PUT",
  path: string,
  body?: Body,
): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const detail = await res.json().catch(() => ({}))
    throw new Error(detail?.detail ?? `${res.status} ${res.statusText}`)
  }
  return res.json() as Promise<T>
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
}

export type Workspace = {
  slug: string
  source: string
  stages: Record<string, string>
  any_failed: boolean
  any_awaiting: boolean
  all_done: boolean
  first_thumb: string | null
  /** Number of scenes the detector found. `null` if detect hasn't run yet. */
  scenes_found: number | null
}

export const api = {
  workspaces: {
    list: () => jsonRequest<{ workspaces: Workspace[] }>("GET", "/api/workspaces"),
    detail: (slug: string) => jsonRequest<Workspace & { manifest: unknown }>("GET", `/api/workspaces/${slug}`),
    log: (slug: string, stage: string) =>
      fetch(`/api/workspaces/${slug}/logs/${stage}`).then((r) => r.ok ? r.text() : Promise.reject(r.statusText)),
    run: (slug: string, body: { only?: string; from?: string; through?: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/run`, body),
    reset: (slug: string, body: { from: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/reset`, body),
    clean: (slug: string) =>
      jsonRequest<{ cleaned: number }>("POST", `/api/workspaces/${slug}/clean`),
    approve: (slug: string) =>
      jsonRequest<{ approved: number }>("POST", `/api/workspaces/${slug}/approve`),
    openPreview: (slug: string) =>
      jsonRequest<{ opened: boolean; path: string }>("POST", `/api/workspaces/${slug}/open-preview`),
    cuts: {
      get: (slug: string) =>
        jsonRequest<CutsResponse>("GET", `/api/workspaces/${slug}/cuts`),
      put: (slug: string, body: { cuts: CutsBody; expected_hash: string | null }) =>
        jsonRequest<{ hash: string }>("PUT", `/api/workspaces/${slug}/cuts`, body as unknown as Record<string, unknown>),
    },
    thumbUrl: (slug: string, name: string) => `/api/workspaces/${slug}/thumbs/${name}`,
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
    schema: () => jsonRequest<Record<string, unknown>>("GET", "/api/config/schema"),
  },
}
