# Configuration

`editr` reads configuration from:

```text
~/.config/editr/config.toml
```

Use `--config PATH` to load a different file. If the default file does not
exist, `editr` creates it the first time it needs configuration.

## Complete Example

```toml
local_root = "~/remote"
editor = "nvim"
mode = "two-way-safe"
sync_vcs = true
keep_session = false

[hydrate]
max_auto_size = "25 MB"
default_mode = "live"

[watcher]
interval = "60s"
notify = true
auto_stop_hydration_after = "10m"

[ignore]
"*" = [
  ".DS_Store",
  ".venv/",
  "venv/",
  "__pycache__/",
  ".mypy_cache/",
  ".ruff_cache/",
  ".pytest_cache/",
  "node_modules/",
  ".git/*.lock",
  ".git/**/*.lock",
]

"cluster:/home/user/project*" = [
  "wandb/",
  "checkpoints/",
  "*.pt",
]
```

## Top-Level Keys

`local_root`

Directory where mirrors and session metadata are stored. The default is
`~/remote`.

`editor`

Shell command used to start the editor from the mirror. If the command contains
`{path}`, `editr` substitutes the local mirror path. Otherwise it appends `.`.

`mode`

Mutagen mode for the steady-state session. The default is `two-way-safe`.

`sync_vcs`

Whether to sync VCS metadata such as `.git`. The default is `true` so local Git
tools work.

`keep_session`

Whether the Mutagen session should keep running after the editor exits. The
default is `false`.

## Ignore Rules

Ignore rules live under `[ignore]`.

The `"*"` key applies globally. Other keys match canonical target strings such
as:

```text
host:/absolute/remote/path
```

`*` and `?` are supported in target keys. `*` may match across path separators.

Examples:

```toml
[ignore]
"*" = [
  ".DS_Store",
  "__pycache__/",
  "node_modules/",
]

"gpu-cluster:/home/user/research*" = [
  "wandb/",
  "checkpoints/",
  "runs/",
  "*.pt",
]

"logs-host:/srv/apps/*" = [
  "logs/",
  "*.log",
]
```

Ignore patterns are passed to Mutagen. Ignored paths are not synced in either
direction. This is appropriate for generated artifacts and caches, but not for
tracked source files.

Good ignore candidates:

- `wandb/`
- `checkpoints/`
- `runs/`
- `logs/`
- `.venv/`
- `node_modules/`
- large generated `*.pt`, `*.pkl`, or `*.jsonl` outputs

Risky ignore candidates:

- tracked source files
- tracked config files
- tracked data files needed by tests or Git review
- partial subtrees that local Git tools need to understand

If local Git or Neogit correctness matters, avoid ignoring tracked paths.

## Hydration

Hydration copies or syncs one remote file into the mirror. It is mostly used by
editor integrations for ignored remote-only files.

```toml
[hydrate]
max_auto_size = "25 MB"
default_mode = "live"
```

`max_auto_size`

Default maximum file size for automatic hydration unless a caller explicitly
allows a larger file.

`default_mode`

`oneshot` copies the file and immediately terminates the temporary Mutagen
session. `live` keeps a per-file session alive while the owning editor buffer is
open.

## Watcher

```toml
[watcher]
interval = "60s"
notify = true
auto_stop_hydration_after = "10m"
```

`editr watch` uses these defaults to find suspicious sessions after crashes and
to stop stale hydration sessions.

## CLI Overrides

Most important config values have CLI equivalents:

```sh
editr host:/repo --local-root ~/mirrors
editr host:/repo --editor 'hx {path}'
editr host:/repo --ignore 'wandb/' --ignore '*.pt'
editr host:/repo --ignore-vcs
editr host:/repo --keep-session
```

CLI ignore rules are added to config ignore rules for that invocation.
