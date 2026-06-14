# gstep MCP — Manage Micro Steps

Use the **gstep MCP** (`mcp__gstep__*`) tools to capture small, incremental steps while working — keep Git commits for macro-level published history.

## When to use

- Any non-trivial task with multiple intermediate states (refactors, multi-file edits, exploratory implementation, debugging sessions).
- Before risky edits where you may want to roll back to an intermediate state.
- When the user asks you to checkpoint, snapshot, or save progress without polluting Git history.

## Workflow

1. **Start a session** at the beginning of a task:
   `mcp__gstep__gstep_begin` with a descriptive `name` (optionally `anchor: "git:HEAD"`).
2. **Commit micro steps** frequently as you make progress:
   `mcp__gstep__gstep_commit` with a short `message` describing the small step.
3. **Inspect state** as needed:
   - `mcp__gstep__gstep_status` — current macro/micro state
   - `mcp__gstep__gstep_timeline` — combined Git + gstep history
   - `mcp__gstep__gstep_diff` with selectors like `git:HEAD`, `gstep:@`, `gstep:step-N`, `worktree`
   - `mcp__gstep__gstep_show <selector>`
4. **Branch / explore variants** with `mcp__gstep__gstep_branch` when trying alternatives.
5. **Recover state** with `mcp__gstep__gstep_checkout <selector>` (use `as_worktree: true` to materialize without moving HEAD).
6. **Graduate to Git** when a micro step is ready to become a real commit: `mcp__gstep__gstep_promote` makes the Git commit and binds it in one shot (optionally `git_notes: true`, or `no_bind: true`). Use `mcp__gstep__gstep_bind` only to attach an already-made commit to its step.

## Rules

- Prefer `gstep_commit` over creating throwaway Git commits during in-progress work.
- Micro step messages should be terse and concrete (e.g. `"extract helper"`, `"fix off-by-one"`).
- Do **not** use gstep for trivial one-shot tasks (single-file rename, one-line fix) — it's overhead without payoff.
- Selectors: `git:<rev>`, `gstep:@` (latest), `gstep:step-N`, `worktree`.
