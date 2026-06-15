# gstep

`gstep` is a Git commit-aware micro-step layer for AI coding workflows.

Git commits remain the formal project history. `gstep` adds temporary micro steps between those commits so an agent or developer can checkpoint, compare, branch, inspect, and materialize intermediate work without polluting the Git commit log.

For multi-agent workflows, `gstep` can keep one shared logical repository while giving each agent its own transparent writable layer. Agents use the normal commands (`gstep status`, `gstep commit`, `gstep diff`, and so on) from their agent context; conflicts are deferred until commit time.

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

Each commit records which code agent created it (claude / codex) and that
agent's session id — Claude is detected from its environment, Codex from its
active session for the working directory. Override with `--agent` / `--session`:

```sh
gstep commit -m "extract tokenizer" --agent codex --session <id>
```

A *different* code agent can then recover the originating session's context and
continue the work. `gstep context` locates the recorded session's transcript,
parses it (Claude and Codex use different on-disk formats), and prints the
original task plus recent conversation turns:

```sh
gstep context              # latest step (gstep:@)
gstep context gstep:step-2
gstep context --json
```

Currently `claude` and `codex` are supported.

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

Or do the whole thing — lay the step into the worktree, make the Git commit, and
bind it back — in one shot with `promote`:

```sh
gstep promote gstep:step-1 -m "refactor parser"
# add --git-notes to also record provenance in Git notes,
# or --no-bind to skip recording the binding.
```

## Multi-Agent Collaboration

Start a session anchored at the current Git commit. This also initializes the shared agent timeline:

```sh
gstep begin team
```

Create agent layers:

```sh
gstep fork agent-a
gstep fork agent-b
```

Inside an agent process context, native commands operate on the agent layer. The current prototype recognizes the agent from `GSTEP_AGENT` or from the current directory being under the agent view path:

```sh
GSTEP_AGENT=agent-a gstep status
GSTEP_AGENT=agent-a gstep commit -m "agent-a change"
```

`gstep commit` merges the agent layer into the collaboration shared head. Non-overlapping changes are merged automatically. Conflicting edits are recorded in `.git/gstep/state.json`, the shared head is left unchanged, and the agent can keep editing before retrying the same native `gstep commit`.

## Commands

```text
gstep begin <name> [--anchor git:<rev>]
gstep fork <name> [--from <selector>]
gstep status [--all] [--json]
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
gstep promote gstep:<step> -m <message> [--git-notes] [--no-bind]
gstep bind git:<rev> --from gstep:<step> [--git-notes]
gstep mcp
gstep close --prune
```

Run `gstep --help` for the command list, or `gstep <command> --help`
(equivalently `gstep help <command>`) for detailed help on a single command.

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
gstep_fork
gstep_status
gstep_timeline
gstep_show
gstep_diff
gstep_commit
gstep_branch
gstep_checkout
gstep_materialize
gstep_promote
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
.git/gstep/agents/<name>/upper/
.git/gstep/agents/<name>/tombstones
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
