# td2do

A CLI that keeps TODO comments tracked as issues, for a developer and the agents working for them.

A TODO is **tracked** when it points to an open issue: `TODO(gh#123): retry on 429`. Before anyone, person or agent, writes a TODO, `td2do find`
shows whether the work is already recorded; `td2do add` writes the comment and files the issue once; `td2do check` fails a branch that adds a
TODO with no issue.

**Status:** design decided, nothing built yet. See [the design spec](docs/specs/2026-10-29-td2do-design.md).

## Planned commands

| Command | What it does |
| --- | --- |
| `td2do list` | Every TODO with its issue reference |
| `td2do find <words>` | Search TODOs and open `todo` issues before writing a new one |
| `td2do add <file>:<line> <text>` | Write a canonical TODO, filing its issue once |
| `td2do track <file>:<line>` | File an issue for an existing untracked TODO |
| `td2do show <ref>` | An issue and every TODO pointing at it |
| `td2do check` | Fail on new TODOs with no issue (only lines this branch adds) |
| `td2do removed --since <commit> --comment` | Comment on issues whose last TODO was removed |
| `td2do init` | Config, pre-commit hook, CI step and the agent rule |

Every command takes `--json`.

## Plan

Rust, a single binary. GitHub issues first; the pr-gate relay later, through a repo-level issue ledger (spec section 10).
