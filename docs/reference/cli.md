# CLI reference

Every command is non-interactive and safe to re-run. Commands find the project by looking for `tiger.toml` in the working directory and its parents.

## Contents

- [Global flags](#global-flags) · [Exit codes](#exit-codes)
- [`init`](#tiger-init) · [`add`](#tiger-add) · [`rm`](#tiger-rm) · [`sync`](#tiger-sync) · [`pin`](#tiger-pin) · [`unpin`](#tiger-unpin) · [`status`](#tiger-status) · [`show`](#tiger-show) · [`store`](#tiger-store) · [`gc`](#tiger-gc) · [`config`](#tiger-config)
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
| `1` | Usage or config error, or the command couldn't run. A rejected config edit is never saved |
| `2` | Part of a sync failed: some refs, or moving fallback data into the shared store. Everything else still synced; failed refs keep their copies, and data that didn't move stays in the fallback store |

## `tiger init`

```sh
tiger init [--dir <dir>] [--no-gitignore]
```

Creates `tiger.toml` in the working directory with a usage header, `dir`, and an empty `[refs]`; creates the refs folder and `TIGER.md`; appends `/<dir>/` to `.gitignore` unless git already ignores it. Works outside Git too.

Prints a line for your agent instructions, written for the folder those instructions live in: the Git root, or the project folder outside Git. Its commands add `--cwd` when the config is in a subfolder. The `Next:` command, like every hint tiger prints, is written for your shell's current folder.

- `--dir` defaults to the `dir` setting, else `tiger`.
- `--dir .` puts copies beside the config and leaves ignore files alone, since ignoring the folder would ignore the config too.
- `--no-gitignore` leaves `.gitignore` alone; the header then doesn't claim the folder is ignored.
- Re-running is safe: an existing `tiger.toml` is kept, and the gitignore line is never added twice.

## `tiger add`

```sh
tiger add <url> [--as <id>] [--ref <branch|tag|sha>] [--note "…"]
          [--include <glob>…] [--exclude <glob>…] [--history] [--releases] [--no-propagate] [--no-sync] [--no-validate]
```

Adds a ref to `tiger.toml`, then syncs it.

- `<url>` is any git URL (HTTPS, SSH, `file://`), a local path (stored absolute), or GitHub shorthand `owner/repo`.
- The id defaults to the repo name (`ueberdosis/tiptap` → `tiptap`).
- Without `--ref`, the ref follows the remote's default branch.
- Before saving, `add` checks that the remote answers and that a named branch or tag exists; if not, it exits 1 and saves nothing. A full commit SHA is checked when it syncs.
- A ref with only a URL is written as `id = "url"` under `[refs]`, and one with options as an inline table, `id = { url = "…", ref = "…" }`, so entries stay in the order you added them. Entries you wrote as `[refs.<id>]` tables stay as they are.
- `--note` becomes comment lines above the entry, wrapped at 78 columns. Paragraphs, blank lines between them, and list items keep their shape; existing comments are never reformatted.
- `--no-sync` saves the entry without copying it, after the remote check. Add `--no-validate` to skip that check too (it requires `--no-sync`); the next sync checks every entry at once. Invalid ids, globs, and options are still rejected immediately. `--json` reports `validation`: `checked`, `deferred`, or `not-checked` when the entry already existed.
- To add many refs at once, you can also write them into `tiger.toml` directly and run one sync.
- Adding a URL that is already a ref (without `--as`) changes nothing and syncs it. Adding it again with options exits 1: change the entry in `tiger.toml` instead.
- With no `tiger.toml` anywhere above, `add` first runs `init` at the git repository's root (or the working directory outside git).

## `tiger rm`

```sh
tiger rm <id>…
```

Removes the entries (with the comments above them) and deletes their copies and release notes. A folder tiger didn't create is never deleted. Unknown ids exit 1 before anything changes. The store keeps the repos' data for any other project that uses them; `tiger gc` frees what no project uses.

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
- **Promotion.** When a project has a fallback store and the shared store is writable, sync moves the selected refs' data from the fallback store into the shared store. It needs no network, keeps each copy's files, status, and copied time, and copies complete trees, never filtered copies.
  - A copy made from the fallback store, of a commit the shared store already had, is re-cloned from the shared store's tree, so the two share disk space instead of each holding its own.
  - Data that fails to move stays in the fallback store for the next sync to retry. The failure is reported separately from the refs, and the exit code is 2.
  - After a full sync has moved everything, the fallback store's cache is deleted. A sync of only some refs keeps whatever the others still need. If the cleanup itself fails, sync warns and still exits 0.
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

Store details appear only when they matter: trees promoted from the fallback store, fallback data still waiting to move, or a project the shared store couldn't register.

While it works, sync reports progress on stderr, so a long fetch never looks like a hang:

```text
tiger: syncing 42 refs
tiger: checking 40 repos upstream
tiger: fetching 3 commits
tiger: fetching commits · 1/3 done · 6.0s
```

It prints a line when work starts and when a step with real work begins (checking, fetching, copying, cleaning up a fallback store). While a step runs, it adds an update only when there is something new: at most every 5 seconds when the step advanced, and every 15 seconds when nothing did, so a long wait never looks like a hang. It says right away when it waits for another tiger process, and when a remote stalls and is retried. `--quiet` hides progress, and `--json` keeps stdout to the one final document.

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
tiger status [<id>…] [--offline]
```

For each ref, or only the ones named: what it tracks, the commit on disk, the upstream commit, and a verdict. Never writes anything. `--offline` skips the remotes.

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

## `tiger show`

```sh
tiger show [<id>…]
```

What each ref's copy holds, from the state file, the store, and the copy itself; never the network. Use it to confirm a ref picked up the commit you expected.

```text
store: ~/.tiger · registered

zod  https://github.com/colinhacks/zod
  note      Pinned: we target 3.22 until we migrate.
  tracks    tag v3.22.4 (pinned)
  commit    e7a9b9b  2023-10-12  v3.22.4  (Colin McDonnell)
  copied    2026-10-13 06:10 UTC (2h ago) · checked 5m ago
  copy      tiger/zod · 412 files · 2.1 MB · copy-on-write clone
  filters   include src/** · exclude **/*.test.ts
  also in   ~/code/editor as zod (8a1b2c3)
```

- **note**: the comment directly above the entry in `tiger.toml`, up to the nearest blank line, so a heading over a group of refs isn't taken for one ref's note.
- **commit**: the commit's short SHA, committer date, subject, and author. Without the store (deleted, or never shared), only the SHA.
- **copied**: when the copy was last made; **checked**: when a sync last confirmed it matches upstream (or its pin).
- **also in**: other registered projects with a ref to the same repo, and the commit each holds.
- **store** (the first line): the shared store, whether this project is registered there for `--everywhere`, and whether the last sync had to use the fallback store or left data in it. `show` only reads; it never tests or creates a store.
- A ref that never synced says `not synced yet`; one whose entry changed since says so, with the command that rebuilds it.

`--json` gives, per ref: `id`, `url`, `note` (when there is one), `ref`, `synced`, `kind`, `tracks`, `commit` (`sha`, `date`, `author`, `subject`), `copied_at` and `checked_at` (Unix seconds), `copy_mode`, `path`, `files`, `bytes`, `include`, `exclude`, `history`, `release_notes`, `elsewhere` (`project`, `id`, `commit`), `config_changed`.

The top-level `store` gives `configured_shared`, `last_sync` (`path` and `mode`, `shared` or `fallback`; null if no sync recorded it yet), `registered` (true, false, or null when the registry can't be read, with `registration_error`), `fallback_path`, `fallback_trees`, and `fallback_repos`.

## `tiger store`

```sh
tiger store
```

What the shared store holds, and who uses it; never the network, never writes.

```text
~/.tiger · 85.2 MB
projects
  ~/code/editor     4 refs · synced 2h ago
  ~/old/prototype   missing: `tiger gc` forgets it
repos
  github.com/colinhacks/zod · 6.3 MB
    e7a9b9b  2023-10-12  v3.22.4  →  ~/code/editor (zod), ~/code/docs (zod)
    1c9e0b2  2023-09-01  v3.22.2  →  unused: `tiger gc` removes it
```

Each repo lists one line per tree (a commit in use), newest first, with the projects and ids whose copy is that commit. `--json` gives `path`, `bytes`, `projects` (`project`, `refs`, `synced_at`, `exists`), and `repos` (`key`, `bytes`, `trees` with `commit`, `bytes`, `used_by`).

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

`--json` prints one JSON document. Commit SHAs are full. Copy and release-note paths are relative to the project; project and store paths identify their locations on the machine.

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
  "store": { "path": "/home/me/.tiger", "mode": "shared" },
  "registered": true,
  "promotion": { "promoted_trees": 0, "remaining_trees": 0, "remaining_repos": 0,
                 "failures": [], "cleanup_warnings": [] },
  "warnings": []
}
```

`tiger sync --everywhere --json` has `projects` (each `{ project, refs, summary, error? }`), `summary` across all of them, and `warnings`. `tiger add`, `pin`, and `unpin` include the sync they ran under `sync`.

Sync also gives `store` (`path`, and `mode`: `shared` or `fallback`), plus `registered` and `promotion` for the project (per project with `--everywhere`). `promotion` gives `promoted_trees`, `relinked` (copies re-cloned from the shared store), `remaining_trees` and `remaining_repos` (what the fallback store still holds), `failures` (each `{ repo, commit, error }`), and `cleanup_warnings`. `summary.failed` counts only refs; an exit code of 2 with no failed refs means a promotion failure.

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
