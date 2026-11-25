import { useEffect, useRef } from "react"
import type { LockLease } from "./api"
import { getBootstrap } from "./bootstrap"

export type DashboardEvent =
  | { type: "workspace_updated"; slug: string; stages: Record<string, string>; updated_at: string }
  | { type: "workspaces_added"; slugs: string[] }
  | { type: "workspaces_removed"; slugs: string[] }
  | { type: "cuts_changed"; slug: string; hash: string }
  | { type: "config_changed" }
  | { type: "lock_changed"; slug: string; locked_by: LockLease | null }
  | { type: "run_started"; slug: string; stages: string[] }
  | { type: "run_finished"; slug: string; ok: boolean }
  | { type: "project_changed"; working_root: string | null }
  | { type: "sync_lagged" }
  | { type: "heartbeat" }

export function useSSE(onEvent: (ev: DashboardEvent) => void, enabled = true) {
  const cbRef = useRef(onEvent)
  useEffect(() => {
    cbRef.current = onEvent
  }, [onEvent])

  useEffect(() => {
    if (!enabled) return
    const { baseUrl, token } = getBootstrap()
    const url = token ? `${baseUrl}/m/${token}/events` : `${baseUrl}/api/events`
    const source = new EventSource(url)
    source.onmessage = (m) => {
      try {
        cbRef.current(JSON.parse(m.data))
      } catch {
        // ignore malformed frames
      }
    }
    source.onerror = () => {
      // EventSource automatically reconnects with backoff
    }
    return () => source.close()
  }, [enabled])
}
