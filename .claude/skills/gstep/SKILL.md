---
name: gstep
description: Dogfood gstep on the gstep repo — drive the gstep MCP (or the local `gstep` CLI) to checkpoint micro steps between Git commits. Use when the user wants to start a session, checkpoint progress, diff/branch/rollback gstep state, or test changes to the gstep tool itself against real workflows.
---

# gstep (project skill)

Drive the **gstep MCP** (`mcp__gstep__*`) tools — or, when developing the tool itself, the locally-built `./target/release/gstep` binary — to capture micro steps between Git commits.

This skill lives in the gstep repo, so it serves two audiences:
1. **Users of gstep working in this repo** — checkpoint refactors and exploratory work.
2. **Contributors developing gstep** — dogfood the CLI/MCP against real changes to validate behavior.

## When this skill fires

- User says: "start gstep", "checkpoint", "snapshot", "save progress", "gstep <something>", `/gstep`.
- Non-trivial multi-step task starting (refactor, multi-file edit, exploration, debugging).
- User is about to make a risky edit and wants rollback.
- User wants to test a gstep code change against a live workflow.

Skip for trivial one-shot edits.

## Two ways to drive gstep

Default to the **MCP** when available. Fall back to the **CLI** when:
- The user explicitly says "use the CLI" / "test the binary".
- They're validating a change to the gstep source (dogfooding).
- The MCP server isn't running.

### MCP tool routing

| Intent | Tool |
|--------|------|
| start / begin a session | `mcp__gstep__gstep_begin` (`name` required; default `anchor: "git:HEAD"`) |
| commit / checkpoint / save | `mcp__gstep__gstep_commit` (terse `message`) |
| status / where am I | `mcp__gstep__gstep_status` |
| timeline / history | `mcp__gstep__gstep_timeline` |
| diff | `mcp__gstep__gstep_diff` |
| show <selector> | `mcp__gstep__gstep_show` |
| branch / variant | `mcp__gstep__gstep_branch` |
| checkout / rollback | `mcp__gstep__gstep_checkout` (`as_worktree: true` to avoid moving HEAD) |
| graduate to Git commit | `mcp__gstep__gstep_bind` (offer `git_notes: true`) |
| materialize a step elsewhere | `mcp__gstep__gstep_materialize` |

### CLI equivalents

Build first if needed: `cargo build --release`. Then `./target/release/gstep`.

| Intent | Command |
|--------|---------|
| start | `gstep begin <name>` |
| commit | `gstep commit -m "<msg>"` |
| status | `gstep status` (add `--json` for structured output) |
| timeline | `gstep timeline` (add `--graph` or `--json`) |
| diff | `gstep diff <from> <to>` (e.g. `git:HEAD gstep:@`) |
| show | `gstep show <selector>` |
| materialize | `gstep ...` (check `gstep --help` for current subcommand) |

If a subcommand isn't documented in README.md, run `./target/release/gstep --help` to discover it — don't guess flags.

## Selectors

- `git:<rev>` — any Git revision (`git:HEAD`, `git:HEAD~2`, `git:main`)
- `gstep:@` — latest gstep step
- `gstep:step-N` — by step number
- `gstep:base` — session anchor
- `worktree` — current working tree

## Workflow

### Step 1: Decide MCP vs CLI

Default MCP. Switch to CLI only when the user is testing the binary or the MCP isn't available.

### Step 2: Gather context (if needed)

For commit/diff/rollback intents, call `gstep_status` first. For a fresh start, go directly to `gstep_begin`.

### Step 3: Infer name / message

- **Session name**: short kebab-case derived from the task. E.g. `refactor-timeline-cmd`, `fix-selector-parse`, `try-json-output`.
- **Step message**: one short concrete phrase. E.g. `"extract helper"`, `"add JSON arm"`, `"fix off-by-one"`. No body.

If unclear what changed, run `git diff` + `gstep_diff worktree gstep:@` (or vs `git:HEAD` if no prior step).

### Step 4: Execute & report

Chain calls when the user requests multiple steps. Final report: one line — what happened, resulting selector if relevant. No long summaries.

## Dogfooding mode (developing gstep itself)

When the user is changing gstep source and wants to validate:

1. `cargo build --release` (or `cargo build` if speed matters more than realism).
2. Use the freshly built `./target/release/gstep` binary directly — do NOT use the MCP server, which may be running an older build.
3. If validating a regression fix, reproduce the bug first with the old behavior described, then rerun with the new binary.
4. After the test, summarize: command run, expected vs actual output.

## Rules

- Prefer `gstep_commit` over throwaway Git commits for in-progress work.
- Never run destructive Git ops (`reset --hard`, `branch -D`) as part of gstep work without explicit user confirmation.
- If the user asks to checkpoint but no session exists, run `gstep_begin` first with an inferred name, then commit.
- Don't mirror every gstep commit into Git — that defeats the purpose.
- When dogfooding, treat MCP and CLI output as independent sources of truth — if they disagree, that's a bug worth surfacing.

## Examples

User: `/gstep start refactoring the timeline command`
→ MCP: `gstep_begin { name: "refactor-timeline-cmd", anchor: "git:HEAD" }`
→ "Started session `refactor-timeline-cmd` at HEAD."

User: `checkpoint`
→ `gstep_diff { from: "gstep:@", to: "worktree" }` to infer message
→ `gstep_commit { message: "<inferred>" }`
→ "Captured → gstep:step-3."

User: `test that --json works on timeline with the new build`
→ Dogfooding mode. `cargo build --release` → `./target/release/gstep timeline --json`
→ Report stdout snippet and whether it parses.

User: `roll back to step 2, keep HEAD where it is`
→ `gstep_checkout { selector: "gstep:step-2", as_worktree: true }`
→ "Materialized step-2 in a worktree; HEAD untouched."
