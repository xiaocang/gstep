# gstep Multi-Agent Collaboration — Gap Analysis & Next Plan

Research into what the multi-agent collaboration features of gstep still need, grounded
in the current code (`src/main.rs`, `tests/cli.rs`).

## 1. What already works

gstep's multi-agent model is an **OverlayFS-style layering** on top of the Git object store:

| Capability | Implementation | Location |
|---|---|---|
| Shared baseline | `begin` creates `Collab{shared_head_tree}`, initialized to the anchor's tree | `main.rs:456` |
| Derive an agent layer | `fork <name>` builds `Agent{base_tree, upper_dir, tombstones, view_path}` | `main.rs:485` |
| Compute an agent view | `agent_tree = base_tree + upper files − tombstones`, written to a new tree | `main.rs:2497` |
| Commit & merge | On commit, a **3-way merge-tree** (base / shared / agent): clean → advance `shared_head_tree` + clear overlay; conflict → record a `Conflict` and do **not** advance the shared head | `main.rs:611, 2665` |
| Current-agent detection | `GSTEP_AGENT` env var, or cwd inside an agent's `view_path` | `main.rs:1974` |
| Multi-layer view | `status --all` lists each layer's dirty/conflict | `main.rs:544` |
| Cross-agent handoff | Each commit records agent + session_id; `context` reads the originating agent's transcript digest | `main.rs:611-644` |

The merge semantics are sound: when agent B's base is stale, its next commit 3-way-merges
in whatever A already landed (rebase-like).

## 2. Gaps (by priority)

### 🔴 P0-1: The agent "write path" is not wired up (missing foundation)
`upper_dir`, `view_path`, and `tombstones` are created, but **no code** ever:
1. materializes `agent_tree` into `view_path` for the agent to edit,
2. syncs the agent's worktree edits back into `upper/`, or
3. records deletions as tombstones.

Tests **write directly** into `.git/gstep/agents/alpha/upper/app.txt` (`tests/cli.rs:313,341`),
bypassing this layer. In a real MCP session an agent has no tool to land edits inside its
own layer — and **deletions are never captured**.

- Interface: `gstep_agent_materialize <name>` (materialize view) + `gstep_agent_sync <name>`
  (diff view↔base, write upper + tombstones); or have `commit` read directly from the
  view_path worktree.
- Difficulty: Medium · **Without this the whole overlay is just scaffolding.**

### 🔴 P0-2: No concurrency control on `state.json` (lost updates)
`load_state`/`save_state` are whole-file read-modify-write with **no lock** (`main.rs:1928-1943`).
Two agents committing concurrently → the later writer clobbers the earlier writer's step /
shared_head / conflicts. (The temp index is PID-isolated, so it's fine; the race is only on
the state file.)

- Interface: file lock (flock / `O_EXCL` lockfile) around read-modify-write, or a CAS on the head.
- Difficulty: Low-Medium · P0

### 🟠 P1-1: Conflicts are a dead end
Conflicts are written into `collab.conflicts` (`main.rs:669`) but **no command lists / shows /
resolves them**; the conflict-marked tree is stored but never surfaced to the agent. The agent
can only blindly re-commit, and the conflict is silently dropped on the next clean commit.

- Interface: `gstep_conflicts` (list), `gstep_conflict_show <id>` (materialize the conflict tree
  with markers), `gstep_resolve <id>` (accept a resolved worktree); optional `--ours/--theirs/--rebase`.
- Difficulty: Medium-High · P1

### 🟠 P1-2: Zero agent-to-agent coordination / no claim mechanism
There is no "who is editing which file" map, so two agents editing the same file = a guaranteed
conflict at commit time, with **no advance warning**. No task assignment, no leases.

- Interface: `gstep_claim <agent> <glob>` (lease with TTL); `status` shows claims and overlapping
  edits; warn at fork/commit if another agent's upper touches the same path.
- Difficulty: Medium · P1

### 🟡 P2
- **Thin observability**: `status --all` is only a snapshot and **does not show conflict details**
  (only the id, `main.rs:604`); there's no per-agent "what changed vs shared head" diff and no
  liveness/heartbeat. → inline per-agent diff + `last_active` in status; `gstep_activity` live feed.
  Difficulty Low-Medium.
- **No lifecycle / GC**: `fork` only adds; the only way to drop a layer is `close --prune`, which
  deletes the entire `.gstep` dir (`main.rs:1262`, all-or-nothing). Zombie layers and shadow
  objects are never reclaimed. → `gstep_agent_drop <name>`, stale detection, `gstep gc`.
  Difficulty Low.
- **No "shared head moved" signal / no proactive rebase**: after A commits, other agents'
  `base_tree` stays stale until their own next commit, with no notification in between.
  → `gstep_agent_rebase`/`pull` to push an idle agent onto the shared head and re-materialize;
  mark "behind by N steps" in status. Difficulty Medium.
- **Non-atomic writes**: `save_state` uses `fs::write` directly (`main.rs:1941`); a crash mid-write
  corrupts state. → write to a temp file + rename. Difficulty Low.

### ⚪ P3
- **Handoff is one-directional and step-scoped**: `context` only reads the originating agent's
  digest for a *committed* step; concurrently-active agents have no shared scratch/intent channel
  ("I'm refactoring auth, don't touch"). → per-agent `note`/intent field; `context --agent <name>`
  to read an uncommitted layer.
- **Permissions / boundaries**: any process can set `GSTEP_AGENT=<any>` and commit as that agent;
  no scope enforcement (acceptable for cooperative agents, but no isolation guarantee). Ties into
  P1-2 (claims).

## 3. Roadmap (by priority)

| Priority | Feature | Why | Difficulty |
|---|---|---|---|
| **P0** | Agent write path (materialize + sync + tombstone capture) | Overlay is otherwise unusable; deletions lost | Medium |
| **P0** | Lock + atomic write for `state.json` | Concurrent commits lose updates / crash corrupts | Low-Medium |
| **P1** | Conflict list/show/resolve (+ rebase) | Conflicts are currently unresolvable | Medium-High |
| **P1** | `claim` / leases + overlap warning | Avoid conflicts up front; enable task assignment | Medium |
| **P2** | status: inline per-agent diff + conflict details + liveness | Observability | Low-Medium |
| **P2** | `agent_drop` / stale detection / `gc` | Lifecycle reclamation | Low |
| **P2** | `agent_rebase`/`pull` + behind-by-N marker | Proactive sync, fewer surprises | Medium |
| **P3** | per-agent note/intent + live-layer context | Real-time coordination | Low-Medium |
| **P3** | scope permission enforcement | Isolation guarantee (optional) | Medium |

## 4. Bottom line

gstep's multi-agent *merge semantics* (3-way merge, provenance, handoff) are well designed, but
two foundations are missing to make it a usable system — **(1) a real write path for agents to
edit/delete within their layer, and (2) a lock for concurrent commits** — plus an upper-layer
**conflict-resolution loop**. These three are the gap between "demoable" and "usable".
