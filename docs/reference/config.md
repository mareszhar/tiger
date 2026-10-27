# Configuration reference

Two files configure tiger: the project config `tiger.toml`, and the global settings file, kept per machine. Commit the project config to share its refs, or keep it locally.

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
| `dir` | `"tiger"` | The refs folder, relative to the folder with `tiger.toml`. `"."` puts copies beside the config. Other paths must stay inside the project and must not have hidden segments: search tools skip hidden folders |
| `propagate` | `true` | The default for every ref's `propagate` |
| `refs` | empty | The refs: id → URL string or table |

### Refs

The key is the **id**: the ref's folder name. Ids use letters, digits, `.`, `_`, and `-`, start with a letter or digit, and differ from each other in more than case. `tiger.toml`, `TIGER.md`, and names ending in `.releases.md` are reserved, without regard to case.

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

Globs follow [globset](https://docs.rs/globset) syntax: `*` stays within a folder, `**` crosses folders, `{a,b}` alternates (`**/*.{snap,svg}`). Folders left empty by filters are removed.

Filters shape the copy, not the download: sync fetches and stores each commit whole, then leaves out of the copy what the filters drop. Use them to keep agents in the parts of a repo worth reading. Excluding images, fonts, or other media to save space gains almost nothing where copies share disk space with the store (APFS, Btrfs, XFS); elsewhere each copy is a full copy, and filters do make it smaller.

tiger reports every invalid field at once, with the valid names and a suggestion for typos. `tiger add`, `rm`, `pin`, and `unpin` edit the file without touching other entries, comments, or formatting, and refuse to save an edit that would make the config invalid.

### The refs folder

| Path | Owner | Contents |
| --- | --- | --- |
| `<id>/` | tiger | The copy: read-only files. With `history = true`, a git clone |
| `<id>.releases.md` | tiger | Release notes (`releases = true`) |
| `TIGER.md` | tiger | The pointer for agents; rewritten when out of date |
| `.tiger-state.json` | tiger | What tiger last put here, per id, and which store it used. Deleting it makes tiger treat existing copies as not its own |
| `.tiger-lock`, `.tiger-tmp-*` | tiger | A sync's lock and in-progress copies |
| `.tiger-store/` | tiger | The fallback store, when the shared one isn't writable. Removed once its data has moved to the shared store |
| anything else | you | Never read, changed, or deleted by tiger |

With `dir = "."`, the config lives here too; tiger reads and edits it as usual and leaves everything else alone. Whether the folder is ignored is up to you.

Configs can be nested, as long as each has its own refs folder: tiger refuses a config whose refs folder another config already uses. The refs folder must also stay inside the project, symlinks included, and can't be an existing file.

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

A repo's folder comes from its URL, so ids never matter here: two projects can call the same repo different things, or use the same id for different repos. HTTPS and SSH URLs of one repo share a folder, and on GitHub, GitLab, Bitbucket, and Codeberg, which ignore letter case in owner and repo names, so do spellings that differ only in case (the folder uses lowercase). On other hosts, case can tell repos apart, so it's kept.

Deleting the store loses nothing: each project rebuilds on its next sync and registers again. `tiger gc` removes what no project uses.
