# Getting started

From nothing to agents reading real source in a few minutes, then one refresh for every project on the machine.

## 1. Install

```sh
brew install mareszhar/tap/tiger
```

Or download a binary from [GitHub Releases](https://github.com/mareszhar/tiger/releases) (macOS and Linux, arm64 and x64) and put it on your `PATH`. Or build it: `cargo install --git https://github.com/mareszhar/tiger`. The only runtime requirement is `git`.

## 2. Add your first refs

In your project's root:

```sh
tiger init
tiger add ueberdosis/tiptap --note "Editor framework. Read the extension API and node views."
tiger add colinhacks/zod --ref v3.22.4 --note "Pinned: we target 3.22 until we migrate."
```

You now have:

```text
tiger.toml           the refs, with your notes as comments (commit it)
tiger/               gitignored
  TIGER.md           tells an agent what this folder is
  tiptap/            the repo's default branch, read-only
  zod/               tag v3.22.4, read-only
```

`tiger.toml` reads like this:

```toml
[refs]

# Editor framework. Read the extension API and node views.
tiptap = "https://github.com/ueberdosis/tiptap"

# Pinned: we target 3.22 until we migrate.
zod = { url = "https://github.com/colinhacks/zod", ref = "v3.22.4" }
```

A ref with only a URL follows whatever the remote's default branch is (`main`, `master`, `trunk`). You never have to name it.

## 3. Tell your agents

`tiger init` prints a line for your agent instructions (`AGENTS.md`, `CLAUDE.md`, …):

> Read-only copies of repos we depend on live in `tiger/`, managed by tiger and described in `tiger.toml`. Read the source there before guessing; search one with `rg "pattern" tiger/<id>`. Refresh with `tiger sync`.

The folder is gitignored, so project-wide searches skip it and stay focused on your code; searching a ref means passing its path. An agent that opens `tiger.toml` finds a usage header, and one that lands in `tiger/` finds `TIGER.md`. Agents can run every command themselves: "add X as a ref", "pin zod to v3.23.0", "refresh the refs".

## 4. Keep them fresh

```sh
tiger status          # which refs are behind
tiger sync            # fetch only what changed
```

A sync checks every repo with one `git ls-remote` in parallel and downloads only commits it doesn't have. Pinned refs that are synced cost nothing.

## 5. Every project at once

Every project that syncs is registered in the shared store (`~/.tiger`), and a repo is downloaded once per machine however many projects use it. To update every project that uses a changed repo:

```sh
tiger sync --everywhere
```

To do that in the background, every 6 hours:

```sh
tiger config set schedule 6h
```

`tiger config` shows every global setting and whether the job is installed; `tiger config set schedule off` removes it. A project that wants to update only on its own schedule sets `propagate = false` in its `tiger.toml` (or on one ref).

## 6. Your own files in the refs folder

tiger owns the ref folders, release notes, `TIGER.md`, and names starting with `.tiger`. Anything else you put in `tiger/` (a screenshot, a PDF, an archived snapshot) stays where it is: no sync, `rm`, or `gc` touches it. If one of your folders has the name of a new ref, that ref fails instead of replacing it.

## 7. Agent sandboxes

Copies are real files inside the project, so an agent reads them with no permission to read outside the repo. Syncing writes to the shared store, though, and a sandbox that forbids writes outside the project forbids that too. In that case tiger keeps a private store in `tiger/.tiger-store/` and says so: everything works, but nothing is shared.

Choose one:

- **Refresh outside the sandbox.** Run `tiger sync --everywhere` in your terminal, or set a schedule, and let agents just read.
- **Allow writes to the store.** Add `~/.tiger` to your sandbox's writable paths; agents' syncs then share the store too.

## 8. Private repos

tiger never handles credentials: git does, with your **global** git config. A project's local config (`git config --local`) doesn't apply, so a ref resolves the same from every project and from the scheduled job.

With HTTPS and a read-only token scoped to one repo, give git a rewrite for that repo only. Git uses the longest matching rewrite, so it wins over broader ones:

```sh
git config --global url."https://<token>@github.com/me/private-lib".insteadOf "https://github.com/me/private-lib"
tiger add https://github.com/me/private-lib
```

`tiger.toml` keeps the plain URL, so it's safe to commit; the token stays in your git config. A credential helper works the same way (with `credential.useHttpPath = true` for per-repo tokens). SSH URLs work too, but every fetch then uses your SSH key; if approving it takes a fingerprint, the scheduled job would ask each time, so prefer HTTPS for refs.

Someone without access sees that ref fail with git's message while the rest sync.

## 9. Moving from vendir

For each vendir directory entry, add a ref; then delete the vendir copy so tiger can own the folder.

```yaml
# vendir.yml
directories:
  - path: __references__
    contents:
      - path: selenita
        git: { url: https://github.com/mareszhar/selenita, ref: origin/main }
```

```sh
tiger init --dir __references__
tiger add mareszhar/selenita --no-sync     # origin/main is the default branch: no --ref needed
rm -rf __references__/selenita              # vendir's copy; tiger won't overwrite a folder it didn't create
tiger sync
rm vendir.yml vendir.lock.yml
```

vendir's `ref: origin/<branch>` becomes `--ref <branch>` (or nothing, for the default branch), a tag stays a tag, and `includePaths`/`excludePaths` become `include`/`exclude`. vendir's lock file has no equivalent: pin the refs that need exact versions.

## What's next

- [CLI reference](../reference/cli.md): every command, flag, status, and the JSON output
- [Configuration reference](../reference/config.md): every field, global settings, the store
- [Vision](../vision.md): why tiger works the way it does
