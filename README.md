# vsplit

Local, scene-aware video splitter with a human review gate. Built for batches of personal video.

## Install

Prerequisites: Python 3.11+, FFmpeg, FFprobe.

```bash
uv venv
uv pip install -e ".[dev]"
```

## Quick start

```bash
cd ~/vacation-clips
vsplit scan                     # adopt every video as a workspace
vsplit run --through preview    # detect scenes, generate thumbnails
open .vsplit/workspaces/*/preview/index.html
# edit .vsplit/workspaces/*/decisions/cuts.json (set keep:false on scenes you don't want)
vsplit run                      # plan, split, verify
ls .vsplit/workspaces/*/output/ # your clips
```

## Single-file shortcut

```bash
vsplit one Birthday.mov              # scan + run + open preview
vsplit one Birthday.mov --auto-approve   # skip the review gate
```

## Commands

| Command | Purpose |
|--|--|
| `vsplit scan [path]` | Discover and adopt video files |
| `vsplit run [filter]` | Advance pipelines |
| `vsplit status` | Show pipeline progress |
| `vsplit approve <filter>` | Mark workspaces past the review gate |
| `vsplit reset <filter> --from <stage>` | Re-run stages |
| `vsplit clean <filter>` | Delete workspace state (source files untouched) |
| `vsplit one <file>` | Single-file shortcut |

## Filter grammar

`--filter key=value` where key is `status`, `stage`, or `slug`. Repeat to AND. Bare positional argument is a slug glob.

```
vsplit run --filter status=awaiting_user
vsplit run "vacation-*"
vsplit clean --filter status=done
```

## Config

`.vsplit/config.yaml` overrides defaults. See `docs/superpowers/specs/2026-05-21-vsplit-design.md` §3.4 for the full schema.

## Windows notes

- Run from a path short enough that workspace files stay under MAX_PATH. The tool warns at scan time if you're close.
- Exclude your working folder from real-time antivirus scanning during heavy operations.

## License

(not yet)
