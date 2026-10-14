# tiger documentation

Two short documents set the ground for everything else. The rest is organized by what you are trying to do.

## Start here

| Read | To learn |
| --- | --- |
| [Vision](./vision.md) | why tiger exists, the model it is built on, and the principles every change answers to |
| [Language](./language.md) | the vocabulary, naming rules, and writing style: one word per idea, everywhere |

Want refs in your project right now? [Getting started](./guide/getting-started.md) takes a few minutes.

## By task

| I want to… | Read |
| --- | --- |
| add refs, tell my agents, keep every project fresh | [Getting started](./guide/getting-started.md) |
| move from vendir | [Getting started §9](./guide/getting-started.md#9-moving-from-vendir) |
| keep the config beside its copies, or move copies to another folder | [Getting started §10–11](./guide/getting-started.md#10-configs-beside-copies) · [Moving copies beside their config](./guide/moving-copies-beside-config.md) |
| know exactly what a command or flag does, or the JSON shape | [CLI reference](./reference/cli.md) |
| know every `tiger.toml` field, global setting, and file tiger writes | [Configuration reference](./reference/config.md) |
| know what's deliberately deferred (npm, Windows, …) and what would change that | [Status](./status.md) |
| change tiger itself | [Architecture](./maintainers/architecture.md) · [Evidence](./maintainers/evidence.md) · [Workflow](./maintainers/workflow.md) |

## How the docs are organized

Every fact has one home.

```text
docs/
├── vision.md          why tiger exists · the model · principles · scope · decisions   (everyone)
├── language.md        vocabulary · naming · error and writing style                     (everyone)
├── status.md          decisions made for now, and what would reopen each                (everyone)
├── explorations/      ideas considered but not built, kept for when they come back      (maintainers)
├── guide/             how to do things, by task                                         (users)
├── reference/         exact contracts                                                   (users)
└── maintainers/       how tiger is built, proven, and released                          (maintainers)
```

- **Guides** teach by task and link to the reference for precise behavior.
- **The reference** states the shipped contract in the present tense. When it and a guide disagree, the reference wins and the guide is the bug.
- **[Status](./status.md)** lists decisions made for now, each with what would reopen it. **Explorations** keep the thinking behind deferred ideas; each says up front that it isn't built.
- **History** lives in [`CHANGELOG.md`](../CHANGELOG.md), whose sections are also the release notes. **Plans and progress** never live here; they live in ignored `__temp__/` records — see [workflow](./maintainers/workflow.md).
