# tiger — architecture

How tiger is built: the path of a sync, the modules that own each step, the invariants that keep copies trustworthy, and the measurements behind the choices. Read this before changing `src/`.

## 1. The path of a sync

```text
tiger sync [--everywhere]
   │
   ▼
config.rs     find tiger.toml upward · parse + validate (every problem at once)
   │          --everywhere: + every project in the registry (store.rs)
   ▼
sync.rs       open the store (shared, or the fallback in the refs folder)
   │          lock stores in canonical path order, then projects in path order
   │          load each .tiger-state.json (state.rs)
   ▼
promotion     import selected fallback Git data and complete trees into the shared store
   ▼
plan          per ref: folder tiger didn't create → failed
   │                   synced tag/commit pin, same config → unchanged (no network)
   │                   commit pin → resolved
   ▼
resolve_all   one ls-remote per URL, union of patterns, 32 at a time (remote.rs)
   │          resolve each want: default branch · branch · tag (peeled) · clash → failed
   │          same commit + same config + folder exists → unchanged
   ▼
materialize   each (repo, commit) once: store.ensure_tree — repo lock, shallow fetch,
   │          extract with read-tree + checkout-index, read-only, rename into trees/
   │          each changed ref: clone tree → .tiger-tmp-<id> → filters → atomic swap (copy.rs)
   │          history = true: the project's own blob:none clone instead (history.rs)
   ▼
releases      tag-list digest changed → GitHub API → <id>.releases.md (releases.rs)
   ▼
finish        delete refs removed from the config (only ids in the state file)
              save state · write TIGER.md (pointer.rs) · update the registry
              clean verified redundant fallback data, retaining lock files
              report: one record per ref, exit 2 if any failed
```

`status` runs `plan`'s inputs and `ls-remote` but never writes. `add`, `rm`, `pin`, and `unpin` edit `tiger.toml` through `edit.rs`, then call `sync` for the refs they touched.

## 2. Modules

| Module | Owns |
| --- | --- |
| `main.rs` | CLI parsing (clap), dispatch, text vs JSON, exit codes, `tiger config` |
| `config.rs` | `tiger.toml`: discovery, parsing, validation, `RefEntry` |
| `edit.rs` | Comment-preserving edits to `tiger.toml` (`toml_edit`) |
| `commands.rs` | `add`, `rm`, `pin`, `unpin`: check first, then edit, then sync |
| `init.rs` | The usage header, `.gitignore`, first config |
| `pointer.rs` | `TIGER.md` |
| `progress.rs` | Progress on stderr: a line per step with real work, lock waits and retries as they happen, a heartbeat after two quiet seconds |
| `promotion.rs` | Fallback imports, separate failure reports, and verified duplicate cleanup |
| `remote.rs` | `Want` (what a ref asks for), `ls-remote`, resolution to `Resolved` |
| `sync.rs` | The sync pipeline above, reports |
| `store.rs` | Store paths, locks, fetch, extract, registry |
| `copy.rs` | Copy-on-write clones, atomic swap, filters, read-only files, removal |
| `history.rs` | `history = true` clones |
| `releases.rs` | GitHub release notes |
| `state.rs` | `.tiger-state.json`, the project lock |
| `status.rs` | Verdicts |
| `show.rs` | `show` and `store`: introspection without the network |
| `gc.rs` | Pruning the store |
| `settings.rs` | Global settings |
| `schedule.rs` | The launchd agent / systemd timer |
| `git.rs` | Running git: non-interactive, timeouts, error summaries |
| `url.rs` | URL shorthand, store keys, default ids |
| `util.rs` | Parallel map, atomic writes, small helpers |

## 3. Invariants

Each holds for every command. 1, 2, 5, and 6 have end-to-end tests; 3 and 4 are covered by unit tests and probes; 7 and 8 hold by construction.

1. **tiger deletes only what it created.** A ref folder is removed only when `.tiger-state.json` lists its id; a new ref whose folder already exists fails. The config is read and edited as usual even when it lives in the refs folder. Other user files there are never opened. Fallback cleanup removes only known cache trees and repos, retaining locks and unknown files.
2. **A failed ref keeps its copy and its state.** Nothing about a ref changes until its new copy is complete.
3. **Readers never see half a copy.** Copies are built in `.tiger-tmp-<id>-…` beside the destination and swapped in with `renamex_np(RENAME_SWAP)` (macOS) or `renameat2(RENAME_EXCHANGE)` (Linux). Store trees are extracted to `tmp/` and renamed into place, so a tree that exists is complete.
4. **Every git call is non-interactive, bounded, and independent of the working directory.** Network commands run from `/`, so only global and system git config apply. No prompts, no LFS downloads, SSH in batch mode; `ls-remote` has a 15s deadline with one retry, transfers abort when they stall. A timeout kills git's whole process group.
5. **The config is edited, never rewritten.** Edits go through `toml_edit`, and an edit that would make the config invalid is not saved.
6. **The network is asked once per URL per run,** and each `(repo, commit)` is fetched and extracted once per run, across every project.
7. **Locks never deadlock.** Before taking project locks, a sync captures existing fallback stores and acquires all store locks in canonical path order: the active store shared and promotion sources exclusive. It then locks projects in path order. Fetching/importing holds one destination repo lock at a time. `gc` holds its store lock exclusively. Source lock files survive cleanup, so a waiting process cannot acquire a replacement lock.
8. **Sync does not edit the config or Git ignore rules.** Generated output and state stay inside the refs folder; users choose whether to ignore them. Only `init`, `add`, `rm`, `pin`, and `unpin` edit the config or default ignore rules.

Promotion imports local Git objects, checks the destination commit, and publishes a complete tree before source data is removed. It clones an existing complete fallback tree to retain copy-on-write sharing, or extracts from Git when that cached tree is missing. As with existing shared trees, complete cache trees are trusted by construction; filtered project copies are never used as trees. Promotion preserves copy status and copied timestamps. State and registry writes precede cleanup. Import failures return exit 2 separately from ref failures; duplicate cleanup failures warn. Partial syncs retain unselected refs' source data. The state records the last sync store; `show` reads that record and the configured registry without probing writability.

## 4. Implementation choices

- **Rust, with a TOML config.** `toml_edit` keeps every comment and space through edits, which is what notes-as-comments needs. The release build is about 3 MB and links only system libraries.
- **git, not a git library.** The user's git config, credential helpers, SSH setup, and URL rewrites apply exactly as they do on the command line.
- **Trees are extracted with `read-tree` + `checkout-index`** into a temporary index, not `git archive`, which drops `export-ignore` files.
- **Fetch by refname for branches and tags, by SHA for commits,** with a full fetch as fallback for servers that refuse SHAs. A branch that moved between `ls-remote` and the fetch is copied at the newer commit, which is the commit the report and state record.
- **The whole tree is cloned in one `clonefile(2)` call on macOS**, falling back to per-file clones (`FICLONE` on Linux) and then plain copies across volumes or filesystems.
- **Copies are read-only files in writable folders.** Filters and swaps can delete files; editors and agents can't change them.

## 5. Measurements

Recorded on an M-series Mac (APFS), October 2026. Re-measure when the sync path changes.

| What | Result |
| --- | --- |
| Release build | 3,238,144 bytes (3.1 MiB); links libSystem and libiconv |
| Clone a 50,000-file, 195 MB tree | `clonefile` on the folder 0.61s · per-file clones 8.5s · plain copy 18.1s |
| Check 100 GitHub repos (no-op, `tiger status`) | 15–34s, network-bound: ~2.2s per GitHub round trip from this machine. Concurrency 32 beat 16 and 64 |
| Same, before stall retries | 50–83s, with single `ls-remote`s stalling for minutes |
| No-op sync of 3 refs (2 branches, 1 pinned tag) | ~1–4.5s: one round trip per branch |
| First sync of big repos with many tags | next.js (4,099 tags): 50s, 47 MB in the store, 34,025 files · TanStack/router (4,366 tags): 15s, 11 MB · react: 16s, 11 MB. No tags fetched; each store repo holds 2 refs. No-op sync of all three: 4s |

## 6. Where to change what

| To change… | Start in |
| --- | --- |
| A config field | `config.rs` (parse, validate), `state.rs` (`matches`, if it changes the copy), `edit.rs` (if commands write it), [config reference](../reference/config.md) |
| What counts as changed | `RefState::matches`, `plan` and `resolve_all` in `sync.rs` |
| How copies are made | `copy.rs`, `build_copy` in `sync.rs` |
| Network behavior | `git.rs` (timeouts, environment), `remote.rs` |
| Output | `output.rs` (text) and the serialized structs (JSON); [CLI reference](../reference/cli.md) |
