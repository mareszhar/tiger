# tiger — workflow

How changes to tiger are planned, carried out, reviewed, and released, so any person or agent can pick up work mid-flight and know what is true, what is planned, and what is left.

## 1. Two speeds

| Change | How it moves |
| --- | --- |
| **Small**: a bug fix, a doc correction, one flag | Change, test, doc, and changelog entry together. |
| **Initiative**: multi-step work, work across sessions or people, a release | Planned in a handoff, executed against a tracker, reviewed in round reports, announced in the changelog. |

It is an initiative if you would want to resume it tomorrow, or hand it to someone else, without re-reading the whole diff.

## 2. Records

An initiative is named after the version it ships in, without dots: `v010` for 0.1.0. Its working records live in `__temp__/`, which is gitignored; the changelog and status are tracked.

| Record | Answers | File |
| --- | --- | --- |
| **handoff** | What does this release change, and how do we know it's done? | `__temp__/v010-handoff.md` |
| **tracker** | Where is execution, what was decided, what was measured, what's next? | `__temp__/v010-tracker.md` |
| **round report** | How good is this pass, and what must change before the next? | `__temp__/v010-round-<n>.md` |
| **changelog** | What changed for someone using tiger, and how do they migrate? | [`CHANGELOG.md`](../../CHANGELOG.md), under `## [Unreleased]` until released |
| **status** | Which decisions are for now, and what would reopen them? | [`docs/status.md`](../status.md) |
| **exploration** | What did we consider, and what would we need to settle if it comes back? | [`docs/explorations/<topic>.md`](../explorations/) |

- The **handoff** is written against the last release and never records progress. When execution reveals something new, the handoff gains a plan for it, written as if nobody had started.
- The **tracker** is the implementer's memory: decisions, probes with numbers, gate results, and one row per requirement answered by a test name or `file:line`.
- **Round reports** are never rewritten; their sequence is the initiative's history.
- The **changelog** follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/): every user-visible change lands under `## [Unreleased]` in the same change that makes it, grouped as Added, Changed, Deprecated, Removed, Fixed, or Security. It speaks the user's vocabulary, with before/after for every breaking change so an agent can migrate a config mechanically. It is the one tracked document that narrates history; the release script stamps it, and each version's section becomes that GitHub release's notes.
- **Status** lists decisions made for now, each with the trigger that reopens it. When a trigger fires, the decision becomes an initiative; when it's settled for good, it moves to [vision §6](../vision.md#6-decisions).

Plans describe what isn't true yet, so they stay out of tracked docs. A fact worth keeping moves into a canonical doc, rewritten in the present tense. Deleting records is the maintainer's call.

An **exploration** is the exception: it records an idea we considered but didn't build, so it is tracked and opens with "Not built", linked from its row in [status](../status.md). Status keeps the decision and what would reopen it; the exploration keeps the examples and open questions for whoever picks it up.

## 3. Standing practice

- **Prove host behavior before building on it.** A design that rests on what git, a filesystem, or a service manager does opens with a probe, and its result goes in the tracker.
- **Measure, then claim.** Speed and size claims in docs come from a recorded measurement ([architecture §5](./architecture.md#5-measurements)).
- **Every promise has a test that fails when it breaks** ([evidence](./evidence.md)).
- **Disclose weak evidence.** Separate what was run from what was only read.
- **Challenge the plan.** An implementer who believes a step is wrong says so before building it.
- **Decide for now, out loud.** A decision that is right today but not forever goes in [status](../status.md) with the trigger that reopens it, so nobody re-argues it early or forgets it late.

## 4. Working a handoff

1. Read the handoff, then the docs it points to. Don't start from the code.
2. Open the tracker; record the baseline (commit, toolchain, gate results).
3. Work slice by slice: probe, failing test, change, docs.
4. Update the tracker after every meaningful step.
5. End a pass with the full gate and the per-requirement table, then ask for review.

## 5. Releasing

A version numbers the contract: below 1.0, breaking changes bump the minor, everything else the patch. Releases are the maintainer's; an agent prepares one and runs the release script only when asked.

1. The gates are green on a clean `main`, CI passed on macOS and Linux, and `## [Unreleased]` in `CHANGELOG.md` says everything this release changes.
2. Run the release script in a terminal:

   ```sh
   scripts/release.sh patch        # or minor, major, or an exact version: scripts/release.sh 0.1.0
   ```

   It requires a clean tree, a branch with an upstream, and a non-empty `## [Unreleased]`; rejects an existing tag; bumps `version` in `Cargo.toml`; stamps the changelog (`## [Unreleased]` becomes `## [<version>] - <date>`, with a fresh empty `Unreleased` above it and compare links below); runs the gates (which also update `Cargo.lock`); commits `🔖 release v<version>`; creates the annotated tag `v<version>` with message `tiger v<version>`; and runs `git push --atomic --follow-tags`. If anything fails before the push, it restores the version files and the changelog and removes the tag. If the push itself fails, the commit and tag stay local; fix the cause and rerun the push.
3. The tag starts the release workflow ([Actions](https://github.com/mareszhar/tiger/actions) shows it). It checks the tag matches `Cargo.toml` and the changelog has notes for it, tests and builds four binaries (macOS and Linux, arm64 and x64), publishes the GitHub release with the version's changelog section as its notes (`scripts/release-notes.sh <version>` prints them), and pushes the formula to `mareszhar/homebrew-tap`.
4. Check the install a user gets once the workflow is green: `brew update && brew upgrade tiger` (or `brew install mareszhar/tap/tiger`), then `tiger --version`. `brew upgrade` alone may not see the new formula: Homebrew refreshes taps automatically at most once a day.

**One-time setup for the tap:** create the public repository `mareszhar/homebrew-tap` with a README, so it has a `main` branch (Homebrew finds `<owner>/tap` at `<owner>/homebrew-tap`; one tap holds every formula you publish, in `Formula/`). Create a fine-grained token with *Contents: read and write* on `homebrew-tap` only, and add it to this repository in the browser: *Settings → Secrets and variables → Actions → New repository secret*, named `HOMEBREW_TAP_TOKEN`. Without it, the GitHub release still publishes and only the tap step fails.

Releasing needs nothing but git: the push goes over SSH like any other, and everything that needs GitHub credentials runs in Actions. Don't sign in the GitHub CLI for releases; its token could push without the approval an SSH key in the Secure Enclave asks for.
