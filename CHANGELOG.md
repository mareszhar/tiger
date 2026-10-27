# Changelog

Every change to tiger that someone using it would notice. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html): below 1.0, a breaking change bumps the minor version, and each one comes with a migration note an agent can apply mechanically.

## [Unreleased]

### Changed

- **`TIGER.md` says filters keep a copy focused, not small.** tiger downloads and stores each repo whole whatever `include` and `exclude` say, and copies share disk space with the store, so agents no longer need to exclude images, fonts, and other media to save space. Each refs folder's `TIGER.md` updates on its next sync.
- The README's filter example and the configuration reference describe filters as a way to keep agents in the parts of a repo worth reading, and say when they do save space: only where copies are full copies.

## [0.2.4] - 2026-10-14

### Changed

- `tiger --help` lists the exit codes one per line as `1 = usage or config error`, so they no longer read like a report of failures.

## [0.2.3] - 2026-10-14

### Changed

- **One store entry per repo, whatever the letter case.** On GitHub, GitLab, Bitbucket, and Codeberg, where `TanStack/router` and `tanstack/router` are the same repo, both spellings now share one entry in the store instead of being downloaded twice. Elsewhere, case still tells repos apart. Existing store folders are renamed to match on the first sync or `tiger gc` that has the store to itself; nothing is downloaded again.

### Fixed

- **An emptied fallback store is removed.** Once a project's fallback store has moved all its data into the shared store, sync now deletes its `.tiger-store/` folder instead of leaving empty folders behind. A fallback store holding files that aren't tiger's is left alone.

## [0.2.2] - 2026-10-14

### Fixed

- **Copies made inside a sandbox end up sharing disk space.** When a project first synced inside a sandbox, its copies came from its own fallback store. If the shared store already had those commits, the copies kept their own disk space even after promotion. The next sync with a writable shared store now re-clones them from it, without changing their files, status, or copied time. Projects already promoted get the same result with one `tiger sync --force`.
- The getting-started guide explains that Finder and `du` count each copy at full size even though copies share disk space with the store.

## [0.2.1] - 2026-10-13

### Changed

- **Quieter progress.** While a step runs, sync adds an update only when the step advanced, at most every 5 seconds, plus one every 15 seconds when nothing moved, instead of repeating itself every 2 seconds. Copying and fallback-store cleanup now announce themselves when they start.
- **The getting-started guide shows how to upgrade:** `brew update && brew upgrade tiger`, since `brew upgrade` alone can miss a new release for up to a day.

## [0.2.0] - 2026-10-13

Config and copies can now share a folder, long syncs show what they're doing, and a project synced inside a sandbox catches up with the shared store on its own.

**After upgrading,** run `tiger sync --everywhere` once from a terminal, plus `tiger sync` in any project that has only ever synced inside a sandbox (it isn't registered yet). It rewrites each project's `TIGER.md` with the new instructions, corrects refs pinned to annotated tags, and moves any fallback-store data into the shared store. Configs need no changes.

### Added

- **Config and copies in one folder.** `tiger init --dir .` keeps the copies beside `tiger.toml`, for example all inside `__references__/`, and leaves your ignore files alone. tiger works outside Git too, and whether to commit the config is up to you.
- **Progress during sync.** Sync says on stderr what it's doing as it goes: checking repos, fetching commits, waiting for another tiger process, retrying a stalled remote. A long sync never looks like a hang. `--quiet` hides it, and `--json` output is unchanged.
- **`tiger add --no-sync --no-validate`** saves a ref without contacting its remote, so adding many refs is instant; the next sync checks them all at once.
- **`tiger show` starts with the store:** the shared store, whether the project is registered for `--everywhere`, and whether the last sync had to use a fallback store.
- **`tiger show` prints each ref's note,** the comment above its entry, so an agent learns why a ref is there without opening `tiger.toml`. JSON gives it as `note`.
- **`tiger status <id>…`** checks only the refs you name, like `sync`, `show`, and `rm`.

### Changed

- **`tiger add` writes refs with options as inline tables**, `id = { url = "…", ref = "…" }`, under `[refs]`, so entries stay in the order you added them instead of options-bearing refs moving below the rest. Entries already written as `[refs.<id>]` tables stay valid and untouched. If a script of yours looked for `[refs.<id>]` headers, have it parse the TOML instead.
- **Notes keep their shape.** `--note` text keeps its paragraphs and list items when wrapped at 78 columns.
- **Hints and generated instructions use the shortest command that works** from where they'll be run: plain `tiger sync` inside the project, `tiger --cwd <folder> sync` from a parent folder.
- **`tiger rm` says where the data went:** the store keeps it for other projects, and `tiger gc` frees what no project uses.
- **`tiger.toml` and `TIGER.md` are reserved ids**, in any letter case, so no ref can collide with them. A ref that used one (unlikely) needs renaming by hand: stop syncs, rename its folder, and rename its key in both `tiger.toml` and `.tiger-state.json`.

### Fixed

- **Annotated tags pin the commit they point at.** A ref pinned to an annotated tag could record the tag's own ID instead of its commit, so `tiger show` printed the wrong commit and `tiger status` reported the tag as moved when it hadn't. Run `tiger sync` once: it corrects any ref recorded this way (it shows as `changed`, with the same files).
- **Data from a sandbox reaches the shared store.** After a sync inside a sandbox used the fallback store, the next sync that can write the shared store now moves that data into it, even for refs that didn't change, and then cleans up the fallback store. Before, unchanged refs stayed only in the fallback store.
- Two configs can no longer use the same refs folder, which would have mixed up which copies each one owns.
- A refs folder can't point outside the project through a symlink, or be an existing file.

The getting-started guide now covers what happens when you delete or move folders by hand (nothing breaks), keeping a config's folder out of commits, and moving copies to another folder; a separate guide moves them beside their config while keeping pins whose repo is gone. Per-ref folders are deferred, with the thinking kept in `docs/explorations/`.

## [0.1.0] - 2026-10-13

The first release: read-only, up-to-date copies of other repos inside your project, so agents can read the source instead of guessing.

### Added

- **Refs in `tiger.toml`.** Each entry is a git repo to copy into the refs folder (`tiger/` by default), with notes about it as plain comments above it. A URL alone follows the repo's default branch, whatever it is named; `ref` follows another branch or pins a tag or commit. `include` and `exclude` keep only the files you want.
- **`tiger sync` fetches only what changed.** One `git ls-remote` per repo decides; a changed ref downloads one commit, with no history and no tags, so repos with thousands of tags (next.js, TanStack) sync as quickly as small ones. Synced pins need no network at all.
- **One store per machine.** Each repo is downloaded once into `~/.tiger`, however many projects use it, and every project gets real, read-only folders cloned copy-on-write from it: real files for search tools and sandboxed agents, almost no extra disk.
- **Every project at once.** `tiger sync --everywhere` updates every project that uses a changed repo, checking each repo once. `tiger config set schedule 6h` runs it in the background (launchd on macOS, a systemd timer on Linux). `propagate = false` lets a project or a ref update only on its own schedule.
- **Commands that edit the config and keep your comments:** `tiger init`, `tiger add <url|owner/repo> [--note …]`, `tiger rm`, `tiger pin <id> [<tag|sha>]`, `tiger unpin`.
- **Introspection:** `tiger status` shows which refs are behind upstream; `tiger show [<id>]` shows the commit, date, and subject each copy holds, when it was copied and last checked, and which other projects use the same repo; `tiger store` shows what the shared store holds and who uses it.
- **Safe by default.** A failed ref keeps its existing copy while the rest sync (exit code 2). tiger only deletes what it created, so your own files in the refs folder stay. Changed copies are swapped in atomically, so a reader never sees half a repo. A remote that stalls is retried once, then reported, instead of holding up the sync.
- **Built for agents.** Non-interactive commands, `--json` on every command, errors that name the fix, a usage header in `tiger.toml`, and a `TIGER.md` pointer in the refs folder.
- **Options per ref:** `history = true` makes the copy a real clone (`git log` and `git diff` work in place); `releases = true` writes `<id>.releases.md` from GitHub Releases.
- **Private repos** through your global git config: SSH keys, credential helpers, or URL rewrites. tiger never handles credentials and never prompts.
- **Sandbox fallback.** When the shared store isn't writable, tiger keeps a private store inside the refs folder and says so.
- **`tiger gc`** prunes what no project uses from the store; **`tiger config`** shows and changes global settings (`dir`, `store`, `concurrency`, `schedule`).
- Binaries for macOS and Linux (arm64 and x64), installable with `brew install mareszhar/tap/tiger`.

[unreleased]: https://github.com/mareszhar/tiger/compare/v0.2.4...HEAD
[0.2.4]: https://github.com/mareszhar/tiger/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/mareszhar/tiger/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/mareszhar/tiger/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/mareszhar/tiger/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/mareszhar/tiger/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mareszhar/tiger/releases/tag/v0.1.0
