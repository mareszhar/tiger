# tiger — evidence

How tiger proves its promises. A green suite should mean the promises hold, not that code ran.

## 1. Two kinds of tests

| Kind | Lives in | Proves |
| --- | --- | --- |
| **Unit** | `#[cfg(test)]` in each module | Pure logic: parsing, resolution, URL keys, filters, config edits, rendering |
| **End-to-end** | `tests/cli.rs` | A promise as a user or agent sees it: the real binary, real git, real files |

Every user-visible promise has an end-to-end test named after it, in the user's words: `failures_keep_the_existing_copy_and_the_rest_still_sync`, `tiger_only_touches_what_it_created`.

## 2. What a good end-to-end test contains

- **Real repositories.** Upstreams are local git repositories reached through `file://` URLs, so the suite runs offline and fast. Their default branch is `trunk`, so nothing can pass by assuming `main`.
- **A hermetic machine.** `Env` gives each test its own `HOME`, store (`TIGER_HOME`), settings (`XDG_CONFIG_HOME`), and job folder (`TIGER_SCHEDULER_DIR`), and runs git with `GIT_CONFIG_GLOBAL=/dev/null`. No test reads or writes the developer's real configuration, store, or service manager.
- **A positive control beside every negative one.** "The old copy stays" is checked next to "the other ref synced"; "propagate = false is left alone" next to "the other project was updated".
- **Exit codes and JSON, not just text.** Assert the exit code, then the JSON fields an agent would read. Text output is asserted only where its wording is the contract.
- **The files themselves.** Read the copy, the config, the state file, the store. A report saying "changed" is not evidence that the file changed.

## 3. Gates

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs all three on macOS and Linux (`.github/workflows/ci.yml`).

## 4. Proving a change is load-bearing

A new test must fail without the change it protects. Revert the change (or break it on purpose), watch the test fail for the right reason, restore it. Record the failing output in the tracker.

## 5. What the suite can't see

State these plainly in a tracker rather than implying coverage:

- **Network behavior against real hosts** (GitHub's latency, stalls, auth): probe by hand, record numbers in [architecture §5](./architecture.md#5-measurements).
- **The GitHub Releases API:** covered by rendering tests; the request path needs a live run.
- **Loading the background job** into launchd or systemd: the suite checks the job files only.
- **Linux-only paths** (`FICLONE`, `renameat2`, systemd): exercised by CI on Linux, not on a Mac.
