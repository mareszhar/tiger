# Per-ref directories

> **Not built.** Deferred; see [status](../status.md) for what would reopen it. This page records an idea we explored, so a future planner can start from it. It promises nothing.

Today each config writes all its copies into one refs folder. A per-ref `dir` would let one config place some copies elsewhere, for example next to the product that uses them, while keeping a single list of refs.

## What it could look like

```toml
dir = "refs"

[refs]
shared = "https://github.com/acme/shared"
tnt = { url = "https://github.com/acme/tnt", dir = "product-tnt/refs" }
```

A ref's `dir` would override the project's, and both would be relative to the folder with `tiger.toml`. The id still names the copy, so `tnt` would land in `product-tnt/refs/tnt/`.

This is different from several configs. Separate configs give separate lists of refs, so an agent working on one product sees only that product's refs. One config with overrides gives one list, with copies in several places. Placement wouldn't hide any entry from an agent reading the config.

## Why it isn't a small change

A branch override changes what goes into a copy's folder. A `dir` override changes where tiger is allowed to write and delete, and that touches its safety promises:

- **Ownership needs paths, not just ids.** State would have to record where each copy actually lives, so editing or removing an entry can never point tiger at the wrong folder.
- **Changing a ref's `dir` moves a copy.** The old copy has to stay until the new one and its state are ready; tiger must refuse destinations it doesn't own and recover if interrupted.
- **Destinations must be checked together,** across configs too: no copy inside another copy or on top of tiger's own files, and nothing that leaves the project through a symlink.
- **Locks must cover every destination,** so two projects never write the same copy or pointer.
- **Each refs folder needs its own `TIGER.md`,** release notes need a home, and old pointers must go without touching user files.
- **Ignore rules stay the user's choice.** Placement must not make sync edit tracked ignore files.

One option worth exploring: keep a single state file, project lock, and fallback store in the default refs folder, with each state entry recording its copy's path. That keeps one config working as one project.

Remote resolution, shared trees, pins, filters, and read-only copies would keep their meaning. Each copy would still be built beside its own destination and swapped in atomically; moving several copies wouldn't need to happen as one transaction, as long as each copy and its ownership stay trustworthy.
