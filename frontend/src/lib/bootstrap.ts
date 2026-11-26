export type ServerBootstrap = {
  baseUrl: string
  token: string
  isTauri: boolean
  os: string
}

export type AppConfig = {
  recent_folders: string[]
  last_folder: string | null
  max_recent_folders: number
  ffmpeg_path: string | null
  ffprobe_path: string | null
  video_player: string | null
  wayland_disable_dmabuf: boolean
}

let cachedBootstrap: ServerBootstrap | null = null

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}

export async function initBootstrap(): Promise<ServerBootstrap> {
  if (cachedBootstrap) return cachedBootstrap

  if (isTauriRuntime()) {
    const { invoke } = await import("@tauri-apps/api/core")
    const res = await invoke<{ base_url: string; token: string; os?: string }>("get_server_bootstrap")
    cachedBootstrap = {
      baseUrl: res.base_url.replace(/\/$/, ""),
      token: res.token,
      isTauri: true,
      os: res.os ?? "unknown",
    }
    return cachedBootstrap
  }

  // Local Vite HMR development fallback
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env ?? {}
  const baseUrl = (env.VITE_VSPLIT_DEV_URL ?? "http://127.0.0.1:8765").replace(/\/$/, "")
  const token = env.VITE_VSPLIT_DEV_TOKEN ?? ""
  cachedBootstrap = { baseUrl, token, isTauri: false, os: "unknown" }
  return cachedBootstrap
}

export function getBootstrap(): ServerBootstrap {
  if (!cachedBootstrap) {
    return { baseUrl: "", token: "", isTauri: isTauriRuntime(), os: "unknown" }
  }
  return cachedBootstrap
}

export async function pickFolderNative(): Promise<string | null> {
  if (!isTauriRuntime()) return null
  const { invoke } = await import("@tauri-apps/api/core")
  return invoke<string | null>("pick_folder")
}

export async function pickFileNative(title?: string): Promise<string | null> {
  if (!isTauriRuntime()) return null
  const { invoke } = await import("@tauri-apps/api/core")
  return invoke<string | null>("pick_file", { title: title ?? null })
}

export async function getAppConfigNative(): Promise<AppConfig | null> {
  if (!isTauriRuntime()) return null
  const { invoke } = await import("@tauri-apps/api/core")
  return invoke<AppConfig>("get_app_config")
}

export async function setAppConfigNative(cfg: AppConfig): Promise<AppConfig | null> {
  if (!isTauriRuntime()) return null
  const { invoke } = await import("@tauri-apps/api/core")
  return invoke<AppConfig>("set_app_config", { config: cfg })
}

export async function repairCliShimNative(): Promise<string | null> {
  if (!isTauriRuntime()) return null
  const { invoke } = await import("@tauri-apps/api/core")
  return invoke<string>("repair_cli_shim")
}
