// Stages the compiled release `vsplit` CLI binary into `apps/desktop/src-tauri/binaries/`
// with the current Rust target triple suffix required by Tauri v2 `externalBin` and AppImage `resources`.
import { execSync } from "node:child_process"
import { copyFileSync, mkdirSync, chmodSync, existsSync } from "node:fs"
import { resolve, dirname } from "node:path"
import { fileURLToPath } from "node:url"

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, "../../..")
const binDir = resolve(here, "../src-tauri/binaries")

const rustcV = execSync("rustc -vV", { encoding: "utf8" })
const hostLine = rustcV.split(/\r?\n/).find((l) => l.startsWith("host:"))
if (!hostLine) {
  throw new Error("Could not determine rustc host target triple")
}
const triple = hostLine.replace("host:", "").trim()
const isWin = process.platform === "win32"
const ext = isWin ? ".exe" : ""

const srcBin = resolve(repoRoot, "target", "release", `vsplit${ext}`)
if (!existsSync(srcBin)) {
  console.log(`Building release vsplit-cli (${triple})...`)
  execSync("cargo build -p vsplit-cli --release", { cwd: repoRoot, stdio: "inherit" })
}

mkdirSync(binDir, { recursive: true })
const dstTriple = resolve(binDir, `vsplit-${triple}${ext}`)
copyFileSync(srcBin, dstTriple)
if (!isWin) {
  chmodSync(dstTriple, 0o755)
  const dstPlain = resolve(binDir, "vsplit")
  copyFileSync(srcBin, dstPlain)
  chmodSync(dstPlain, 0o755)
}
console.log(`Staged CLI sidecar -> ${dstTriple}`)
