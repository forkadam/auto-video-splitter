from __future__ import annotations
import threading
import signal
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.orchestrator import dispatch, Workspace
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


def _stage_instance(name: str):
    from vsplit.stages.probe import ProbeStage
    from vsplit.stages.detect import DetectStage
    from vsplit.stages.preview import PreviewStage
    from vsplit.stages.review import ReviewStage
    from vsplit.stages.plan import PlanStage
    from vsplit.stages.split import SplitStage
    from vsplit.stages.verify import VerifyStage
    REGISTRY = {
        "probe": ProbeStage, "detect": DetectStage,
        "preview": PreviewStage, "review": ReviewStage,
        "plan": PlanStage, "split": SplitStage, "verify": VerifyStage,
    }
    return REGISTRY[name]()


def _unlock_approved_reviews(workspaces: list[Workspace]) -> None:
    """Reset review stages from awaiting_user→pending when approved_at is set."""
    for ws in workspaces:
        m = load_manifest(ws.workspace_dir)
        block = m.stages["review"]
        if block.status == "awaiting_user" and block.artifacts.get("approved_at"):
            block.status = "pending"
            save_manifest(ws.workspace_dir, m)


@click.command(name="run")
@click.argument("slug_glob", required=False)
@click.option("--only", "only_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--from", "from_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--through", "through_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
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

    if only_stage:
        stages_to_run = [only_stage]
    else:
        start = STAGE_NAMES.index(from_stage) if from_stage else 0
        end = STAGE_NAMES.index(through_stage) + 1 if through_stage else len(STAGE_NAMES)
        stages_to_run = list(STAGE_NAMES[start:end])

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
        # workspace is "at stage X" if X is the next non-done stage
        for s in STAGE_NAMES:
            if m.stages[s].status != "done":
                return s == val
        return False

    slugs = apply_filters(slugs, preds, status_pred=has_status, stage_pred=at_stage)

    workspaces = []
    for s in slugs:
        ws = workspace_dir(working_root, s)
        m = load_manifest(ws)
        workspaces.append(Workspace(
            slug=s,
            workspace_dir=ws,
            source_path=working_root / m.source.relative_path,
        ))

    if dry_run:
        for w in workspaces:
            console.print(f"[blue]would run[/]: {w.slug} stages={stages_to_run}")
        return

    _unlock_approved_reviews(workspaces)

    cancel_event = threading.Event()

    def _on_sigint(signum, frame):
        console.print("\n[yellow]cancellation requested; finishing in-flight stages...[/]")
        cancel_event.set()

    try:
        signal.signal(signal.SIGINT, _on_sigint)
    except (OSError, ValueError):
        pass  # non-main thread or unsupported platform

    dispatch(
        workspaces=workspaces,
        working_root=working_root,
        config=config,
        cancel_event=cancel_event,
        stages_to_run=stages_to_run,
        stage_factory=_stage_instance,
    )

    if cancel_event.is_set():
        ctx.exit(130)
