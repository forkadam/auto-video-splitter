import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"

const STATUSES = [
  "any", "pending", "running", "done", "failed", "awaiting_user", "skipped", "no_scenes",
]
const STAGES = ["any", "probe", "detect", "preview", "review", "plan", "split", "verify"]

// Display underscored values as space-separated for readability ("awaiting user"
// rather than "awaiting_user"). The value itself stays snake_case so it lines
// up with the on-disk stage status strings used by matchesFilter.
const labelOf = (s: string) => s.replace(/_/g, " ")

export type FilterState = { status: string; stage: string; slug: string }

export function FilterBar({ value, onChange }: { value: FilterState; onChange: (v: FilterState) => void }) {
  return (
    <div className="flex gap-2 items-center mb-4">
      <Select value={value.status} onValueChange={(v) => onChange({ ...value, status: v })}>
        <SelectTrigger className="w-40">
          <span className="text-muted-foreground text-xs mr-1">status:</span>
          <SelectValue placeholder="status" />
        </SelectTrigger>
        <SelectContent>{STATUSES.map(s => <SelectItem key={s} value={s}>{labelOf(s)}</SelectItem>)}</SelectContent>
      </Select>
      <Select value={value.stage} onValueChange={(v) => onChange({ ...value, stage: v })}>
        <SelectTrigger className="w-40">
          <span className="text-muted-foreground text-xs mr-1">stage:</span>
          <SelectValue placeholder="stage" />
        </SelectTrigger>
        <SelectContent>{STAGES.map(s => <SelectItem key={s} value={s}>{labelOf(s)}</SelectItem>)}</SelectContent>
      </Select>
      <Input
        placeholder="slug glob (e.g. vacation-*)"
        value={value.slug}
        onChange={(e) => onChange({ ...value, slug: e.target.value })}
        className="max-w-xs"
      />
    </div>
  )
}
