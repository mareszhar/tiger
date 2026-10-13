# CLI reference

Every command is non-interactive and safe to re-run. Commands find the project by looking for `tiger.toml` in the working directory and its parents.

## Contents

- [Global flags](#global-flags) · [Exit codes](#exit-codes)
- [`init`](#tiger-init) · [`add`](#tiger-add) · [`rm`](#tiger-rm) · [`sync`](#tiger-sync) · [`pin`](#tiger-pin) · [`unpin`](#tiger-unpin) · [`status`](#tiger-status) · [`gc`](#tiger-gc) · [`config`](#tiger-config)
- [JSON output](#json-output) · [Environment](#environment)

## Global flags

| Flag | Effect |
| --- | --- |
| `--json` | Print the result as JSON on stdout, errors included (`{ "error": … }`) |
| `-q`, `--quiet` | Print only failures and warnings |
| `--cwd <dir>` | Run as if started in `<dir>` |
| `-h`, `--help` | Short help; `--help` also shows the `tiger.toml` fields and examples |
| `-V`, `--version` | Print the version |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Done |
| `1` | Usage or config error, or the command couldn't run; nothing was changed |
| `2` | Some refs failed; every other ref synced. Failed refs kept their existing copies |

## `tiger init`

```sh
tiger init [--dir <dir>] [--no-gitignore]
```

Creates `tiger.toml` in the working directory with a usage header, `dir`, and an empty `[refs]`; creates the refs folder and `TIGER.md`; appends `/<dir>/` to `.gitignore` unless git already ignores it. Prints a line to add to agent instructions.

- `--dir` defaults to the `dir` setting, else `tiger`.
- `--no-gitignore` leaves `.gitignore` alone; the header then doesn't claim the folder is ignored.
- Re-running is safe: an existing `tiger.toml` is kept, and the gitignore line is never added twice.

## `tiger add`

```sh
tiger add <url> [--as <id>] [--ref <branch|tag|sha>] [--note "…"]
          [--include <glob>…] [--exclude <glob>…] [--history] [--releases] [--no-propagate] [--no-sync]
```

Adds a ref to `tiger.toml`, then syncs it.

- `<url>` is any git URL (HTTPS, SSH, `file://`), a local path (stored absolute), or GitHub shorthand `owner/repo`.
- The id defaults to the repo name (`ueberdosis/tiptap` → `tiptap`).
- Without `--ref`, the ref follows the remote's default branch.
- The remote is checked (and the branch, tag, or commit found) before anything is written. An unreachable URL or a missing ref exits 1 and leaves the config as it was.
- A ref with only a URL is written as `id = "url"` under `[refs]`; one with options as a `[refs.<id>]` table. `--note` becomes comment lines above the entry, wrapped at 78 columns.
- Adding a URL that is already a ref (without `--as`) changes nothing and syncs it. Adding it again with options exits 1: change the entry in `tiger.toml` instead.
- With no `tiger.toml` anywhere above, `add` first runs `init` at the git repository's root (or the working directory outside git).

## `tiger rm`

```sh
tiger rm <id>…
```

Removes the entries (with the comments above them) and deletes their copies and release notes. A folder tiger didn't create is never deleted. Unknown ids exit 1 before anything changes.

## `tiger sync`

```sh
tiger sync [<id>…] [--everywhere] [--force]
```

Makes copies match the config, fetching only what changed upstream.

- With ids, only those refs sync, and refs removed from the config are left for a full sync.
- A ref is **unchanged** when its URL, `ref`, filters, and `history` match what was last synced, its folder exists, and the upstream commit is the same. A pinned tag or commit that is synced needs no network.
- A ref whose folder exists but isn't in the state file fails instead of overwriting it.
- A ref that fails keeps its existing copy. The rest still sync; the exit code is 2.
- Refs removed from the config have their copies and release notes deleted.
- A ref with `releases` turned off has its `<id>.releases.md` deleted.
- `--force` rebuilds every selected copy even when nothing changed.
- `--everywhere` also syncs every project in the store's registry, as each project's own sync would, except that refs with `propagate = false` in other projects are skipped. With ids, other projects sync only refs for the same repos. It runs without a current project (the scheduled job does), forgets registered projects whose `tiger.toml` is gone, and needs a writable shared store.

Output, one line per ref that didn't stay unchanged:

```text
  + new        framework  main → 4f2a91c
  ~ changed    tiptap     main 88a5dd3 → 1c9e0b2
  ! failed     internal   git: git@github.com: Permission denied (publickey). · kept existing copy
  - removed    old-lib
  = unchanged  39 refs
synced 2 · unchanged 39 · failed 1 · removed 1 · 4.8s
```

## `tiger pin`

```sh
tiger pin <id> [<tag|sha>] [--no-sync]
```

Sets the ref's `ref` field, keeping its notes, then syncs it.

- Without a target, pins the commit synced now (`tiger sync <id>` first if it never synced).
- A tag must exist on the remote. A full 40-character SHA is taken as is.
- A branch is refused: a branch follows, and a pin doesn't move. The error shows how to follow it or pin its current commit.

## `tiger unpin`

```sh
tiger unpin <id> [--no-sync]
```

Removes the ref's `ref` field, so it follows the default branch again, then syncs it.

## `tiger status`

```sh
tiger status [--offline]
```

For each ref: what it tracks, the commit on disk, the upstream commit, and a verdict. Never writes anything. `--offline` skips the remotes.

| Verdict | Meaning |
| --- | --- |
| `up to date` | The copy is at the branch's latest commit |
| `behind` | Upstream moved; `tiger sync` updates it (also: a pinned tag that moved upstream) |
| `pinned` | A tag or commit, synced |
| `not synced` | No copy yet |
| `config changed` | The entry changed since its last sync (URL, `ref`, filters, `history`) |
| `removed from config` | The next sync deletes its copy |
| `unknown` | Offline, or the remote didn't answer; `detail` says which |

A ref whose copy is a full copy (no copy-on-write) shows `· full copy`.

```text
tiptap    main (default branch)  1c9e0b2  up to date
zod       tag v3.22.4            e7a9b9b  pinned
internal  main (default branch)  4f2a91c  unknown · git: Permission denied (publickey).
```

## `tiger gc`

```sh
tiger gc
```

Removes from the shared store every tree and repo that no registered project's state uses, forgets registered projects whose `tiger.toml` is gone, and compacts the remaining repos. Stops without deleting anything if a registered project's config can't be read. Waits for running syncs.

## `tiger config`

```sh
tiger config [list]
tiger config get <key>
tiger config set <key> <value>
tiger config unset <key>
```

Shows or changes [global settings](./config.md#global-settings). `list` (the default) shows every setting, its default, and whether the background job is installed. Setting `schedule` installs, replaces, or removes the background job.

## JSON output

`--json` prints one JSON document. Commit SHAs are full; paths are relative to the project unless named `project`.

`tiger sync --json`:

```json
{
  "project": "/work/app",
  "refs": [
    { "id": "tiptap", "status": "changed", "tracks": "main", "from": "88a5dd3…", "to": "1c9e0b2…", "path": "tiger/tiptap" },
    { "id": "internal", "status": "failed", "tracks": "main", "path": "tiger/internal",
      "error": "git: Permission denied (publickey). · kept existing copy" }
  ],
  "summary": { "synced": 1, "unchanged": 39, "failed": 1, "removed": 0, "ms": 4800 },
  "warnings": []
}
```

`tiger sync --everywhere --json` has `projects` (each `{ project, refs, summary, error? }`), `summary` across all of them, and `warnings`. `tiger add`, `pin`, and `unpin` include the sync they ran under `sync`.

`tiger status --json`:

```json
{
  "project": "/work/app",
  "refs": [
    { "id": "tiptap", "tracks": "main (default branch)", "local": "1c9e…", "upstream": "1c9e…",
      "verdict": "up-to-date", "copy_mode": "clone", "path": "tiger/tiptap" }
  ]
}
```

`copy_mode` is `clone` (copy-on-write), `copy` (full copy), or `git` (`history = true`). Errors:

```json
{ "error": { "message": "unknown ref 'tiptp'", "context": ["refs: tiptap, zod"], "hint": "did you mean 'tiptap'?" } }
```

## Environment

| Variable | Effect |
| --- | --- |
| `TIGER_HOME` | The shared store's location (overrides the `store` setting) |
| `XDG_CONFIG_HOME` | Settings live in `$XDG_CONFIG_HOME/tiger/config.toml` |
| `GITHUB_TOKEN`, `GH_TOKEN` | Token for the GitHub Releases API; otherwise `gh auth token`, otherwise anonymous |
| `GIT_SSH_COMMAND` | Used as is; when unset, tiger runs SSH with `BatchMode=yes` and timeouts |

tiger runs network git commands outside any repository, so your global and system git config apply (credential helpers, SSH config, `insteadOf` rewrites) and a project's local config doesn't. It runs git with prompts disabled (`GIT_TERMINAL_PROMPT=0`) and LFS downloads skipped (`GIT_LFS_SKIP_SMUDGE=1`). An `ls-remote` that doesn't answer in 15s is retried once; a transfer that stalls below 1 KB/s for 30s is aborted.
