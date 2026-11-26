import { useEffect, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
import { api } from "@/lib/api"

export function LogDrawer({
  slug, stage, onClose,
}: { slug: string | null; stage: string | null; onClose: () => void }) {
  const [loaded, setLoaded] = useState<{ key: string; text: string } | null>(null)
  const key = slug && stage ? `${slug}:${stage}` : null

  useEffect(() => {
    if (!slug || !stage) return
    let cancelled = false
    const reqKey = `${slug}:${stage}`
    api.workspaces.log(slug, stage)
      .then((text) => {
        if (!cancelled) setLoaded({ key: reqKey, text })
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoaded({ key: reqKey, text: `(failed to load log: ${e})` })
      })
    return () => { cancelled = true }
  }, [slug, stage])

  const display = key && loaded?.key === key ? loaded.text : "Loading…"

  return (
    <Drawer open={!!slug} onOpenChange={(o) => !o && onClose()}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{slug} · {stage} log</DrawerTitle>
        </DrawerHeader>
        <pre className="px-4 pb-4 text-xs font-mono overflow-auto max-h-[60vh] whitespace-pre-wrap">{display}</pre>
      </DrawerContent>
    </Drawer>
  )
}
