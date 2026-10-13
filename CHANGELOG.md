# Changelog

Every change to tiger that someone using it would notice. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html): below 1.0, a breaking change bumps the minor version, and each one comes with a migration note an agent can apply mechanically.

## [Unreleased]

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

[unreleased]: https://github.com/mareszhar/tiger/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/mareszhar/tiger/releases/tag/v0.1.0
