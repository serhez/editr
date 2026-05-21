# Editor Setup

`editr` is editor-agnostic. It creates or resumes the mirror, changes the
editor process working directory to that mirror, exports `EDITR_*` environment
variables, and starts the configured editor command.

The editor command is a shell command:

- If it contains `{path}`, `editr` substitutes the local mirror path.
- Otherwise `editr` appends `.` and runs the command from the mirror.

## Neovim

Basic:

```sh
editr host:/repo --editor nvim
```

Config:

```toml
editor = "nvim"
```

With a custom startup command:

```toml
editor = "nvim --cmd 'set number' {path}"
```

For Neovim-specific remote pickers, canola/oil integration, and hydration, use
the optional `editr.nvim` companion plugin.

## Helix

```sh
editr host:/repo --editor 'hx {path}'
```

Config:

```toml
editor = "hx {path}"
```

Helix will see the local mirror as an ordinary project directory.

## Vim

```sh
editr host:/repo --editor 'vim {path}'
```

Config:

```toml
editor = "vim {path}"
```

## Emacs Terminal Mode

```sh
editr host:/repo --editor 'emacs -nw {path}'
```

Config:

```toml
editor = "emacs -nw {path}"
```

## Micro

```sh
editr host:/repo --editor 'micro {path}'
```

Config:

```toml
editor = "micro {path}"
```

## Using `$EDITOR`

Shell expansion works because `editor` is a shell command:

```sh
editr host:/repo --editor '${EDITOR:-nvim} {path}'
```

In config, prefer an explicit editor command. It makes behavior easier to
reason about across shells and service environments.

## Environment Variables

The editor receives:

```text
EDITR=1
EDITR_CONTEXT
EDITR_SESSION
EDITR_REMOTE_TARGET
EDITR_REMOTE_HOST
EDITR_REMOTE_PATH
EDITR_LOCAL_PATH
EDITR_BIN
```

Editors can ignore these. Companion integrations can use them to map remote
paths back to the local mirror.

## Choosing an Editor Command

Use `{path}` if the editor expects a path argument:

```toml
editor = "hx {path}"
```

Omit `{path}` if you want `editr` to append `.`:

```toml
editor = "nvim"
```

These are equivalent for most terminal editors:

```toml
editor = "nvim"
editor = "nvim {path}"
```
