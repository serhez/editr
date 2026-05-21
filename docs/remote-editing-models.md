# Remote Editing Models

`editr` sits between two common remote editing approaches:

- Run the editor remotely over SSH.
- Browse remote files lazily from a local editor.

Both are useful. Neither gives local editor tooling a complete local workspace.
`editr` fills that gap by keeping a real local mirror of the remote project.

## Comparison

| Approach | Strengths | Weaknesses |
| --- | --- | --- |
| SSH into the machine and run an editor remotely | No local sync; all tools run beside the files | Requires remote editor configuration, remote plugins, remote language servers, and remote compute |
| Lazy SSH browser such as canola/oil SSH | Fast directory browsing and per-file remote reads | Local LSPs, formatters, Git tools, `rg`, `fd`, and most plugins cannot treat it as a normal filesystem |
| `editr` mirror | Local LSPs, formatters, pickers, Neogit, shell tools, and editor plugins see a normal directory | Real files must sync, so huge generated paths should be ignored |
| `editr` plus editor integration | Local tooling for source code plus prompted access to ignored remote-only files | Requires editor-specific integration and clear hydration rules |

## What `editr` Gives You

`editr` turns a remote project into a normal local directory. That makes the
largest part of a local development setup work without every plugin needing SSH
support:

- local LSPs can index source files
- local formatters and linters can run on the mirror
- local `rg`, `fd`, file pickers, and grep pickers work normally
- local Neogit and other Git tools can operate on the mirrored repository
- terminal tools can run against the mirror
- editor compute runs on the local machine instead of a shared login node

This is different from canola/oil SSH. canola/oil can browse remote directories
lazily, but it does not make the remote project a local filesystem.

## What Lazy SSH Browsing Still Gives You

Lazy SSH browsing is excellent for paths that are too large or too volatile to
mirror:

- logs
- datasets
- checkpoints
- rollouts
- cache directories
- generated experiment outputs

The tradeoff is that these paths are remote buffers, not local files. Local
tools cannot operate on them unless an integration explicitly bridges the remote
selection back into the mirror.

## The Hybrid Rule

The model is intentionally strict:

```text
Mutagen owns the mirror.
canola/oil owns lazy remote browsing.
Hydration is the explicit bridge between them.
```

Synced files should open from the mirror. Ignored files should remain remote
unless the user or editor integration explicitly hydrates them.

## Hydration

Hydration syncs one remote file into the local mirror:

```sh
editr hydrate --context "$EDITR_CONTEXT" --remote-path /repo/logs/run.txt --json
```

Core `editr` exposes hydration as an explicit command. Editor integrations can
use it to implement a prompted flow:

```text
select remote-only file -> check size -> hydrate or open remote or cancel
```

This keeps large files visible without making every remote file part of the
local mirror.

## Why Not Lazy Stubs?

Mutagen does not provide stub files or on-demand file contents. It synchronizes
real filesystem entries according to its scan and ignore rules.

A true stub model would require a filesystem layer such as FUSE or an operating
system file provider. That would also introduce a major risk: local grep, LSP
indexing, or file pickers could accidentally trigger large downloads just by
walking the tree.

`editr` chooses explicit sync instead:

- source code and normal project files are mirrored
- huge generated paths are ignored
- individual remote-only files can be hydrated deliberately

## Write Races

There are two possible access paths to a remote file:

- local mirror file synchronized by Mutagen
- remote buffer opened over SSH

If the same file is edited through both paths, writes can race. The safest rule
is:

- synced files open locally from the mirror
- ignored files open remotely unless explicitly hydrated
- hydrated files should be opened from the mirror while hydration is active

`editr.nvim` follows this rule for the actions it owns.

## Git Risk With Ignored Tracked Files

Ignoring tracked files is inherently risky. If a tracked path is excluded from
the mirror, local Git may report it as missing or deleted, and tools like
Neogit may show confusing state.

There is no general way for `editr` to make Git both local and complete while
also excluding tracked files from the local mirror.

Practical guidance:

- ignore untracked artifacts such as logs, caches, checkpoints, and generated
  outputs
- avoid ignoring tracked source, config, or data files needed by Git workflows
- document project-specific ignores carefully
- use remote browsing and hydration for huge untracked artifacts

## Why the Neovim Plugin Exists

Core `editr` is editor-agnostic. It exports context:

```text
EDITR_SESSION
EDITR_CONTEXT
EDITR_REMOTE_TARGET
EDITR_REMOTE_HOST
EDITR_REMOTE_PATH
EDITR_LOCAL_PATH
```

`editr.nvim` reads that context and adds editor-specific behavior:

- remote Snacks pickers that map results to local mirror paths
- canola/oil remote browsing entry points
- canola selection routing through hydration
- explicit `:EditrHydrate`
- cleanup for live per-file hydration sessions

This keeps the command-line tool useful for any terminal editor while giving
Neovim users a richer hybrid workflow.
