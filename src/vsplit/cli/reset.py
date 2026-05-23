from __future__ import annotations
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import reset_workspaces
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


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

    from_eff = "probe" if reset_all else from_stage
    reset_workspaces(working_root, slugs, from_=from_eff)
    for slug in slugs:
        console.print(f"[green]reset[/]: {slug} from={from_eff}")
