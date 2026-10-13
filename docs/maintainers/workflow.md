# tiger — workflow

How changes to tiger are planned, carried out, reviewed, and released, so any person or agent can pick up work mid-flight and know what is true, what is planned, and what is left.

## 1. Two speeds

| Change | How it moves |
| --- | --- |
| **Small**: a bug fix, a doc correction, one flag | Change, test, and doc together. A breaking change gets a changelog entry with its migration. |
| **Initiative**: multi-step work, work across sessions or people, a release | Planned in a handoff, executed against a tracker, reviewed in round reports, announced in a changelog. |

It is an initiative if you would want to resume it tomorrow, or hand it to someone else, without re-reading the whole diff.

## 2. Records

An initiative is named after the version it ships in, without dots: `v010` for 0.1.0. Records live in `__temp__/`, which is gitignored.

| Record | Answers | File |
| --- | --- | --- |
| **handoff** | What does this release change, and how do we know it's done? | `__temp__/v010-handoff.md` |
| **tracker** | Where is execution, what was decided, what was measured, what's next? | `__temp__/v010-tracker.md` |
| **round report** | How good is this pass, and what must change before the next? | `__temp__/v010-round-<n>.md` |
| **changelog** | What changed for someone using tiger, and how do they migrate? | `__temp__/v010-changelog.md`, then the release tag's message |

- The **handoff** is written against the last release and never records progress. When execution reveals something new, the handoff gains a plan for it, written as if nobody had started.
- The **tracker** is the implementer's memory: decisions, probes with numbers, gate results, and one row per requirement answered by a test name or `file:line`.
- **Round reports** are never rewritten; their sequence is the initiative's history.
- The **changelog** speaks the user's vocabulary, with before/after for every breaking change so an agent can migrate a config mechanically.

Plans describe what isn't true yet, so they stay out of tracked docs. A fact worth keeping moves into a canonical doc, rewritten in the present tense. Deleting records is the maintainer's call.

## 3. Standing practice

- **Prove host behavior before building on it.** A design that rests on what git, a filesystem, or a service manager does opens with a probe, and its result goes in the tracker.
- **Measure, then claim.** Speed and size claims in docs come from a recorded measurement ([architecture §5](./architecture.md#5-measurements)).
- **Every promise has a test that fails when it breaks** ([evidence](./evidence.md)).
- **Disclose weak evidence.** Separate what was run from what was only read.
- **Challenge the plan.** An implementer who believes a step is wrong says so before building it.

## 4. Working a handoff

1. Read the handoff, then the docs it points to. Don't start from the code.
2. Open the tracker; record the baseline (commit, toolchain, gate results).
3. Work slice by slice: probe, failing test, change, docs.
4. Update the tracker after every meaningful step.
5. End a pass with the full gate and the per-requirement table, then ask for review.

## 5. Releasing

A version numbers the contract: below 1.0, breaking changes bump the minor, everything else the patch. Releases are the maintainer's; an agent prepares one and runs the release script only when asked.

1. The gates are green on a clean `main`, CI passed on macOS and Linux, and `__temp__/v<version>-changelog.md` is final.
2. Run the release script in a terminal:

   ```sh
   scripts/release.sh patch        # or minor, major, or an exact version: scripts/release.sh 0.1.0
   ```

   It requires a clean tree and a branch with an upstream, rejects an existing tag, bumps `version` in `Cargo.toml`, runs the gates (which also update `Cargo.lock`), commits `🔖 release v<version>` when the version changed, creates the annotated tag `v<version>` with message `tiger v<version>`, and runs `git push --atomic --follow-tags`. If anything fails before the push, it restores the version files and removes the tag. If the push itself fails, the commit and tag stay local; fix the cause and rerun the push.
3. The tag starts the release workflow (`gh run watch` follows it). It checks the tag matches `Cargo.toml`, tests and builds four binaries (macOS and Linux, arm64 and x64), publishes the GitHub release, and pushes the formula to `mareszhar/homebrew-tap`.
4. Replace the generated release notes with the changelog. The GitHub release is the changelog's permanent home; the repository has no `CHANGELOG.md`.

   ```sh
   gh release edit v0.1.0 --notes-file __temp__/v010-changelog.md
   ```

5. Check the install a user gets: `brew upgrade tiger` (or `brew install mareszhar/tap/tiger`), then `tiger --version`.

**One-time setup for the tap:** create the public repository `mareszhar/homebrew-tap` (Homebrew finds `<owner>/tap` at `<owner>/homebrew-tap`), then add a `HOMEBREW_TAP_TOKEN` secret to this repository: a fine-grained token with *Contents: read and write* on `homebrew-tap` only. Without it, the GitHub release still publishes and only the tap step fails.
