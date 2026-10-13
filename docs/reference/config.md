# Configuration reference

Two files configure tiger: the project config `tiger.toml`, committed with the project, and the global settings file, kept per machine.

## `tiger.toml`

```toml
# Notes and the usage header are plain TOML comments.
dir = "tiger"
propagate = true

[refs]
# Follows the default branch.
tiptap = "https://github.com/ueberdosis/tiptap"

# Pinned to a tag, written inline.
zod = { url = "https://github.com/colinhacks/zod", ref = "v3.4.2" }

# Options, written as a table.
[refs.big]
url = "https://github.com/org/big"
include = ["src/**", "docs/**"]
exclude = ["**/*.snap"]
```

### Top level

| Field | Default | Meaning |
| --- | --- | --- |
| `dir` | `"tiger"` | The refs folder, relative to the project. Must stay inside the project and must not be hidden (no segment starting with `.`): search tools skip hidden folders |
| `propagate` | `true` | The default for every ref's `propagate` |
| `refs` | empty | The refs: id → URL string or table |

### Refs

The key is the **id**: the ref's folder name. Ids use letters, digits, `.`, `_`, and `-`, start with a letter or digit, and differ from each other in more than case. `TIGER.md` and names ending in `.releases.md` are reserved.

A ref is a URL string, or a table (inline or `[refs.<id>]`) with these fields:

| Field | Default | Meaning |
| --- | --- | --- |
| `url` | required | Any git URL: HTTPS, SSH (`git@host:path`, `ssh://…`), `file://`, or an absolute path |
| `ref` | the default branch | A branch to follow, or a tag or full 40-character commit SHA to pin. `refs/heads/<name>` and `refs/tags/<name>` are exact; a bare name that is both a branch and a tag is an error. Short SHAs are rejected |
| `include` | everything | Globs of files to keep, relative to the repo root (`src/**`) |
| `exclude` | nothing | Globs of files to drop, applied after `include` |
| `history` | `false` | Make the copy the project's own `--filter=blob:none` clone, so `git log` and `git diff` work in place. Not combinable with `include`/`exclude` |
| `releases` | `false` | Write `<id>.releases.md` beside the copy from GitHub Releases. GitHub only; elsewhere the ref syncs and warns |
| `propagate` | top-level `propagate` | `false`: `sync --everywhere` run from another project leaves this ref alone |

Globs follow [globset](https://docs.rs/globset) syntax: `*` stays within a folder, `**` crosses folders, `{a,b}` alternates. Folders left empty by filters are removed.

tiger reports every invalid field at once, with the valid names and a suggestion for typos. `tiger add`, `rm`, `pin`, and `unpin` edit the file without touching other entries, comments, or formatting, and refuse to save an edit that would make the config invalid.

### The refs folder

| Path | Owner | Contents |
| --- | --- | --- |
| `<id>/` | tiger | The copy: read-only files. With `history = true`, a git clone |
| `<id>.releases.md` | tiger | Release notes (`releases = true`) |
| `TIGER.md` | tiger | The pointer for agents; rewritten when out of date |
| `.tiger-state.json` | tiger | What was last synced, per id. Deleting it makes tiger treat existing copies as unmanaged |
| `.tiger-lock`, `.tiger-tmp-*` | tiger | A sync's lock and in-progress copies |
| `.tiger-store/` | tiger | The fallback store, when the shared one isn't writable |
| anything else | you | Never read, changed, or deleted by tiger |

## Global settings

`$XDG_CONFIG_HOME/tiger/config.toml`, default `~/.config/tiger/config.toml`. Change them with `tiger config set <key> <value>`; the file keeps any comments you add.

| Key | Default | Meaning |
| --- | --- | --- |
| `dir` | `tiger` | The refs folder `tiger init` writes into new configs |
| `store` | `~/.tiger` | The shared store. `TIGER_HOME` overrides it |
| `concurrency` | `32` | Network operations at once (1–64) |
| `schedule` | `off` | Run `tiger sync --everywhere --quiet` in the background: `off`, or an interval such as `30m`, `6h`, `1d` (minimum `15m`) |

### The background job

Setting `schedule` installs or replaces the job; `off` (or `tiger config unset schedule`) removes it.

| System | Job | Log |
| --- | --- | --- |
| macOS | launchd agent `tiger.sync` in `~/Library/LaunchAgents/tiger.sync.plist` | `<store>/logs/schedule.log` |
| Linux | systemd user units `tiger-sync.service` and `tiger-sync.timer` | `<store>/logs/schedule.log` |

The job runs the `tiger` on your `PATH` when that is the same binary (so upgrades keep working), from your home folder, with your current `PATH`, `TIGER_HOME`, and `XDG_CONFIG_HOME`. Tokens are never written into job files.

## The store

```text
~/.tiger/
  repos/<host>/<path>.git      shallow bare clone, one per repo (HTTPS and SSH URLs share one)
  trees/<host>/<path>/<sha>/   read-only files of one commit
  registry.json                projects that use the store
  locks/  logs/  tmp/
```

Deleting the store loses nothing: each project rebuilds on its next sync and registers again. `tiger gc` removes what no project uses.
