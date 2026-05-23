from __future__ import annotations
import threading
import signal
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import run_workspaces
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


@click.command(name="run")
@click.argument("slug_glob", required=False)
@click.option("--only", "only_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--from", "from_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--through", "through_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--filter", "filters", multiple=True)
@click.option("--concurrency", "concurrency_overrides", multiple=True,
              help="stage=N, e.g. split=4")
@click.option("--dry-run", is_flag=True)
@click.pass_context
def run_cmd(ctx, slug_glob, only_stage, from_stage, through_stage,
            filters, concurrency_overrides, dry_run):
    """Advance workspaces through the pipeline."""
    working_root = Path(ctx.obj["working_dir"])
    config = load_resolved_config(working_root=working_root,
                                  explicit_path=ctx.obj.get("config_path"))
    for override in concurrency_overrides:
        if "=" not in override:
            raise click.ClickException(f"bad --concurrency: {override}")
        k, v = override.split("=", 1)
        config.concurrency[k] = int(v)

    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    preds: list[Predicate] = []
    if slug_glob:
        preds.append(Predicate(key="slug", value=slug_glob))
    for f in filters:
        preds.append(parse_filter(f))

    def has_status(slug: str, val: str) -> bool:
        m = load_manifest(workspace_dir(working_root, slug))
        return any(b.status == val for b in m.stages.values())

    def at_stage(slug: str, val: str) -> bool:
        m = load_manifest(workspace_dir(working_root, slug))
        for s in STAGE_NAMES:
            if m.stages[s].status != "done":
                return s == val
        return False

    slugs = apply_filters(slugs, preds, status_pred=has_status, stage_pred=at_stage)

    if dry_run:
        for s in slugs:
            console.print(f"[blue]would run[/]: {s}")
        return

    cancel_event = threading.Event()

    def _on_sigint(signum, frame):
        console.print("\n[yellow]cancellation requested; finishing in-flight stages...[/]")
        cancel_event.set()

    try:
        signal.signal(signal.SIGINT, _on_sigint)
    except (OSError, ValueError):
        pass

    run_workspaces(
        working_root, slugs,
        config=config, cancel_event=cancel_event,
        only=only_stage, from_=from_stage, through=through_stage,
    )
    if cancel_event.is_set():
        ctx.exit(130)
