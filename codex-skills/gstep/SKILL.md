---
name: gstep
description: Work with gstep, a Git commit-aware micro-step layer for AI coding workflows. Use when Codex needs to checkpoint intermediate work without creating Git commits, inspect or compare gstep micro steps, branch local experiment variants, materialize a selector to another directory, bind a final Git commit back to a micro step, or use the gstep MCP tools exposed by a configured `gstep` server.
---

# Gstep

## Overview

Use `gstep` to manage temporary micro steps between real Git commits. Git remains the formal history; gstep stores local snapshots under `.git/gstep/` and exposes both a CLI and MCP server for checkpointing, inspecting, diffing, branching, and materializing intermediate work.

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
- `gstep_status`
- `gstep_timeline`
- `gstep_show`
- `gstep_diff`
- `gstep_commit`
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
