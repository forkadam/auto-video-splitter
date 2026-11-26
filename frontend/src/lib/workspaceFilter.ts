import type { Workspace } from "./api"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

export type SortKey = "name" | "status" | "duration_desc" | "duration_asc"

export type FilterState = {
  status: string
  stage: string
  slug: string
  sort: SortKey
}

export function matchesStatusBucket(ws: Workspace, status: string): boolean {
  if (status === "any") return true
  if (status === "ignored") return ws.ignored
  if (ws.ignored) return false
  if (status === "done") return ws.all_done
  if (status === "no_scenes") {
    return ws.scenes_found === 0 && Object.values(ws.stages).includes("awaiting_user")
  }
  if (status === "awaiting_user") {
    return ws.scenes_found !== 0 && Object.values(ws.stages).includes("awaiting_user")
  }
  if (status === "failed") return ws.any_failed
  if (status === "running") return Object.values(ws.stages).includes("running")
  if (status === "pending") {
    return (
      !ws.all_done
      && !ws.any_failed
      && !Object.values(ws.stages).includes("running")
      && !Object.values(ws.stages).includes("awaiting_user")
    )
  }
  return Object.values(ws.stages).includes(status)
}

function matchesSlugQuery(ws: Workspace, rawQuery: string): boolean {
  const q = rawQuery.trim()
  if (!q) return true
  if (q.includes("*") || q.includes("?")) {
    const escaped = q.replace(/[.+^${}()|[\]\\]/g, "\\$&")
    const regex = new RegExp("^" + escaped.replace(/\*/g, ".*").replace(/\?/g, ".") + "$", "i")
    return regex.test(ws.slug) || regex.test(ws.source)
  }
  const lower = q.toLowerCase()
  return ws.slug.toLowerCase().includes(lower) || ws.source.toLowerCase().includes(lower)
}

export function matchesFilter(ws: Workspace, f: FilterState): boolean {
  if (!matchesStatusBucket(ws, f.status)) return false
  if (f.stage !== "any") {
    const next = STAGES.find(
      (s) => ws.stages[s] !== "done" && ws.stages[s] !== "skipped",
    )
    if (next !== f.stage) return false
  }
  return matchesSlugQuery(ws, f.slug)
}

function statusPriority(ws: Workspace): number {
  if (ws.ignored) return 6
  if (ws.any_failed) return 0
  if (Object.values(ws.stages).includes("running")) return 1
  if (ws.scenes_found !== 0 && Object.values(ws.stages).includes("awaiting_user")) return 2
  if (ws.scenes_found === 0 && Object.values(ws.stages).includes("awaiting_user")) return 3
  if (!ws.all_done) return 4
  return 5
}

export function sortWorkspaces(list: Workspace[], sort: SortKey): Workspace[] {
  const copy = [...list]
  const byName = (a: Workspace, b: Workspace) =>
    a.slug.localeCompare(b.slug, undefined, { numeric: true, sensitivity: "base" })
  if (sort === "status") {
    copy.sort((a, b) => statusPriority(a) - statusPriority(b) || byName(a, b))
  } else if (sort === "duration_desc") {
    copy.sort((a, b) => (b.duration_seconds ?? -1) - (a.duration_seconds ?? -1) || byName(a, b))
  } else if (sort === "duration_asc") {
    copy.sort(
      (a, b) =>
        (a.duration_seconds ?? Number.POSITIVE_INFINITY)
        - (b.duration_seconds ?? Number.POSITIVE_INFINITY)
        || byName(a, b),
    )
  } else {
    copy.sort(byName)
  }
  return copy
}
