import { useEffect } from "react"

export type DashboardEvent =
  | { type: "workspace_updated"; slug: string; stages: Record<string, string>; updated_at: string }
  | { type: "workspaces_added"; slugs: string[] }
  | { type: "workspaces_removed"; slugs: string[] }
  | { type: "run_started"; slug: string; stages: string[] }
  | { type: "run_finished"; slug: string; ok: boolean }
  | { type: "config_changed" }
  | { type: "heartbeat" }

export function useSSE(onEvent: (ev: DashboardEvent) => void) {
  useEffect(() => {
    const source = new EventSource("/api/events")
    source.onmessage = (m) => {
      try {
        onEvent(JSON.parse(m.data))
      } catch {
        // ignore malformed
      }
    }
    source.onerror = () => {
      // EventSource auto-reconnects with backoff
    }
    return () => source.close()
  }, [onEvent])
}
