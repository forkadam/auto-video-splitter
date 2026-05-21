from __future__ import annotations
from rich.console import Console
from rich.table import Table
from vsplit.schemas.manifest import STAGE_NAMES, Manifest

STAGE_HEADERS = {n: n.upper() for n in STAGE_NAMES}

_STATUS_GLYPH = {
    "done": "ok",
    "pending": "-",
    "running": "RUN",
    "failed": "FAIL",
    "awaiting_user": "wait",
    "skipped": "skip",
}


def render_status_table(manifests: dict[str, Manifest], console: Console) -> None:
    if not manifests:
        console.print("0 workspaces")
        return
    table = Table(show_header=True, header_style="bold")
    table.add_column("SLUG")
    for n in STAGE_NAMES:
        table.add_column(STAGE_HEADERS[n])
    for slug, m in sorted(manifests.items()):
        row = [slug]
        for n in STAGE_NAMES:
            row.append(_STATUS_GLYPH.get(m.stages[n].status, m.stages[n].status))
        table.add_row(*row)
    console.print(table)

    done = sum(1 for m in manifests.values()
               if all(m.stages[n].status == "done" for n in STAGE_NAMES))
    awaiting = sum(1 for m in manifests.values()
                   if any(m.stages[n].status == "awaiting_user" for n in STAGE_NAMES))
    failed = sum(1 for m in manifests.values()
                 if any(m.stages[n].status == "failed" for n in STAGE_NAMES))
    console.print(
        f"\n{len(manifests)} workspaces · {done} done · "
        f"{awaiting} awaiting user · {failed} failed"
    )
