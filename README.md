# gstep

`gstep` is a Git commit-aware micro-step layer for AI coding workflows.

Git commits remain the formal project history. `gstep` adds temporary micro steps between those commits so an agent or developer can checkpoint, compare, branch, inspect, and materialize intermediate work without polluting the Git commit log.

## Status

This is an early prototype. It is designed to be small, local-first, and dependency-free.

## Core Model

`gstep` treats the project timeline as two kinds of steps:

- **Git macro steps**: real Git commits, referenced as `git:<rev>`.
- **gstep micro steps**: local temporary snapshots, referenced as `gstep:<step>`.

Examples:

```text
git:HEAD
gstep:base
gstep:@
gstep:step-1
worktree
```

`gstep` reads Git history, but it does not create Git commits, Git branches, or move Git `HEAD`.

## Install

From this repository:

```sh
cargo build --release
```

The binary will be available at:

```sh
target/release/gstep
```

## Quick Start

Start a session anchored at the current Git commit:

```sh
gstep begin refactor-parser
```

Create a micro step from the current worktree:

```sh
gstep commit -m "extract tokenizer"
```

Inspect status:

```sh
gstep status
gstep status --json
```

Compare formal Git history with the current micro step:

```sh
gstep diff git:HEAD gstep:@
gstep diff git:HEAD gstep:@ --json
```

Show the combined timeline:

```sh
gstep timeline
gstep timeline --graph
gstep timeline --json
```

Export a selector to another directory:

```sh
gstep materialize gstep:@ /tmp/gstep-current
gstep materialize git:HEAD~1 /tmp/gstep-old
```

Bind a final Git commit back to the micro step it came from:

```sh
git add -A
git commit -m "refactor parser"
gstep bind git:HEAD --from gstep:step-1
```

Optionally also write the binding as Git notes:

```sh
gstep bind git:HEAD --from gstep:step-1 --git-notes
```

## Commands

```text
gstep begin <name> [--anchor git:<rev>]
gstep status [--json]
gstep timeline [--graph] [--include-git] [--json]
gstep log [--steps-only] [--include-git]
gstep show <selector>
gstep diff <selector-a> <selector-b> [--json]
gstep commit -m <message>
gstep branch <name> [--from <selector>]
gstep checkout gstep:<step-or-branch>
gstep checkout --as-worktree <selector>
gstep revert gstep:<step>
gstep materialize <selector> <path>
gstep bind git:<rev> --from gstep:<step> [--git-notes]
gstep mcp
gstep close --prune
```

## Selectors

```text
git:<rev>       Any Git revision that resolves to a commit.
gstep:base      The Git commit used as the current session anchor.
gstep:@         The current gstep micro step.
gstep:<name>    A gstep micro step or branch.
worktree        The current working tree snapshot.
```

Examples:

```sh
gstep show git:HEAD
gstep show gstep:@
gstep diff git:HEAD~1 git:HEAD
gstep diff gstep:step-1 worktree --json
gstep branch pratt-parser --from git:HEAD
```

## Checkout Safety

`gstep checkout git:<rev>` is intentionally refused by default because it would be easy to confuse with `git checkout` and accidentally move into detached history.

Use one of these explicit forms instead:

```sh
git checkout <rev>
gstep materialize git:<rev> /tmp/view-old
gstep checkout --as-worktree git:<rev>
```

`gstep checkout --as-worktree <selector>` writes the selected tree into the working directory without moving Git `HEAD`.

## MCP Server

`gstep` includes a stdio MCP server:

```sh
gstep mcp
```

It supports:

- `initialize`
- `ping`
- `tools/list`
- `tools/call`

Exposed tools include:

```text
gstep_begin
gstep_status
gstep_timeline
gstep_show
gstep_diff
gstep_commit
gstep_branch
gstep_checkout
gstep_materialize
gstep_bind
```

Example MCP server configuration:

```json
{
  "mcpServers": {
    "gstep": {
      "command": "/absolute/path/to/gstep",
      "args": ["mcp"]
    }
  }
}
```

## Storage

Local metadata is stored under:

```text
.git/gstep/
```

The main files are:

```text
.git/gstep/state.json
.git/gstep/bindings.json
.git/gstep/shadow.git/objects/info/alternates
```

The shadow `alternates` file points at the repository's main Git object store so `gstep` can read Git commit trees while keeping its own metadata separate.

## Development

Run formatting, tests, and lint checks:

```sh
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
```

The integration tests create temporary Git repositories and exercise real Git operations.

## License

MIT. See [LICENSE](LICENSE).
