import type { Workspace } from "./api"

const ORDER = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

export type NextAction =
  | { kind: "done"; label: "Done" }
  | { kind: "retry"; label: string; stage: string }
  | { kind: "running"; label: string; stage: string }
  | { kind: "review"; label: "Review & approve →" }
  | { kind: "empty"; label: "No scenes" }
  | { kind: "run"; label: string; stage: string }
  | { kind: "idle"; label: "Idle" }

export function nextAction(ws: Workspace): NextAction {
  if (ws.all_done) return { kind: "done", label: "Done" }

  for (const s of ORDER) {
    if (ws.stages[s] === "failed") {
      return { kind: "retry", label: `Retry: ${s} →`, stage: s }
    }
  }

  for (const s of ORDER) {
    if (ws.stages[s] === "running") {
      return { kind: "running", label: `Running: ${s}…`, stage: s }
    }
  }

  // Detector found nothing — approving review would produce zero clips.
  // Flag distinctly from a real "Review & approve" so the user can spot
  // the dead end and decide to lower the threshold + reset, or just clean.
  if (ws.stages.review === "awaiting_user" && ws.scenes_found === 0) {
    return { kind: "empty", label: "No scenes" }
  }

  if (ws.stages.review === "awaiting_user") {
    return { kind: "review", label: "Review & approve →" }
  }

  for (const s of ORDER) {
    if (ws.stages[s] !== "done" && ws.stages[s] !== "skipped") {
      return { kind: "run", label: `Run: ${s} →`, stage: s }
    }
  }

  return { kind: "idle", label: "Idle" }
}
