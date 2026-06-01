---
name: gstep
description: Work with gstep, a Git commit-aware micro-step layer for AI coding workflows. Use when Codex needs to checkpoint intermediate work without creating Git commits, inspect or compare gstep micro steps, branch local experiment variants, materialize a selector to another directory, bind a final Git commit back to a micro step, or use the gstep MCP tools exposed by a configured `gstep` server.
---

# Gstep

## Overview

Use `gstep` to manage temporary micro steps between real Git commits. Git remains the formal history; gstep stores local snapshots under `.git/gstep/` and exposes both a CLI and MCP server for checkpointing, inspecting, diffing, branching, and materializing intermediate work.

For multi-agent collaboration, `gstep` can keep a shared logical repository with one transparent writable layer per agent. Agents should still use native commands (`gstep status`, `gstep commit`, `gstep diff`, and related selector commands) from their agent context instead of a separate `agent` command surface.

## Safety Rules

- Treat Git commits as the durable project history and gstep steps as local scratch checkpoints.
- Do not use gstep as a replacement for `git status`, `git diff`, or normal Git commits when the user explicitly asks for Git operations.
- Check status before mutating the worktree: run `gstep status --json` or call `gstep_status`.
- Prefer `gstep materialize <selector> <path>` for inspection in a separate directory when the user does not clearly want to rewrite the current worktree.
- Use `gstep checkout --as-worktree <selector>` only when writing a selected tree into the current worktree is intentional.
- Do not call `gstep close --prune` unless the user explicitly wants to remove local gstep metadata.

## Command Preference

If the Codex session has a configured `gstep` MCP server, prefer the MCP tools for gstep operations:

- `gstep_begin`
- `gstep_fork`
- `gstep_status`
- `gstep_timeline`
- `gstep_show`
- `gstep_diff`
- `gstep_commit`
- `gstep_context`
- `gstep_branch`
- `gstep_checkout`
- `gstep_materialize`
- `gstep_bind`

If MCP tools are unavailable, use the local CLI. In repos with RTK guidance, prefix shell commands with `rtk`, for example `rtk gstep status --json`.

## Core Workflows

### Start a Session

Use this when the user wants a new set of micro checkpoints for a task.

```sh
gstep begin <name>
gstep begin <name> --anchor git:<rev>
```

Default to anchoring at `git:HEAD` unless the user names a historical commit or branch.

### Checkpoint Work

Use this when the user wants a lightweight checkpoint without a Git commit.

```sh
gstep status --json
gstep commit -m "short checkpoint message"
```

Use specific, action-oriented micro-step messages. Do not stage files or create Git commits as part of this workflow.

### Cross-Agent Handoff

Every `gstep commit` records which code agent created the step and that agent's
session id. Codex is detected automatically from the active session; you do not
need to supply anything. To pick up work another agent (e.g. Claude) checkpointed,
read its session context first:

```sh
gstep context              # the latest step (gstep:@)
gstep context gstep:step-2
gstep context --json
```

This prints the originating agent, its session id, the transcript path, and a
digest of the conversation (the original task plus recent turns) so you can
understand what was being done and continue it. Use this before resuming or
building on a step you did not create yourself.

### Multi-Agent Collaboration

Use this when several agents need isolated views over the same logical repository.

```sh
gstep begin <name>
gstep fork <agent-name>
gstep status --all --json
```

Do not use a top-level `gstep agent` command. Native commands are agent-aware through the current process context, such as `GSTEP_AGENT` or a cwd under the agent view path:

```sh
GSTEP_AGENT=<agent-name> gstep status --json
GSTEP_AGENT=<agent-name> gstep commit -m "short checkpoint message"
```

`gstep commit` merges the current agent's layer into the collaboration shared head. Non-conflicting changes are merged automatically; conflicts are reported and recorded without updating the shared head.

### Inspect Timeline and Content

Use these commands before deciding whether to resume, branch, revert, or bind work:

```sh
gstep timeline --json
gstep timeline --graph --include-git
gstep show gstep:@
gstep show git:HEAD
gstep diff git:HEAD gstep:@
gstep diff gstep:@ worktree --json
```

Selectors include `git:<rev>`, `gstep:base`, `gstep:@`, `gstep:<name>`, and `worktree`.

### Branch or Try Variants

Use this when the user wants alternate local directions without Git branches:

```sh
gstep branch <name> --from <selector>
gstep checkout gstep:<name>
```

Confirm dirty state before checkout. If the user only wants to view a variant, materialize it instead of checking it out.

### Materialize a Selector

Use this when the user wants a copy of a Git or gstep state in another directory:

```sh
gstep materialize <selector> <path>
```

Choose a path under `/tmp` unless the user specifies a destination.

### Bind Final Git History

Use this after the user has created a real Git commit from work that came from a micro step:

```sh
gstep bind git:HEAD --from gstep:<step>
gstep bind git:HEAD --from gstep:<step> --git-notes
```

Only use `--git-notes` if the user wants the binding written into Git notes.

## MCP Configuration

Configure Codex with a stdio server entry that points to an absolute `gstep` binary path:

```toml
[mcp_servers.gstep]
command = "/absolute/path/to/gstep"
args = ["mcp"]
startup_timeout_sec = 3
enabled = true
```

After configuring, start a new Codex session or reload MCP tools so the `gstep_*` tools become available.
