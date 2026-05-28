import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.orchestrator import dispatch, Workspace
from vsplit.core.types import Stage, StageContext, StageResult
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.schemas.manifest import STAGE_NAMES

class _CountingStage(Stage):
    name = "probe"
    depends_on: list[str] = []
    output_subdir = None
    calls: list[str] = []
    lock = threading.Lock()
    def run(self, ctx):
        with _CountingStage.lock:
            _CountingStage.calls.append(ctx.workspace_dir.name)
        return StageResult()

def _seed(tmp_path: Path, slugs: list[str]):
    workspaces = []
    for s in slugs:
        ws = tmp_path / s
        ws.mkdir()
        m = new_manifest(slug=s, source=SourceInfo(
            relative_path=f"{s}.mp4", size_bytes=1,
            mtime=datetime.now(timezone.utc), partial_hash="x"))
        save_manifest(ws, m)
        workspaces.append(Workspace(slug=s, workspace_dir=ws, source_path=ws / f"{s}.mp4"))
    return workspaces

def test_dispatch_runs_every_workspace_once(tmp_path, monkeypatch):
    _CountingStage.calls = []
    ws_list = _seed(tmp_path, ["a", "b", "c"])
    def factory(name: str):
        return _CountingStage()
    # Run only probe (single stage subset)
    dispatch(
        workspaces=ws_list,
        working_root=tmp_path,
        config=Config(),
        cancel_event=threading.Event(),
        stages_to_run=["probe"],
        stage_factory=factory,
    )
    assert sorted(_CountingStage.calls) == ["a", "b", "c"]
    for w in ws_list:
        m = load_manifest(w.workspace_dir)
        assert m.stages["probe"].status == "done"

def test_dispatch_isolates_failures(tmp_path):
    class _BadOnB(Stage):
        name = "probe"
        depends_on: list[str] = []
        output_subdir = None
        def run(self, ctx):
            if ctx.workspace_dir.name == "b":
                raise RuntimeError("boom")
            return StageResult()
    ws_list = _seed(tmp_path, ["a", "b", "c"])
    dispatch(
        workspaces=ws_list,
        working_root=tmp_path,
        config=Config(),
        cancel_event=threading.Event(),
        stages_to_run=["probe"],
        stage_factory=lambda n: _BadOnB(),
    )
    statuses = {w.slug: load_manifest(w.workspace_dir).stages["probe"].status
                for w in ws_list}
    assert statuses == {"a": "done", "b": "failed", "c": "done"}


def test_dispatch_force_fails_stage_stuck_running(tmp_path, monkeypatch):
    """If pipeline.run_stage exits without recording a terminal status (e.g. save_manifest
    raised after stage.run returned), the worker leaves the stage as 'running' on disk.
    The orchestrator must detect this when the future completes and force-fail the stage,
    or the workspace hangs forever because _next_runnable short-circuits on 'running'."""
    from vsplit.core import orchestrator as orch_mod
    from vsplit.core.manifest import save_manifest as real_save_manifest

    ws_list = _seed(tmp_path, ["a"])

    # Simulate the production bug: pipeline.run_stage stamps status='running',
    # save_manifest succeeds, stage.run completes, then the final save_manifest
    # raises and propagates up uncaught.
    call_log = {"saves": 0}

    def flaky_save(workspace_dir, manifest):
        # The first save (status='running') succeeds; the second (terminal) raises.
        call_log["saves"] += 1
        if call_log["saves"] == 1:
            return real_save_manifest(workspace_dir, manifest)
        raise OSError("simulated disk hiccup during terminal save_manifest")

    monkeypatch.setattr("vsplit.core.pipeline.save_manifest", flaky_save)

    class _OkStage(Stage):
        name = "probe"
        depends_on: list[str] = []
        output_subdir = None
        def run(self, ctx):
            return StageResult()

    orch_mod.dispatch(
        workspaces=ws_list,
        working_root=tmp_path,
        config=Config(),
        cancel_event=threading.Event(),
        stages_to_run=["probe"],
        stage_factory=lambda n: _OkStage(),
    )

    m = load_manifest(ws_list[0].workspace_dir)
    assert m.stages["probe"].status == "failed", (
        f"orchestrator should have force-failed the stuck 'running' stage, "
        f"got status={m.stages['probe'].status!r}"
    )
    assert m.stages["probe"].error is not None
    assert "without recording" in m.stages["probe"].error.message.lower() \
        or "stuck" in m.stages["probe"].error.message.lower()
