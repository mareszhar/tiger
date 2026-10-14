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

Copies are disposable, so deleting or moving folders by hand never breaks tiger. The worst that can happen is a rebuild, or a ref that refuses to overwrite a folder it didn't make.

| You do this | tiger does this |
| --- | --- |
| Delete a copy, keeping its entry | The next sync of that ref brings it back |
| Move a copy somewhere else | It's an ordinary folder there, which tiger won't touch; the next sync rebuilds the original |
| Delete the whole refs folder, keeping `tiger.toml` | The next sync rebuilds every copy |
| Remove an entry from `tiger.toml` | The next full `tiger sync` deletes its copy |
| Run `tiger rm <id>` | Removes the entry and deletes its copy right away |
| Delete `tiger.toml` | The project is forgotten by the next `sync --everywhere` or `gc`; its copies stay until you delete them |
| Delete `.tiger-state.json` | tiger no longer knows it made the copies, so it won't overwrite or delete them |
| Run `tiger gc` | Frees store space that no remaining project uses |

So deleting a folder that holds a `tiger.toml` is how you retire a project: nothing brings it back. `gc` cleans the shared store only; a fallback store lives in its project's refs folder and goes with it.

## 7. Agent sandboxes

Copies are real files inside the project, so an agent reads them with no permission to read outside the repo. Syncing writes to the shared store, though, and a sandbox that forbids writes outside the project forbids that too. In that case tiger keeps a private store in `tiger/.tiger-store/` and says so: everything works, but nothing is shared.

Choose one:

- **Refresh outside the sandbox.** Run `tiger sync --everywhere` in your terminal, or set a schedule, and let agents just read.
- **Allow writes to the store.** Add `~/.tiger` to your sandbox's writable paths; agents' syncs then share the store too.

When a later sync can write the shared store, tiger moves what the fallback store holds into it, without the network and without touching your copies, then cleans up the fallback store. Anything that fails to move stays put for the next sync to retry. `tiger show` says which store the last sync used and whether the project is registered for `--everywhere`.

## 8. Private repos

tiger never handles credentials: git does, with your **global** git config. A project's local config (`git config --local`) doesn't apply, so a ref resolves the same from every project and from the scheduled job.

The cleanest setup is a read-only token per private repo (a fine-grained token with *Contents: read*), kept in the macOS Keychain and handed out by git's `osxkeychain` helper only for that repo. Once per machine, tell git to tell tokens apart by repo path:

```sh
git config --global credential.https://github.com.useHttpPath true
```

Then once per private repo, from any folder, run `git credential approve`, paste these lines with your values, and finish with an empty line:

```text
protocol=https
host=github.com
path=me/private-lib
username=x-access-token
password=<token>
```

The path must match the URL as written: `me/private-lib` serves `https://github.com/me/private-lib`, the form `tiger add me/private-lib` writes; a clone of `https://github.com/me/private-lib.git` needs a second entry with `path=me/private-lib.git`. Then add the ref as usual:

```sh
tiger add me/private-lib
```

Fetches never prompt: git reads the token from the Keychain, in every project and in the scheduled job. Public repos need no token. `tiger.toml` keeps the plain URL, so it's safe to commit. Homebrew's git already uses `osxkeychain`; with another git, also run `git config --global credential.helper osxkeychain`.

A URL rewrite works too, at the cost of a token in plain text in `~/.gitconfig`: `git config --global url."https://<token>@github.com/me/private-lib".insteadOf "https://github.com/me/private-lib"`. A rewrite that puts a token into every `https://github.com/` URL stops the Keychain from being asked at all, so use one approach or the other. SSH URLs work, but every fetch then uses your SSH key; if approving it takes a fingerprint, the scheduled job would ask each time, so prefer HTTPS for refs.

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
tiger add mareszhar/selenita --as selenita --no-sync --no-validate
# origin/main is the default branch: no --ref needed
rm -rf __references__/selenita              # vendir's copy; tiger won't overwrite a folder it didn't create
tiger sync
rm vendir.yml vendir.lock.yml
```

vendir's `ref: origin/<branch>` becomes `--ref <branch>` (or nothing, for the default branch), a tag stays a tag, and `includePaths`/`excludePaths` become `include`/`exclude`. An `http` source that downloads a branch tarball (`https://github.com/<owner>/<repo>/archive/refs/heads/<branch>.tar.gz`) becomes the repo URL with that branch: no tarball needed, since tiger already fetches one commit without history or tags. vendir's lock file has no equivalent: pin the refs that need exact versions.

Use `--as` to keep a vendir folder name that differs from the repo name. An id is a single folder name, so nested vendir paths need a new home. `--no-validate` skips each remote check while you add entries, and the sync at the end checks them all at once. For a big migration, writing the entries straight into `tiger.toml` (with comments as notes) and syncing once works just as well.

## 10. Configs beside copies

A project is just a folder with a `tiger.toml`, in Git or not. To keep the config with its copies, for example to keep your refs out of a public repo entirely, use `dir = "."`:

```sh
mkdir -p __references__
tiger --cwd __references__ init --dir .
tiger --cwd __references__ add ueberdosis/tiptap
```

```text
__references__/
  tiger.toml
  TIGER.md
  tiptap/
  .tiger-state.json
```

With `dir = "."`, tiger leaves ignore files alone: it can't ignore a folder from inside it. To keep the folder out of your commits, add a rule to the repo's `.gitignore`, or, to keep even that rule to yourself, to Git's local exclude file:

```sh
printf '/__references__/\n' >> "$(git rev-parse --git-path info/exclude)"
```

Ignore rules don't apply to files Git already tracks; `git ls-files __references__` shows whether any are.

Several configs can share a repo. In a monorepo, each product can keep its own `refs/tiger.toml` with `dir = "."`. Commands use the nearest `tiger.toml` in the current folder or above it, so from the repo root you pick one with `tiger --cwd product/refs sync`. Every project still shares the store and joins `sync --everywhere`.

`tiger init` prints a line for your agent instructions but never edits `AGENTS.md` or `CLAUDE.md`. If the refs live only on your machine, point agents to their `TIGER.md` in your prompt or personal instructions.

## 11. Moving copies to another folder

Change `dir` in `tiger.toml`, run `tiger sync`, check the new copies, then delete the old folder. tiger builds the copies in the new place from the store, never overwrites a folder it didn't make, and leaves the old copies alone until you delete them.

A few things worth knowing:

- **Synced inside a sandbox?** Then the old folder's `.tiger-store/` may hold commits the shared store doesn't have yet. Sync once where the shared store is writable before deleting it; `tiger show` says when there's anything left to move.
- **Rebuilding still checks upstream.** A branch that moved is fetched, and refs with `history = true` are cloned again. A ref pinned to a tag whose repo is gone can't be rebuilt; for the common move of `__references__/tiger/` copies to beside `__references__/tiger.toml`, [moving copies beside their config](./moving-copies-beside-config.md) moves the existing copies instead.
- **With `dir = "."`,** the old copies share a folder with your config, so delete them one by one rather than the folder.

## What's next

- [CLI reference](../reference/cli.md): every command, flag, status, and the JSON output
- [Configuration reference](../reference/config.md): every field, global settings, the store
- [Vision](../vision.md): why tiger works the way it does
