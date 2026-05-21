from __future__ import annotations
import shutil
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES, StageBlock

console = Console()

_STAGE_SUBDIRS = {
    "probe": "probe", "detect": "detect", "preview": "preview",
    "review": "decisions", "plan": "plan", "split": "output", "verify": None,
}


@click.command(name="reset")
@click.argument("slug_glob", required=False)
@click.option("--from", "from_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--all", "reset_all", is_flag=True)
@click.option("--filter", "filters", multiple=True)
@click.option("--force", is_flag=True)
@click.pass_context
def reset_cmd(ctx, slug_glob, from_stage, reset_all, filters, force):
    """Reset stages so they re-run on next `vsplit run`."""
    if not from_stage and not reset_all:
        raise click.ClickException("require --from STAGE or --all")
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    preds: list[Predicate] = []
    if slug_glob:
        preds.append(Predicate(key="slug", value=slug_glob))
    for f in filters:
        preds.append(parse_filter(f))

    def has_status(slug, val):
        m = load_manifest(workspace_dir(working_root, slug))
        return any(b.status == val for b in m.stages.values())
    slugs = apply_filters(slugs, preds,
                          status_pred=has_status, stage_pred=lambda *a: True)

    if reset_all and not force and not click.confirm(
        f"reset ALL stages of {len(slugs)} workspace(s)?", default=False
    ):
        return

    start = 0 if reset_all else STAGE_NAMES.index(from_stage)
    targets = list(STAGE_NAMES[start:])
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        m = load_manifest(ws)
        for stage_name in targets:
            sub = _STAGE_SUBDIRS.get(stage_name)
            if sub:
                shutil.rmtree(ws / sub, ignore_errors=True)
            m.stages[stage_name] = StageBlock(status="pending")
        save_manifest(ws, m)
        console.print(f"[green]reset[/]: {slug} from={targets[0]}")
