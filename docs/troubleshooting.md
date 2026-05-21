# Troubleshooting

This guide covers the failure modes most likely to look confusing during remote
editing.

## `editr` Hangs Before the Editor Opens

Run the SSH check directly:

```sh
ssh -o BatchMode=yes -o ConnectTimeout=10 host 'pwd'
```

If that fails, fix SSH before debugging `editr`.

Common causes:

- The SSH agent socket is stale.
- The key is not loaded into the agent.
- The remote login node accepts authentication but cannot start a command
  channel quickly.
- Too many existing SSH, sshfs, Mutagen, canola, or oil connections are open.

Useful checks:

```sh
echo "$SSH_AUTH_SOCK"
ssh-add -l
ssh host 'echo ok'
```

On macOS, if you use a persisted agent environment, make sure the socket exists
and points to a live agent:

```sh
source ~/.ssh/agent/env
ssh-add -l
```

## First Sync Is Too Slow

The first open must copy real files into the mirror. Large generated outputs can
make that expensive.

Check the plan first:

```sh
editr host:/repo --dry-run
```

Then add ignores for files that should not be part of local tooling:

```sh
editr host:/repo --ignore 'wandb/' --ignore 'checkpoints/' --ignore '*.pt'
```

For persistent rules, use `~/.config/editr/config.toml`.

Good candidates are caches, logs, checkpoints, datasets, and generated outputs.
Avoid ignoring tracked source/config files if you use local Git tools.

## Mutagen Session Keeps Running

Normal `editr` sessions terminate when the editor exits. A hard crash, killed
terminal, power loss, or stuck remote connection can leave sessions behind.

Inspect sessions:

```sh
editr list
mutagen sync list
```

Stop one session:

```sh
editr stop host:/absolute/remote/path
editr stop editr-session-name
```

Stop all editr-tracked sessions:

```sh
editr stop --all
```

Use the watcher to report suspicious sessions:

```sh
editr watch
editr watch --once --json
```

## Local Mirror Is Non-Empty and `editr` Refuses to Start

`editr` refuses to bootstrap over an unmarked non-empty mirror because that can
push accidental local files to the remote.

If the mirror is definitely the correct editr mirror and you want to reconcile
it:

```sh
editr host:/repo --no-bootstrap --allow-nonempty
```

If you are not sure, inspect or move the mirror first.

## Local Git Looks Wrong

By default, `editr` syncs `.git` so local Git and Neogit work.

Local Git can look wrong when:

- `.git` was ignored with `--ignore-vcs` or `sync_vcs = false`.
- A tracked file or directory is ignored by Mutagen.
- The mirror is only partially synced because a session is still staging files.

Check:

```sh
git -C ~/remote/host/path status
editr status host:/repo
```

Do not ignore tracked source or config paths if local Git correctness matters.

## Remote Pickers Fail in Neovim

For `editr.nvim` remote Snacks pickers, check SSH and the remote tools:

```sh
ssh host 'cd /absolute/remote/project && command -v rg; command -v git; command -v grep'
```

The grep picker prefers `rg`, then `git grep`, then `find + grep`.

If `rg` exists in an interactive SSH shell but not in the picker, it is probably
only added by interactive shell startup. Put it on PATH for non-interactive SSH
commands or use the fallback commands.

## Hydrated File Keeps Syncing

Live hydration creates a per-file Mutagen session while the owning editor
buffer is open. If Neovim crashes, that session can outlive the buffer.

Inspect and stop it:

```sh
editr list
editr stop editr-hydrate-...
```

The watcher can auto-stop stale hydration sessions:

```sh
editr watch --auto-stop-hydration-after 10m
```

## Passphrase Prompts Repeat

This is usually an SSH agent issue, not an `editr` issue.

Check:

```sh
ssh-add -l
ssh host true
```

If `ssh-add -l` cannot connect to the agent, refresh the agent environment or
start a new agent according to your shell setup.

On macOS:

```sh
ssh-add --apple-load-keychain
```
