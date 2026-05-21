from __future__ import annotations
import threading
from pathlib import Path
import click
from rich.console import Console
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.core.pipeline import run_stage
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()

def _stage_instance(name: str):
    from vsplit.stages.probe import ProbeStage
    from vsplit.stages.detect import DetectStage
    from vsplit.stages.preview import PreviewStage
    from vsplit.stages.review import ReviewStage
    from vsplit.stages.plan import PlanStage
    from vsplit.stages.split import SplitStage
    REGISTRY = {"probe": ProbeStage, "detect": DetectStage, "preview": PreviewStage, "review": ReviewStage, "plan": PlanStage, "split": SplitStage}
    if name not in REGISTRY:
        raise click.ClickException(f"stage '{name}' not implemented yet")
    return REGISTRY[name]()


@click.command(name="run")
@click.argument("slug_glob", required=False)
@click.option("--only", "only_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--from", "from_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--through", "through_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--filter", "filters", multiple=True)
@click.option("--dry-run", is_flag=True)
@click.pass_context
def run_cmd(ctx, slug_glob, only_stage, from_stage, through_stage, filters, dry_run):
    """Advance workspaces through the pipeline."""
    working_root = Path(ctx.obj["working_dir"])
    config = load_resolved_config(working_root=working_root,
                                  explicit_path=ctx.obj.get("config_path"))
    idx = load_index(working_root)

    # Determine which stages this invocation will run.
    if only_stage:
        stages_to_run = [only_stage]
    else:
        start = STAGE_NAMES.index(from_stage) if from_stage else 0
        end = STAGE_NAMES.index(through_stage) + 1 if through_stage else len(STAGE_NAMES)
        stages_to_run = list(STAGE_NAMES[start:end])

    # Filter workspaces by slug glob (other filters added in later tasks).
    import fnmatch
    slugs = list(idx.workspaces.keys())
    if slug_glob:
        slugs = [s for s in slugs if fnmatch.fnmatch(s, slug_glob)]

    cancel_event = threading.Event()
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        manifest = load_manifest(ws)
        source = working_root / manifest.source.relative_path
        for stage_name in stages_to_run:
            block = manifest.stages[stage_name]
            if block.status == "done":
                continue
            if block.status in ("failed", "awaiting_user"):
                console.print(f"[yellow]{slug}/{stage_name}[/]: blocked ({block.status})")
                break
            try:
                stage = _stage_instance(stage_name)
            except click.ClickException as e:
                console.print(f"[yellow]{e}[/]")
                break
            if dry_run:
                console.print(f"[blue]would run[/]: {slug}/{stage_name}")
                continue
            console.print(f"[green]{slug}/{stage_name}[/]: running")
            run_stage(
                stage=stage,
                workspace_dir=ws,
                working_root=working_root,
                source_path=source,
                config=config,
                cancel_event=cancel_event,
            )
            manifest = load_manifest(ws)
            if manifest.stages[stage_name].status != "done":
                break
