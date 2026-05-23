// Regenerate src/lib/api.types.ts from the live FastAPI OpenAPI schema.
// Usage: bun run gen-types
import { spawn } from "node:child_process"
import { rmSync, existsSync } from "node:fs"
import { mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

const PORT = 18765
const tmp = mkdtempSync(join(tmpdir(), "vsplit-gen-types-"))

const server = spawn(
  "uv",
  ["run", "vsplit", "-w", tmp, "dashboard", "--no-open", "--port", String(PORT)],
  { stdio: ["ignore", "ignore", "ignore"], cwd: "../../../.." },
)

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

async function main() {
  // Wait for server up (poll /api/health)
  for (let i = 0; i < 50; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/api/health`)
      if (r.ok) break
    } catch {
      // not ready yet
    }
    await sleep(100)
  }

  const r = await fetch(`http://127.0.0.1:${PORT}/openapi.json`)
  const openapi = await r.text()
  const path = "openapi.json"
  await Bun.write(path, openapi)
  // Run openapi-typescript via bunx
  const child = spawn(
    "bunx",
    ["openapi-typescript", "openapi.json", "-o", "src/lib/api.types.ts"],
    { stdio: "inherit", shell: true },
  )
  child.on("exit", (code) => {
    server.kill()
    if (existsSync(tmp)) rmSync(tmp, { recursive: true, force: true })
    console.log(`gen-types finished with code ${code}`)
    process.exit(code ?? 0)
  })
}

main().catch((e) => {
  console.error(e)
  server.kill()
  process.exit(1)
})
