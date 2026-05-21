# Recipes

This page shows copyable ways to make `editr` feel like a normal part of your
terminal workflow. The examples use placeholder hosts and paths; adapt them to
your SSH config and remote filesystem layout.

For Neovim-specific routing, picker, and canola/oil recipes, see
[`editr.nvim` recipes](https://github.com/serhez/editr.nvim/blob/main/docs/recipes.md).

## Minimal Remote Project

Open a project with the default editor from your config:

```sh
editr cluster:/home/alex/projects/app
```

Open it with a one-off editor command:

```sh
editr cluster:/home/alex/projects/app --editor 'hx {path}'
```

Ignore generated paths that should not be mirrored:

```sh
editr cluster:/home/alex/projects/app \
  --ignore runs/ \
  --ignore checkpoints/ \
  --ignore '*.pt'
```

Ignored paths are excluded in both directions by Mutagen. Use them for caches,
logs, checkpoints, datasets, and generated artifacts, not for tracked source
files you expect local Git or local tools to see.

## Shared Config

Put common defaults in `~/.config/editr/config.toml`:

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

"cluster:/home/*/projects/ml*" = [
  "wandb/",
  "runs/",
  "checkpoints/",
  "*.pt",
  "*.safetensors",
]
```

This keeps the command line short while still allowing project-specific ignores.

## Short Shell Command

A small wrapper is useful when remote projects live under a few known storage
roots. This example creates an `er` command:

```sh
remote_edit() {
  local host=""
  local storage="home"
  local dir=""
  local local_root="$HOME/remote"
  local -a editr_args

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --storage|-s)
        storage="$2"
        shift 2
        ;;
      --dir|-d)
        dir="$2"
        shift 2
        ;;
      --local-root|-L)
        local_root="$2"
        shift 2
        ;;
      --ignore|-i|--config|-C|--mode|-m|--editor|-e)
        editr_args+=("$1" "$2")
        shift 2
        ;;
      --sync-vcs|--ignore-vcs|--keep-session|--dry-run)
        editr_args+=("$1")
        shift
        ;;
      -*)
        editr_args+=("$1")
        shift
        ;;
      *)
        if [[ -z "$host" ]]; then
          host="$1"
          shift
        else
          echo "Unexpected argument: $1" >&2
          return 1
        fi
        ;;
    esac
  done

  if [[ -z "$host" || -z "$dir" ]]; then
    echo "Usage: er HOST --dir PATH [editr options]" >&2
    return 1
  fi

  if [[ "$dir" == /* || "$dir" == *../* || "$dir" == ../* || "$dir" == ".." ]]; then
    echo "--dir must be relative to the selected storage root" >&2
    return 1
  fi

  local base_path
  case "$storage" in
    home) base_path="/home/$USER" ;;
    scratch) base_path="/scratch/$USER" ;;
    work) base_path="/work/$USER" ;;
    *)
      echo "Unknown storage root: $storage" >&2
      return 1
      ;;
  esac

  local remote_path="${base_path%/}/${dir#/}"
  local local_path="${local_root/#\~/$HOME}/$host/$storage/${dir#/}"
  local safe_dir="${dir//\//-}"
  local session_name="editr-${host}-${storage}-${safe_dir}"

  command editr "$host:$remote_path" \
    --local-path "$local_path" \
    --session-name "$session_name" \
    "${editr_args[@]}"
}

alias er=remote_edit
```

Usage:

```sh
er cluster --dir projects/app
er cluster --storage scratch --dir experiments/run-42 --ignore logs/
er cluster --dir projects/app --dry-run
```

The wrapper keeps local mirror paths stable, so later sessions reuse the same
mirror instead of starting from an empty directory.

## macOS SSH Agent Helper

If SSH keys live in the macOS keychain, add this before the final `editr`
command in a wrapper:

```sh
if [[ "$OSTYPE" == darwin* ]]; then
  agent_env="$HOME/.ssh/agent/env"
  if [[ ( -z "${SSH_AUTH_SOCK:-}" || ! -S "$SSH_AUTH_SOCK" ) && -r "$agent_env" ]]; then
    source "$agent_env" >/dev/null
  fi
  if ! ssh-add -l >/dev/null 2>&1; then
    [[ -r "$agent_env" ]] && source "$agent_env" >/dev/null
    ssh-add --apple-load-keychain -q >/dev/null 2>&1 || true
  fi
fi
```

This avoids repeated passphrase prompts when an existing agent can be reused.

## Keep Background Sessions Explicit

The default behavior is transparent: `editr` flushes and terminates the Mutagen
session when the editor exits. Use `--keep-session` only when you intentionally
want background synchronization:

```sh
editr cluster:/home/alex/projects/app --keep-session
editr list
editr stop cluster:/home/alex/projects/app
```

For crash recovery and stale hydration sessions, run:

```sh
editr watch
```
