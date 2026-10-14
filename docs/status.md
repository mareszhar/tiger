# tiger — status

Decisions that are right for now but not forever. Each says what was decided, why it holds today, and the trigger that reopens it. When a trigger fires, the decision becomes an initiative ([workflow](./maintainers/workflow.md)); when one is settled for good, it moves to [vision §6](./vision.md#6-decisions).

| Decision | Why, for now | Reopen when |
| --- | --- | --- |
| **Ships through Homebrew (`mareszhar/tap`) and GitHub Releases only.** No npm package. | Homebrew covers the maintainer's machines; a second channel adds release machinery before the binaries have proven themselves | Someone needs tiger where Homebrew isn't available, typically a cloud agent in a Linux container with Node. Then: `@mszr/tiger` with one package per platform, as esbuild does, so `npx @mszr/tiger sync` works |
| **A tap, not homebrew-core.** | homebrew-core accepts tools with notable, sustained use | tiger meets [homebrew-core's acceptance criteria](https://docs.brew.sh/Acceptable-Formulae) |
| **macOS and Linux only.** | Copy-on-write clones and atomic swaps are built on `clonefile`/`FICLONE` and `renamex_np`/`renameat2` | Someone needs Windows: it needs ReFS block cloning, a rename strategy, and a scheduler |
| **Release notes from GitHub only.** Other hosts sync and warn. | The refs in use are on GitHub | A ref on GitLab, Codeberg, or Gitea wants `releases = true` |
| **No lock file.** | A branch means "latest"; a pin fixes a version; `tiger show` says what each copy holds and when it was checked | A project needs the same commits on every machine without pinning each ref, e.g. to reproduce an agent's answer later |
| **One refs folder per config.** Per-ref directory overrides are deferred ([exploration](./explorations/per-ref-directories.md)). | Separate configs already give separate lists of refs. Overrides need ownership by path, safe relocation, and coordination between output folders | A real project needs one config to group copies into several refs folders, and separate configs do not fit |
| **No vendir converter.** | Agents can translate the config, and additions can defer remote checks ([getting started §9](./guide/getting-started.md#9-moving-from-vendir)) | Translating vendir configs becomes the slowest part of adopting tiger |
| **Network defaults: 32 operations at once; `ls-remote` gets 15s and one retry.** | Measured on one machine and network ([architecture §5](./maintainers/architecture.md#5-measurements)) | Rate-limit errors or stalls show up on another network or host |
| **The background schedule is opt-in** (`schedule = off`). | A job that writes into every project should be a choice | Most users turn it on right after installing |
| **`CHANGELOG.md` lives in the repo,** and each version's section is its release notes. A trial: selenita keeps its changelog in release notes only. | One source for notes, readable offline and by agents, and releases need no manual step | After three releases: if entries drift from what shipped, or the file isn't read, go back to release notes only |
