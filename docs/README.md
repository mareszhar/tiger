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
| know exactly what a command or flag does, or the JSON shape | [CLI reference](./reference/cli.md) |
| know every `tiger.toml` field, global setting, and file tiger writes | [Configuration reference](./reference/config.md) |
| change tiger itself | [Architecture](./maintainers/architecture.md) · [Evidence](./maintainers/evidence.md) · [Workflow](./maintainers/workflow.md) |

## How the docs are organized

Every fact has one home.

```text
docs/
├── vision.md          why tiger exists · the model · principles · scope · decisions   (everyone)
├── language.md        vocabulary · naming · error and writing style                     (everyone)
├── guide/             how to do things, by task                                         (users)
├── reference/         exact contracts                                                   (users)
└── maintainers/       how tiger is built, proven, and released                          (maintainers)
```

- **Guides** teach by task and link to the reference for precise behavior.
- **The reference** states the shipped contract in the present tense. When it and a guide disagree, the reference wins and the guide is the bug.
- **Plans, progress, and history** never live here. They live in ignored `__temp__/` records and, once released, in release notes — see [workflow](./maintainers/workflow.md).
