# Working on tiger

tiger keeps read-only, up-to-date copies of other repos inside a project, so agents can read the source instead of guessing. It is a Rust CLI around `git`, with a TOML config. Its guiding question for every decision: **does this help an agent answer from the source?**

## Read first

1. [`docs/vision.md`](./docs/vision.md): the model, principles, and decisions.
2. [`docs/language.md`](./docs/language.md): the vocabulary; use these words exactly.
3. For changes to `src/`: [`docs/maintainers/architecture.md`](./docs/maintainers/architecture.md) and its invariants.
4. For any initiative: [`docs/maintainers/workflow.md`](./docs/maintainers/workflow.md). The current plan, if any, is the newest `__temp__/v*-handoff.md`; its progress is the matching tracker.

## Commands

| Command | Does |
| --- | --- |
| `cargo test` | Unit and end-to-end tests (offline, hermetic) |
| `cargo clippy --all-targets -- -D warnings` | Lint |
| `cargo fmt --check` | Format check |
| `cargo build --release` | The binary, in `target/release/tiger` |
| `scripts/release.sh patch` | Release: bump, gates, commit, tag, push (maintainer only; see workflow §5) |

## References

Source of repos this project learns from lives in `__references__/`, managed by tiger itself (`tiger.toml`). Read there before guessing; search one with `rg "pattern" __references__/<id>`. Refresh with `tiger sync`.

## Rules that are easy to miss

- The reference docs are the contract. Change behavior, its doc, and its `CHANGELOG.md` entry (under `## [Unreleased]`) in the same change.
- Every promise gets an end-to-end test with a positive control ([evidence](./docs/maintainers/evidence.md)). Tests stay hermetic: never touch the real `HOME`, store, or service manager.
- Never delete what tiger didn't create; never prompt; never leave half a copy. See the invariants.
- Plans and progress live in ignored `__temp__/` records, never in tracked docs. Never delete anything in `__temp__/` or `__references__/` by hand.
- Don't commit, tag, push, or publish unless explicitly asked.
