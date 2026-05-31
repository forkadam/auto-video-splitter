/** Timecode helpers mirroring vsplit's HH:MM:SS.fff format and keyframe snapping. */

/** Matches the backend _TS_RE in schemas/cuts.py exactly. */
export const TC_RE = /^\d{2}:\d{2}:\d{2}\.\d{3}$/

/** "HH:MM:SS.fff" -> seconds. Assumes the string already matches TC_RE. */
export function parseTc(tc: string): number {
  const [h, m, s] = tc.split(":")
  return Number(h) * 3600 + Number(m) * 60 + Number(s)
}

/** seconds -> "HH:MM:SS.fff" (zero-padded), mirroring the stages' _format_tc. */
export function formatTc(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = seconds - h * 3600 - m * 60
  const ss = s.toFixed(3).padStart(6, "0")
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${ss}`
}

/** Largest keyframe <= t, or 0 if none. Mirrors core/keyframes.snap_preceding. */
export function snapPreceding(t: number, keyframes: number[]): number {
  let best = 0
  for (const k of keyframes) {
    if (k <= t && k > best) best = k
  }
  return best
}
