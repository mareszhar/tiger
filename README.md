# tiger

> Read-only, up-to-date copies of other repos inside your project, so agents can read the source instead of guessing.

Agents answer questions about a library far better with its source on disk. Web search is slow, misses things, and only sees what's indexed; a local copy can be searched, read, and cross-referenced like your own code.

Keeping those copies fresh is the hard part. Vendoring tools re-download every repo on every sync, and every project keeps its own copies, so a repo used in ten projects is ten downloads and ten separate refreshes. tiger fixes both:

```sh
tiger add ueberdosis/tiptap --note "Editor framework. Read the extension API and node views."
tiger sync --everywhere
```

```text
~/code/editor
  ~ changed    tiptap    main 88a5dd3 → 1c9e0b2
~/code/docs-site
  ~ changed    tiptap    main 88a5dd3 → 1c9e0b2
synced 2 · unchanged 59 · failed 0 · 12 projects · 4.1s
```

## Highlights

- ⚡ **Fetches only what changed.** One `git ls-remote` per repo decides. Unchanged repos cost a round trip; synced pins cost nothing.
- 🗄️ **One download per machine.** A shared store holds each repo once, however many projects use it.
- 📁 **Real files, almost no disk.** Copies are real folders cloned copy-on-write, so search tools and sandboxed agents read them like any other file. No symlinks.
- 🔄 **One refresh for every project.** `tiger sync --everywhere`, or in the background with `tiger config set schedule 6h`.
- 🌿 **Follows the default branch** when you give just a URL, whatever the branch is named. Pin a tag or commit when you need one.
- 📝 **Notes live in the config.** Comments above each entry say what a ref is for; `tiger add --note` writes them, and every edit keeps them.
- 🤖 **Built for agents.** Non-interactive commands, JSON output, errors that say the fix, read-only copies, and a `TIGER.md` pointer in the folder.
- 🛟 **Failures never delete.** Network down, auth failed, repo moved: the existing copy stays and every other ref still syncs.
- 🧹 **Your files stay yours.** tiger only touches what it created in the refs folder.

## The config

`tiger.toml` is committed with your project. The refs folder (`tiger/` by default) is gitignored.

```toml
[refs]

# Headless editor framework built on ProseMirror. Worth reading: the
# extension API, node views, and how collaboration is wired in.
tiptap = "https://github.com/ueberdosis/tiptap"

# Pinned: we target 3.22 until we migrate.
zod = { url = "https://github.com/colinhacks/zod", ref = "v3.22.4" }

# Only the source: the repo ships hundreds of MB of assets.
[refs.big]
url = "https://github.com/org/big"
include = ["src/**", "docs/**"]

# Private repos use your own git auth (SSH keys or a credential helper).
[refs.framework]
url = "git@github.com:org/framework.git"
history = true     # a real clone: git log works in place
releases = true    # framework.releases.md from GitHub Releases
```

## Get started

```sh
brew install mareszhar/tap/tiger
tiger init
tiger add <url|owner/repo> --note "what it is and why it's here"
```

`tiger init` prints a line for your `AGENTS.md` or `CLAUDE.md`, so agents know the refs are there. The [getting started guide](./docs/guide/getting-started.md) covers the rest, including sandboxes and moving from vendir.

## Commands

```text
tiger init                      create tiger.toml, the refs folder, and its gitignore line
tiger add <url> [options]       add a ref and sync it
tiger rm <id>…                  remove refs and their copies
tiger sync [<id>…]              fetch only what changed  (--everywhere: every project)
tiger status                    what each ref tracks, and whether it's behind
tiger show [<id>…]              what each copy holds: commit, date, subject, size, other projects
tiger store                     what the shared store holds, and which projects use it
tiger pin <id> [<tag|sha>]      pin a ref  ·  tiger unpin <id>  follow the default branch again
tiger gc                        prune what no project uses from the store
tiger config [set <key> <v>]    global settings: dir, store, concurrency, schedule
```

Every command takes `--json`. Exit codes: `0` ok, `1` usage or config error, `2` some refs failed (the rest synced).

## Good to know

- **Only `git` is required.** tiger is a single 3 MB binary for macOS and Linux.
- **Your git setup applies.** Credential helpers, SSH config, and URL rewrites from your global git config work as on the command line ([private repos](./docs/guide/getting-started.md#8-private-repos)). tiger never prompts.
- **It's pre-1.0.** Breaking changes can happen; each ships with a migration note.

## Documentation

| | |
| --- | --- |
| **[Getting started](./docs/guide/getting-started.md)** | Install, first refs, agents, every project at once, sandboxes, vendir |
| **[Vision](./docs/vision.md)** · **[Language](./docs/language.md)** | Why tiger exists, how it thinks, the words it uses |
| **[CLI reference](./docs/reference/cli.md)** · **[Configuration](./docs/reference/config.md)** | Exact contracts |
| **[Changelog](./CHANGELOG.md)** · **[Status](./docs/status.md)** | What each release changed; what's deferred for now, and what would change that |

## License

[AGPL-3.0-only](./LICENSE)
