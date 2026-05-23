import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"

const STATUSES = ["any", "pending", "running", "done", "failed", "awaiting_user", "skipped"]
const STAGES = ["any", "probe", "detect", "preview", "review", "plan", "split", "verify"]

export type FilterState = { status: string; stage: string; slug: string }

export function FilterBar({ value, onChange }: { value: FilterState; onChange: (v: FilterState) => void }) {
  return (
    <div className="flex gap-2 items-center mb-3">
      <Select value={value.status} onValueChange={(v) => onChange({ ...value, status: v })}>
        <SelectTrigger className="w-40"><SelectValue placeholder="status" /></SelectTrigger>
        <SelectContent>{STATUSES.map(s => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent>
      </Select>
      <Select value={value.stage} onValueChange={(v) => onChange({ ...value, stage: v })}>
        <SelectTrigger className="w-40"><SelectValue placeholder="stage" /></SelectTrigger>
        <SelectContent>{STAGES.map(s => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent>
      </Select>
      <Input placeholder="slug glob (e.g. vacation-*)" value={value.slug} onChange={(e) => onChange({ ...value, slug: e.target.value })} className="max-w-xs" />
    </div>
  )
}
