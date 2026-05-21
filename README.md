# editr

Open remote projects in your local editor.

`editr` keeps a local Mutagen mirror of an SSH target and starts your editor
inside that mirror:

```sh
editr cluster:/home/user/project
```

Your editor, language servers, formatters, linters, file pickers, grep tools,
Git UI, and terminal commands see a normal local directory. Mutagen keeps that
directory synchronized with the remote project.

## Why editr?

SSH-native editors and lazy remote browsers are useful, but they do not give
local tools a real filesystem. `editr` is for workflows where the laptop should
run editor tooling while the project still lives on a remote machine.

| Approach | Works well for | Tradeoff |
| --- | --- | --- |
| SSH into the remote and run an editor there | No local sync, files stay remote | Remote editor setup, remote plugins, and remote compute are required |
| canola/oil SSH | Lazy remote browsing and quick remote reads | Local LSPs, Git tools, grep, and most plugins do not see a normal filesystem |
| `editr` | Local LSPs, formatters, pickers, Neogit, and shell tools | Real files must sync; huge paths should be ignored or hydrated intentionally |
| `editr` + editor integration | Local tooling plus prompted access to huge ignored files | Requires editor-specific glue such as `editr.nvim` |

See [Remote Editing Models](docs/remote-editing-models.md) for the detailed
model and tradeoffs.

## Quick Start

1. Install requirements:

```sh
# mutagen must be on PATH
mutagen version
ssh host true
```

2. Install `editr` from this checkout:

```sh
cargo install --path .
```

3. Open a remote project:

```sh
editr host:/absolute/remote/project
```

On first open, `editr` performs a one-way remote-to-local bootstrap so the
remote is the initial source of truth. It then creates a normal two-way Mutagen
session and launches your editor with its working directory set to the local
mirror.

By default, the sync session is flushed and terminated when the editor exits.
Use `--keep-session` only when you intentionally want background sync to keep
running.

## Common Commands

```sh
# Open a project.
editr host:/absolute/remote/project

# Use a specific editor command.
editr host:/absolute/remote/project --editor 'hx {path}'
editr host:/absolute/remote/project --editor 'nvim --clean {path}'

# Ignore large generated paths. Patterns can be repeated.
editr host:/absolute/remote/project --ignore 'wandb/' --ignore '*.pt'

# Inspect the resolved plan without syncing or opening an editor.
editr host:/absolute/remote/project --dry-run

# Inspect, flush, or stop editr-owned sessions.
editr list
editr list --json
editr status host:/absolute/remote/project
editr flush host:/absolute/remote/project
editr stop host:/absolute/remote/project

# Watch for leaked sessions after crashes.
editr watch
editr watch --once --json
```

`editr open host:/path` is equivalent to `editr host:/path`.

## Configuration

The default config path is:

```text
~/.config/editr/config.toml
```

If the file does not exist, `editr` creates one with conservative defaults.

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

See [Configuration](docs/configuration.md) for the full config guide and ignore
recipes.

## Editors

`editr` works with any terminal editor because it only controls the process
working directory and environment. The editor command is a shell command. If it
contains `{path}`, `editr` substitutes the local mirror path there. Otherwise it
appends `.`.

```toml
editor = "nvim"
editor = "hx {path}"
editor = "vim {path}"
editor = "emacs -nw {path}"
```

See [Editor Setup](docs/editors.md) for examples for Neovim, Helix, Vim, Emacs,
and generic `$EDITOR` use.

## Neovim Companion

[`editr.nvim`](https://github.com/serhez/editr.nvim) is the optional Neovim
companion plugin. It is only active when Neovim was launched by `editr`.

It can add:

- Snacks file and grep pickers that search the remote project over SSH and open
  mirror paths.
- canola/oil explorer integration for lazy remote browsing.
- Hydration for ignored remote-only files, with size checks and prompts.
- Router helpers so existing keymaps can call the right local or remote tool.

See the `editr.nvim` README for plugin setup and mapping examples.

## Sync Model

`editr` uses two Mutagen sessions:

1. A short bootstrap session from remote to local.
2. A steady two-way session for the editor lifetime.

Local mirrors are reused. Later opens synchronize changes instead of rebuilding
the whole mirror.

Ignored paths are passed to Mutagen and are not synced in either direction. Use
ignore patterns for generated artifacts, caches, logs, checkpoints, datasets, or
other paths that should remain outside the local mirror. Avoid ignoring tracked
source files if local Git correctness matters.

By default, VCS metadata is synced so local Git tools work. Pass `--ignore-vcs`
or set `sync_vcs = false` if you do not want `.git` synchronized.

## Recovery

Normal exits flush and terminate the project session. If the editor crashes or
the machine loses power, run:

```sh
editr list
editr status --all
editr stop --all
```

`editr watch` can monitor for suspicious sessions and automatically stop stale
hydration sessions. See [Session Watcher](docs/watcher.md).

## Troubleshooting

Common issues:

- SSH hangs or asks for a passphrase repeatedly.
- Mutagen sessions continue running after a crash.
- First sync is slow because the project contains huge generated files.
- Local Git reports missing files because tracked paths were ignored.

See [Troubleshooting](docs/troubleshooting.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
