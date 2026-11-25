import { useEffect, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
import { api } from "@/lib/api"

export function LogDrawer({
  slug, stage, onClose,
}: { slug: string | null; stage: string | null; onClose: () => void }) {
  const [text, setText] = useState<string>("")
  useEffect(() => {
    if (!slug || !stage) return
    api.workspaces.log(slug, stage).then(setText).catch((e) => setText(`(failed to load log: ${e})`))
  }, [slug, stage])
  return (
    <Drawer open={!!slug} onOpenChange={(o) => !o && onClose()}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{slug} · {stage} log</DrawerTitle>
        </DrawerHeader>
        <pre className="px-4 pb-4 text-xs font-mono overflow-auto max-h-[60vh] whitespace-pre-wrap">{text}</pre>
      </DrawerContent>
    </Drawer>
  )
}
