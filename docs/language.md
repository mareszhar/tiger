# tiger — language

tiger's language follows its model. A word means the same thing in the CLI, the config, JSON output, error messages, docs, and code. This document is the vocabulary, the naming rules that keep it predictable, and the house style for everything tiger writes.

## 1. Vocabulary

```text
project ──has──▶ tiger.toml ──lists──▶ refs (id → url, ref, options)
   │                                      │
   └──has──▶ refs folder (tiger/)         │ sync
               ├── <id>/   copy  ◀── clone ── tree ◀── extract ── repo ◀── fetch ── remote
               ├── TIGER.md                     (store: one per machine)
               └── .tiger-state.json
```

| Term | Meaning | Not to be confused with |
| --- | --- | --- |
| **project** | A folder with a `tiger.toml`, usually a git repository's root. | The repos it copies. |
| **config** | `tiger.toml`: the committed list of refs, with notes as comments. The source of truth. | **Settings**, which are global. |
| **settings** | Machine-wide preferences in `~/.config/tiger/config.toml`, changed with `tiger config`. | The project config. |
| **ref** | One entry in the config: a repo to copy, and what to copy from it. | A git ref. When git's meaning is needed, say **branch**, **tag**, or **refname**. |
| **id** | A ref's key in the config, and its folder name. Belongs to the project: two projects may name one repo differently. | The repo's name, which is only the default id. |
| **`ref` field** | What a ref follows or pins: a branch, a tag, or a commit. Absent means the default branch. | — |
| **default branch** | The branch the remote's `HEAD` points at, whatever it is named. | `main`. |
| **follow** | Track a branch: each sync moves the copy to the branch's latest commit. | — |
| **pin** | Fix a ref to a tag or commit. A pinned ref that is synced needs no network. | A lock file; tiger has none. |
| **refs folder** | The project folder holding copies, `tiger/` by default (`dir`). Gitignored by `tiger init`. | The store. |
| **copy** | A ref's folder in the refs folder: real, read-only files. | A symlink, which tiger never makes. |
| **clone** | A copy-on-write copy that shares the store's disk blocks. Also, for `history = true`, the project's own git clone. | A full copy. |
| **store** | The machine's cache of repos and trees, `~/.tiger` by default. Disposable. | The refs folder. |
| **repo** (in the store) | One shallow bare clone per URL, under `repos/`. | — |
| **tree** | The extracted files of one commit, read-only, under `trees/`. Copies are cloned from trees. | A git tree object. |
| **state** | `.tiger-state.json`: what tiger last put in the refs folder. How tiger knows what it owns. | The config. |
| **registry** | The store's list of projects, so `--everywhere` and `gc` can find them. | — |
| **sync** | Make copies match the config, fetching only what changed. | Pull, update, refresh. |
| **propagate** | Let `sync --everywhere` from another project update this one. | — |
| **schedule** | The background job that runs `sync --everywhere` at an interval. | — |
| **release notes** | `<id>.releases.md`, from GitHub Releases. | A changelog in the repo. |
| **pointer** | `TIGER.md` in the refs folder: orients an agent that lands there. | The usage header in `tiger.toml`. |

Sync reports one **status** per ref: `new`, `changed`, `unchanged`, `failed`, or `removed`. Status reports one **verdict** per ref: `up to date`, `behind`, `pinned`, `not synced`, `config changed`, `removed from config`, or `unknown`.

## 2. Naming

> **A name says what kind of thing it is, and means one thing everywhere.**

- **Commands are verbs** the user would say: `add`, `rm`, `sync`, `pin`, `unpin`, `status`, `gc`, `config`, `init`. `rm` matches git and the shell; it is the only abbreviation.
- **Flags say what they change**: `--everywhere`, `--offline`, `--force`. A flag that turns something off starts with `--no-`: `--no-sync`, `--no-gitignore`, `--no-propagate`.
- **Config fields are nouns or yes-or-no facts** without prefixes: `url`, `ref`, `include`, `history`, `releases`, `propagate`. A boolean is `true` when the feature is on.
- **JSON keys are `snake_case`** and use the vocabulary above: `status`, `verdict`, `copy_mode`, `removed_trees`. Enum values are lowercase words joined by `-`: `up-to-date`, `config-changed`.
- **In code**, functions start with a verb (`ensure_tree`, `build_copy`, `resolve_all`), modules are the noun they own (`store`, `remote`, `state`), and `RefEntry` (config) never mixes with `RefState` (what was synced).
- **One spelling per concept.** No aliases. A renamed concept leaves no trace; the changelog owns migration.

## 3. Error messages

An error message is part of the interface, read by people and by agents acting on it. Every error has three parts: **what happened**, **the context that locates it**, and **the fix**.

```text
tiger: invalid tiger.toml (2 problems)
  dir: ".refs" must not be hidden: search tools skip folders starting with '.'
  refs.big.inlcude: unknown key 'inlcude'; did you mean 'include'? (valid: url, ref, include, exclude, history, releases, propagate)
  in /work/app/tiger.toml
  hint: fix the entries above; `tiger --help` describes every field
```

```text
tiger: 'v1' is a branch, and a pin is a tag or commit
  hint: to follow that branch, set ref = "v1" for lib in tiger.toml; to pin its current commit, run `tiger pin lib 4f2a91c…`
```

- Start with `tiger:` and a lowercase sentence fragment; no trailing period.
- Name the ref, field, file, or command involved. List the valid alternatives when there are few.
- Report every problem found, not just the first.
- End with the most likely fix as a `hint:` line when there is one. A fix is a command or an edit, written so it can be run or applied as is.
- A ref's failure in a sync is one line: git's own reason, plus what tiger did about it (`kept existing copy`).
- Never apologize, never blame, never guess beyond the evidence.

## 4. Writing style

For every document, help text, header, and comment tiger ships:

- **Lead with intent.** Say what a thing is for before how it works.
- **Concrete by default.** Pair each contract with the smallest command, config snippet, or output that shows it.
- **Decisions, not deliberations.** State what tiger does. Rationale earns a sentence when it prevents a known mistake.
- **Current, not historical.** Describe how tiger works now; the changelog narrates change.
- **One home per fact.** Link to the owner instead of restating. The [reference](./reference/cli.md) owns exact behavior; guides own how-to; this document owns words.
- **Precise, not padded.** No hedging, no filler, no "simply".
- **Renderer-owned layout.** One paragraph per line in Markdown; Markdown wraps.
