from pathlib import Path
import click
from vsplit.cli.scan import scan_cmd
from vsplit.cli.status import status_cmd
from vsplit.cli.run import run_cmd
from vsplit.cli.approve import approve_cmd
from vsplit.cli.reset import reset_cmd
from vsplit.cli.clean import clean_cmd
from vsplit.cli.one import one_cmd
from vsplit.cli.dashboard import dashboard_cmd


@click.group()
@click.option("--working-dir", "-w", default=".", type=click.Path(file_okay=False))
@click.option("--config", "config_path", default=None, type=click.Path(dir_okay=False))
@click.option("--log-level", default="info",
              type=click.Choice(["debug", "info", "warn", "error"]))
@click.option("--json", "json_output", is_flag=True)
@click.version_option()
@click.pass_context
def main(ctx, working_dir, config_path, log_level, json_output):
    """vsplit — local scene-aware video splitter."""
    ctx.ensure_object(dict)
    ctx.obj["working_dir"] = Path(working_dir).resolve()
    ctx.obj["config_path"] = Path(config_path) if config_path else None
    ctx.obj["log_level"] = log_level
    ctx.obj["json_output"] = json_output


main.add_command(scan_cmd)
main.add_command(status_cmd)
main.add_command(run_cmd)
main.add_command(approve_cmd)
main.add_command(reset_cmd)
main.add_command(clean_cmd)
main.add_command(one_cmd)
main.add_command(dashboard_cmd)
