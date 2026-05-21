# Hydration

Hydration copies or syncs one remote file into the local editr mirror. It is the
bridge between lazy remote browsing and local editor tooling.

```sh
editr hydrate \
  --context "$EDITR_CONTEXT" \
  --remote-path /remote/repo/logs/run.txt \
  --mode live \
  --json
```

Use `--check --json` to inspect size and path mapping without syncing:

```sh
editr hydrate --context "$EDITR_CONTEXT" --remote-path /remote/repo/logs/run.txt --check --json
```

## Modes

- `oneshot`: run a remote-to-local file sync, flush it, and terminate the
  temporary Mutagen session immediately.
- `live`: bootstrap the file, then keep a per-file two-way Mutagen session until
  the editor/plugin stops it.

`editr.nvim` uses live mode by default so edits to hydrated ignored files can
sync back to the remote while the buffer is open.

## Size Limits

`--max-size` rejects hydration above a threshold unless `--allow-large` is
passed. The Neovim plugin first runs `--check`, auto-hydrates files under the
threshold, and prompts for larger or unknown-size files.

Remote size checks are best-effort. A file can grow between the size check and
the actual sync, so large-file prompts are a guardrail rather than a guarantee.

## Risks

Hydrating one ignored file creates a partial local view of that ignored path.
This is intentional: ignored paths are not normal mirrored directories.

Do not ignore tracked files if local Git or Neogit correctness matters. Git
cannot simultaneously see a complete local repository and have tracked paths
excluded from the mirror.
