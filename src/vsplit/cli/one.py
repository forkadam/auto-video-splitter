from __future__ import annotations
import webbrowser
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli.scan import scan_cmd
from vsplit.cli.run import run_cmd
from vsplit.cli.approve import approve_cmd
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.core.slugify import slugify
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


@click.command(name="one")
@click.argument("file_path", type=click.Path(exists=True, dir_okay=False))
@click.option("--through", "through_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--auto-approve", is_flag=True)
@click.pass_context
def one_cmd(ctx, file_path, through_stage, auto_approve):
    """Process a single file end-to-end. Implies scan + run."""
    file_path = Path(file_path).resolve()
    parent = file_path.parent
    # rewrite working dir to the parent of the file
    ctx.obj["working_dir"] = parent

    # 1. scan parent (idempotent; picks up the file)
    ctx.invoke(scan_cmd)

    slug = slugify(file_path.name)

    # 2. run through preview (or through_stage if given)
    target_through = through_stage or "preview"
    ctx.invoke(run_cmd, slug_glob=slug, through_stage=target_through)

    # 3. open preview HTML if it exists
    preview_html = workspace_dir(parent, slug) / "preview" / "index.html"
    if preview_html.exists():
        webbrowser.open(preview_html.as_uri())
        console.print(f"opened {preview_html}")

    if auto_approve:
        # finish review (will fall through to plan/split/verify on next run)
        ctx.invoke(run_cmd, slug_glob=slug, through_stage="review")
        ctx.invoke(approve_cmd, slug_glob=slug)
        ctx.invoke(run_cmd, slug_glob=slug)
