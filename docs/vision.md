# tiger — vision

> **tiger keeps read-only, up-to-date copies of other repos inside your project, so agents can read the source instead of guessing.**

The guiding question, for every decision:

> **Does this help an agent answer from the source?**

This document says why tiger exists, the model it is built on, the principles every change answers to, and what it deliberately is not. Read it before changing tiger; check it before shipping a change.

## 1. Why tiger exists

AI agents answer questions about a library far better when its source is on local disk. Web search and fetches are slow, miss things, and only see what is indexed. A local copy can be searched, read, and cross-referenced like any other file in the project.

Keeping those copies fresh is the hard part. General-purpose vendoring tools re-download every repo on every sync. With vendir, a sync of about 20 repos took 90s cold and about 45s with its cache on, because it re-clones and re-bundles every repo whether or not anything changed. And every project keeps its own copies: a repo used by ten projects is ten downloads, ten copies on disk, and ten separate refreshes. A sync in one project leaves the other nine stale.

tiger does exactly this one job:

```text
$ tiger sync --everywhere
~/code/editor
  ~ changed    tiptap    main 88a5dd3 → 1c9e0b2
~/code/docs-site
  ~ changed    tiptap    main 88a5dd3 → 1c9e0b2
synced 2 · unchanged 59 · failed 0 · 12 projects · 4.1s
```

- **It only fetches what changed.** One `git ls-remote` per repo decides; an unchanged repo costs a round trip, nothing more. tiptap changed once, so it was fetched once and copied into both projects.
- **One download per machine.** A shared store holds each repo once, however many projects read it.
- **Real files at almost no disk cost.** Each project gets real directories, cloned copy-on-write from the store.
- **One refresh for every project.** `tiger sync --everywhere`, by hand or on a schedule, updates every project that uses a changed repo.
- **Agents can drive it.** Every command is non-interactive, re-runnable, and speaks JSON.

## 2. The model

Everything in tiger is one of four things:

```text
  project config            store (one per machine)          refs folder (one per project)
  ──────────────            ───────────────────────          ─────────────────────────────
  tiger.toml           ──▶  ~/.tiger                    ──▶  tiger/
  committed intent:         disposable cache:                disposable output, gitignored:
  one entry per ref,          repos/  shallow bare clones      <id>/   real files, read-only
  notes as comments           trees/  one tree per commit      TIGER.md  the pointer for agents
                              registry.json  projects          .tiger-state.json
```

- A **ref** is one entry in `tiger.toml`: a git URL, optionally a branch, tag, or commit, and options. Its key is its **id**, which is also its folder name.
- The **store** holds each repo once, as a shallow bare clone, plus one extracted **tree** per commit in use.
- The **refs folder** holds one **copy** per ref, cloned from the store's tree. It also holds tiger's state and `TIGER.md`, and anything else you put there.
- **Sync** makes copies match the config, fetching only what changed upstream.

A ref follows the remote's default branch unless it says otherwise:

```toml
[refs]
# Headless editor framework built on ProseMirror. Worth reading: the
# extension API, node views, and how collaboration is wired in.
tiptap = "https://github.com/ueberdosis/tiptap"

# Pinned: we target 3.4 until we migrate.
zod = { url = "https://github.com/colinhacks/zod", ref = "v3.4.2" }
```

## 3. Principles

Every change to tiger answers to these. None outranks the guiding question; together they make it operational.

1. **The project config is the source of truth.** `tiger.toml` is committed, records intent, and carries the notes about each ref as plain comments. There is no separate index to drift: the commented config is the index.

2. **The config is data, never code.** TOML, not a script. Syncing another project's refs never runs that project's code, and tiger can edit the file without losing a comment.

3. **The store is a disposable cache.** Deleting `~/.tiger` loses nothing: every project rebuilds from its config. A fresh clone or a cloud agent with no store just runs `tiger sync`.

4. **tiger owns only what it creates.** In the refs folder, tiger owns the ref folders (`<id>/`), release notes (`<id>.releases.md`), `TIGER.md`, and names starting with `.tiger`. Everything else there — a screenshot, a PDF, an archived snapshot — is yours, and no sync touches it.

5. **Failures never delete anything.** Network down, repo moved, auth failed: the existing copy stays, the ref is reported as failed, and every other ref still syncs.

6. **Copies are real directories, never symlinks.** Search tools skip symlinked directories when they walk a folder, and agent sandboxes block paths that resolve outside the project. A copy-on-write clone gives real files inside the project at almost no disk cost.

7. **Readers never see half a repo.** A changed copy is built beside the old one and swapped in with one atomic rename.

8. **Agents are the main users.** Every command is non-interactive, safe to re-run, and has JSON output. Errors say what happened, where, and the fix. Copies are read-only, so an accidental edit fails loudly instead of vanishing on the next sync.

9. **Pay only for what changed.** A sync of unchanged refs is one `ls-remote` per repo, in parallel; its time is network round trips, not repo size. Pinned refs that are already synced cost nothing at all.

## 4. How it works

### Sync

1. Load and validate `tiger.toml`.
2. Resolve each ref's upstream commit with one `git ls-remote` per repo, 32 at a time. A ref pinned to a tag or commit that is already synced needs no network. A remote that stalls is retried once and then reported as failed, so one slow host never holds up the rest.
3. Skip a ref whose commit, URL, and filters match the state file and whose folder exists.
4. For a new or changed ref, under the repo's lock in the store: fetch that one commit (`--depth 1`) into the shallow bare clone, and extract its files into `trees/…/<sha>/`, read-only, unless that tree already exists.
5. Clone the tree into a temporary folder beside the copy (`clonefile` on APFS, `FICLONE` on Btrfs and XFS, a plain copy elsewhere), apply `include` and `exclude`, and atomically swap it in.
6. Record the result in `.tiger-state.json` and register the project in the store's registry.
7. Remove the folders of refs that were deleted from the config — only folders the state file says tiger created.

### Disk cost

About one compressed shallow clone plus one tree per commit in use, however many projects read it. Copies share the tree's blocks until something writes to them, which nothing does. Cloning a 50,000-file tree takes about 0.6s on APFS. On a filesystem without copy-on-write, or when the project is on a different volume than the store, each copy is a full copy; tiger says so when it happens and `tiger status` shows each ref's copy mode.

### Every project at once

The store's registry records which projects use tiger. `tiger sync --everywhere` loads every registered project's config, checks each repo once, fetches each changed commit once, and updates every project's copy. A project, or one of its refs, can opt out with `propagate = false`; it then updates only when its own `tiger sync` runs. Projects whose folder no longer exists are pruned from the registry.

`tiger config set schedule 6h` installs a background job (launchd on macOS, a systemd user timer on Linux) that runs `tiger sync --everywhere`. A copy is swapped in atomically, so an agent reading mid-sync never sees half a repo.

### Sandboxes

Copies live inside the project, so an agent reads them without any permission to read outside the repo. Writing is another matter: an agent sandbox that forbids writing outside the project also forbids writing to `~/.tiger`. When the store isn't writable, tiger falls back to a store inside the refs folder (`.tiger-store/`) and says so. Nothing is shared in that mode, but everything works. To share the store with sandboxed agents, allow writes to `~/.tiger`; cross-project refreshes are best left to your own terminal or the scheduled job.

### Versions, pins, history, releases

- **A ref without `ref` follows the remote's default branch**, whatever it is named.
- **A branch follows upstream; a tag or a 40-character SHA is a pin.** A name that is both a branch and a tag is an error, never a guess: write `refs/heads/<name>` or `refs/tags/<name>`.
- **Different versions of one repo** point at different trees in the store, built from the same objects.
- **`history = true`** makes the copy the project's own clone with `--filter=blob:none`: full commit history, file contents fetched on first look. `git log -S` and `git diff v3..v4` work in place. These clones are not shared through the store, because sharing objects would make git read outside the project.
- **`releases = true`** writes `<id>.releases.md` from the GitHub Releases API, newest first, only when the repo's tag list changed. GitHub only, for now.

### Private repos

Cloning goes through git's own auth: SSH keys, a credential helper, or a URL rewrite, all from the global git config (a project's local git config never applies, so a ref behaves the same everywhere). tiger never stores or handles credentials for cloning, and it never prompts: a ref that needs a password fails with git's message, and the rest sync. A collaborator or cloud agent without access sees that ref fail while everything else syncs. `tiger init` gitignores the refs folder so private code stays out of commits; whether to keep it ignored is each project's call.

## 5. Scope

tiger **is** a way to keep read-only copies of git repos current inside projects, for reading.

tiger **is not**:

| Not this | Use instead |
| --- | --- |
| A dependency manager or build step | Your package manager |
| A way to patch or modify upstream code | A fork, or a vendored copy you own |
| A fetcher for tarballs, Helm charts, or images | vendir, or a script |
| Code your project imports at runtime | Your package manager; copies are for reading |
| Git-tracked snapshots | Archive a copy into a tracked folder of your own |

## 6. Decisions

| Question | Decision | Why |
| --- | --- | --- |
| Language | Rust | A 3 MB binary per platform with no runtime, fast startup, and `toml_edit`, the best comment-preserving TOML editor available |
| Config format | TOML (`tiger.toml`) | Comments are first-class, agents write it well, and it is data, so no code runs on sync |
| Global settings | `~/.config/tiger/config.toml`, edited with `tiger config` | Settings are kept; the store is disposable, so they live apart |
| Default refs folder | `tiger/` | Short and self-describing with `TIGER.md` inside; `dir` changes it |
| Branch/tag clash | An error that names both full refnames | Branch means follow, tag means pin; a silent guess changes behavior |
| No copy-on-write | Full copies, a warning when it happens, the mode in `tiger status` | Correct everywhere, and never silently expensive |
| Runtime requirement | `git` on `PATH` | Nothing else |

Decisions made for now (distribution, lock files, platforms, defaults) live in [status](./status.md), each with what would reopen it.

## 7. Road to 1.0

tiger stays below 1.0 while the model is proven by real use across many projects. 1.0 is due when:

1. The config format and JSON output have gone a full release with no breaking change.
2. Every command's contract is documented in the [reference](./reference/cli.md) and covered by an end-to-end test against real git repos.
3. Binaries for macOS and Linux (arm64 and x64) ship from release CI, and Homebrew installs them.

Until then, breaking changes are allowed when they serve the guiding question, and each ships with a migration note an agent can apply mechanically.
