use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Error>;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Error(String);

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self(value.to_string())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Ok(());
    }

    let command = args[0].as_str();
    let rest = &args[1..];

    // Top-level help: `gstep`, `gstep help`, `gstep --help`, `gstep -h`.
    // `gstep help <command>` shows that command's detailed help.
    if matches!(command, "help" | "--help" | "-h") {
        return match rest.first() {
            Some(sub) => print_command_help(sub),
            None => {
                print_usage();
                Ok(())
            }
        };
    }

    // Per-subcommand help: `gstep <command> --help` / `gstep <command> -h`.
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return print_command_help(command);
    }

    match command {
        "begin" => cmd_begin(rest),
        "fork" => cmd_fork(rest),
        "status" => cmd_status(rest),
        "timeline" => cmd_timeline(rest),
        "log" => cmd_log(rest),
        "show" => cmd_show(rest),
        "context" => cmd_context(rest),
        "diff" => cmd_diff(rest),
        "commit" => cmd_commit(rest),
        "branch" => cmd_branch(rest),
        "checkout" => cmd_checkout(rest),
        "revert" => cmd_revert(rest),
        "materialize" => cmd_materialize(rest),
        "promote" => cmd_promote(rest),
        "bind" => cmd_bind(rest),
        "mcp" => cmd_mcp(rest),
        "close" => cmd_close(rest),
        // Multi-agent collaboration commands.
        "agent-materialize" => cmd_agent_materialize(rest),
        "agent-sync" => cmd_agent_sync(rest),
        "agent-drop" => cmd_agent_drop(rest),
        "rebase" => cmd_agent_rebase(rest),
        "conflicts" => cmd_conflicts(rest),
        "conflict-show" => cmd_conflict_show(rest),
        "resolve" => cmd_resolve(rest),
        "claim" => cmd_claim(rest),
        "claims" => cmd_claims(rest),
        "note" => cmd_note(rest),
        "activity" => cmd_activity(rest),
        "gc" => cmd_gc(rest),
        command => Err(Error::new(format!("unknown command: {command}"))),
    }
}

fn print_usage() {
    println!(
        "gstep: Git commit-aware micro steps\n\
\n\
Usage:\n\
  gstep begin <name> [--anchor git:<rev>]\n\
  gstep fork <name> [--from <selector>]\n\
  gstep status [--all] [--json]\n\
  gstep timeline [--graph] [--json]\n\
  gstep log [--steps-only] [--include-git]\n\
  gstep show <selector>\n\
  gstep context [<selector>] [--json]\n\
  gstep diff <selector-a> <selector-b> [--json]\n\
  gstep commit -m <message> [--agent <name>] [--session <id>]\n\
  gstep branch <name> [--from <selector>]\n\
  gstep checkout gstep:<step-or-branch>\n\
  gstep checkout --as-worktree <selector>\n\
  gstep revert gstep:<step>\n\
  gstep materialize <selector> <path>\n\
  gstep promote gstep:<step> -m <message> [--git-notes] [--no-bind]\n\
  gstep bind git:<rev> --from gstep:<step> [--git-notes]\n\
  gstep mcp\n\
  gstep close --prune\n\
\n\
Multi-agent collaboration:\n\
  gstep agent-materialize <name>\n\
  gstep agent-sync <name>\n\
  gstep agent-drop <name>\n\
  gstep rebase <name>\n\
  gstep conflicts [--json]\n\
  gstep conflict-show <id> [--checkout]\n\
  gstep resolve <id> [--ours|--theirs|--from <selector>] [-m <message>]\n\
  gstep claim <agent> <glob> [--ttl <secs>] [--release]\n\
  gstep claims [--json]\n\
  gstep note <agent> [<text...>] [--clear]\n\
  gstep activity [--json] [--limit <n>]\n\
  gstep gc\n\
\n\
Run `gstep <command> --help` for details on a command.\n\
\n\
Selectors: git:<rev>, gstep:@, gstep:base, gstep:<step-or-branch>, worktree"
    );
}

const SELECTORS_HELP: &str =
    "Selectors: git:<rev>, gstep:@, gstep:base, gstep:<step-or-branch>, worktree";

/// Print detailed help for a single subcommand. Reached via
/// `gstep <command> --help`, `gstep <command> -h`, or `gstep help <command>`.
///
/// Each arm returns the help body plus whether to append the selectors
/// footer (only commands taking a generic `<selector>` show it).
fn print_command_help(command: &str) -> Result<()> {
    let (body, show_selectors) = match command {
        "begin" => (
            "gstep begin — Start a gstep session anchored to a Git commit\n\
\n\
Usage:\n\
  gstep begin <name> [--anchor git:<rev>]\n\
\n\
Arguments:\n\
  <name>              Descriptive session name.\n\
\n\
Options:\n\
  --anchor git:<rev>  Git commit to anchor the session to (default: git:HEAD).\n\
  -h, --help          Show this help.",
            false,
        ),
        "fork" => (
            "gstep fork — Create an isolated writable agent layer over the shared head\n\
\n\
Usage:\n\
  gstep fork <name> [--from <selector>]\n\
\n\
Arguments:\n\
  <name>             Agent layer name (ASCII letters, numbers, '-', '_').\n\
\n\
Options:\n\
  --from <selector>  Source selector for the layer's base tree\n\
                     (default: the collaboration shared head).\n\
  -h, --help         Show this help.",
            true,
        ),
        "status" => (
            "gstep status — Show Git macro and gstep micro step status\n\
\n\
Usage:\n\
  gstep status [--all] [--json]\n\
\n\
Options:\n\
  --all       Show all agent layers.\n\
  --json      Emit JSON output.\n\
  -h, --help  Show this help.",
            false,
        ),
        "timeline" => (
            "gstep timeline — Show the combined Git + gstep history\n\
\n\
Usage:\n\
  gstep timeline [--graph] [--json]\n\
\n\
Options:\n\
  --graph     Draw an ASCII graph of the timeline.\n\
  --json      Emit JSON output.\n\
  -h, --help  Show this help.",
            false,
        ),
        "log" => (
            "gstep log — List gstep micro steps\n\
\n\
Usage:\n\
  gstep log [--steps-only] [--include-git]\n\
\n\
Options:\n\
  --steps-only   Show only gstep micro steps (default).\n\
  --include-git  Interleave Git commits with the micro steps.\n\
  -h, --help     Show this help.",
            false,
        ),
        "show" => (
            "gstep show — Show a selector's metadata and files\n\
\n\
Usage:\n\
  gstep show <selector>\n\
\n\
Arguments:\n\
  <selector>  Selector to show.\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            true,
        ),
        "context" => (
            "gstep context — Recover the originating agent's session for a step\n\
\n\
Recover which code agent created a step and a digest of its session, so a\n\
different agent can read what was being done and continue it.\n\
\n\
Usage:\n\
  gstep context [<selector>] [--json]\n\
\n\
Arguments:\n\
  <selector>  Step selector (default: gstep:@).\n\
\n\
Options:\n\
  --json      Emit JSON output.\n\
  -h, --help  Show this help.",
            true,
        ),
        "diff" => (
            "gstep diff — Diff two selectors\n\
\n\
Usage:\n\
  gstep diff <selector-a> <selector-b> [--json]\n\
\n\
Arguments:\n\
  <selector-a> <selector-b>  Selectors to compare.\n\
\n\
Options:\n\
  --json      Emit JSON name-status output.\n\
  -h, --help  Show this help.",
            true,
        ),
        "commit" => (
            "gstep commit — Create a gstep micro step from the worktree\n\
\n\
Creates a micro step from the current worktree, or commits the active agent\n\
layer when one is in context. The committing agent and session id are recorded\n\
automatically; pass --agent/--session to override.\n\
\n\
Usage:\n\
  gstep commit -m <message> [--agent <name>] [--session <id>]\n\
\n\
Options:\n\
  -m, --message <message>  Micro step message (required).\n\
  --agent <name>           Override the recorded committing agent.\n\
  --session <id>           Override the recorded session id.\n\
  -h, --help               Show this help.",
            false,
        ),
        "branch" => (
            "gstep branch — Create a gstep branch / variant\n\
\n\
Usage:\n\
  gstep branch <name> [--from <selector>]\n\
\n\
Arguments:\n\
  <name>             Branch name (ASCII letters, numbers, '-', '_').\n\
\n\
Options:\n\
  --from <selector>  Source selector (default: the current step).\n\
  -h, --help         Show this help.",
            true,
        ),
        "checkout" => (
            "gstep checkout — Write a selector into the worktree without moving Git HEAD\n\
\n\
Usage:\n\
  gstep checkout gstep:<step-or-branch>\n\
  gstep checkout --as-worktree <selector>\n\
\n\
Arguments:\n\
  <selector>     Selector to check out.\n\
\n\
Options:\n\
  --as-worktree  Allow any selector (including git:<rev>) to be written to the\n\
                 worktree without moving Git HEAD.\n\
  -h, --help     Show this help.",
            true,
        ),
        "revert" => (
            "gstep revert — Reset the worktree to a step's parent\n\
\n\
Usage:\n\
  gstep revert gstep:<step>\n\
\n\
Arguments:\n\
  gstep:<step>  Step whose parent the worktree is reset to.\n\
\n\
Options:\n\
  -h, --help    Show this help.",
            false,
        ),
        "materialize" => (
            "gstep materialize — Export a selector's tree to a path\n\
\n\
Usage:\n\
  gstep materialize <selector> <path>\n\
\n\
Arguments:\n\
  <selector>  Selector to export.\n\
  <path>      Destination directory.\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            true,
        ),
        "promote" => (
            "gstep promote — Turn a gstep micro step into a real Git commit\n\
\n\
Lays the step's tree into the worktree, commits it with Git on the current\n\
branch, and binds the new commit back to the step it came from.\n\
\n\
Usage:\n\
  gstep promote gstep:<step> -m <message> [--git-notes] [--no-bind]\n\
\n\
Arguments:\n\
  gstep:<step>  Step to promote.\n\
\n\
Options:\n\
  -m, --message <message>  Git commit message (required).\n\
  --git-notes              Also write provenance to refs/notes/gstep.\n\
  --no-bind                Skip recording the gstep->commit binding.\n\
  -h, --help               Show this help.",
            false,
        ),
        "bind" => (
            "gstep bind — Bind a Git commit to the gstep step it came from\n\
\n\
Usage:\n\
  gstep bind git:<rev> --from gstep:<step> [--git-notes]\n\
\n\
Arguments:\n\
  git:<rev>            Git commit to bind.\n\
\n\
Options:\n\
  --from gstep:<step>  Source step the commit came from (required).\n\
  --git-notes          Also write metadata to refs/notes/gstep.\n\
  -h, --help           Show this help.",
            false,
        ),
        "mcp" => (
            "gstep mcp — Run the MCP server over stdio\n\
\n\
Serves the gstep tools as a Model Context Protocol server, reading JSON-RPC\n\
messages from stdin and writing responses to stdout.\n\
\n\
Usage:\n\
  gstep mcp\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        "close" => (
            "gstep close — Close the session and prune local gstep metadata\n\
\n\
Usage:\n\
  gstep close --prune\n\
\n\
Options:\n\
  --prune     Remove the local gstep metadata directory (required).\n\
  -h, --help  Show this help.",
            false,
        ),
        "agent-materialize" => (
            "gstep agent-materialize — Lay an agent layer into its view worktree\n\
\n\
Materializes the agent's current layer (base + overlay) into its view path so\n\
the agent has a real working directory to edit. Any unsynced edits already in\n\
the view are folded into the overlay first, so nothing is lost.\n\
\n\
Usage:\n\
  gstep agent-materialize <name>\n\
\n\
Arguments:\n\
  <name>      Agent layer name.\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        "agent-sync" => (
            "gstep agent-sync — Fold an agent's view edits back into its overlay\n\
\n\
Reconciles the agent's view worktree (including deletions) into its overlay so\n\
the next commit captures exactly what the agent changed.\n\
\n\
Usage:\n\
  gstep agent-sync <name>\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        "agent-drop" => (
            "gstep agent-drop — Remove an agent layer and reclaim its storage\n\
\n\
Usage:\n\
  gstep agent-drop <name>\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        "rebase" => (
            "gstep rebase — Replay an agent's uncommitted changes onto the shared head\n\
\n\
Brings an idle agent layer up to date with the current shared head without\n\
committing, so it stops being behind. A clean replay updates the layer; an\n\
unmergeable one records a conflict to resolve.\n\
\n\
Usage:\n\
  gstep rebase <name>\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        "conflicts" => (
            "gstep conflicts — List open merge conflicts\n\
\n\
Usage:\n\
  gstep conflicts [--json]\n\
\n\
Options:\n\
  --json      Emit JSON output.\n\
  -h, --help  Show this help.",
            false,
        ),
        "conflict-show" => (
            "gstep conflict-show — Show a conflict, optionally checking out its markers\n\
\n\
Usage:\n\
  gstep conflict-show <id> [--checkout]\n\
\n\
Arguments:\n\
  <id>         Conflict id, e.g. conflict-1.\n\
\n\
Options:\n\
  --checkout   Lay the conflict-marker tree into the agent's view for editing.\n\
  -h, --help   Show this help.",
            false,
        ),
        "resolve" => (
            "gstep resolve — Resolve an open conflict\n\
\n\
--theirs resets the agent onto the shared head (abandoning its change); --ours\n\
lands the agent's clean tree; the default (or --from <selector>) lands a\n\
hand-resolved tree, read from the agent's view unless a selector is given.\n\
\n\
Usage:\n\
  gstep resolve <id> [--ours|--theirs|--from <selector>] [-m <message>] [--force]\n\
\n\
Options:\n\
  --ours              Land the agent's own tree.\n\
  --theirs            Abandon the agent's change; reset to the shared head.\n\
  --from <selector>   Land the given selector's tree as the resolution.\n\
  -m, --message <m>   Message for the resolution step.\n\
  --force             Land even if conflict markers remain.\n\
  -h, --help          Show this help.",
            true,
        ),
        "claim" => (
            "gstep claim — Take or release a path lease for an agent\n\
\n\
Usage:\n\
  gstep claim <agent> <glob> [--ttl <secs>] [--release]\n\
\n\
Arguments:\n\
  <agent>      Agent taking the lease.\n\
  <glob>       Path glob (supports ?, *, **).\n\
\n\
Options:\n\
  --ttl <secs> Lease expiry in seconds.\n\
  --release    Release the matching lease instead of taking it.\n\
  -h, --help   Show this help.",
            false,
        ),
        "claims" => (
            "gstep claims — List active path leases\n\
\n\
Usage:\n\
  gstep claims [--json]\n\
\n\
Options:\n\
  --json      Emit JSON output.\n\
  -h, --help  Show this help.",
            false,
        ),
        "note" => (
            "gstep note — Set or clear an agent's advertised intent\n\
\n\
Usage:\n\
  gstep note <agent> <text...>\n\
  gstep note <agent> --clear\n\
\n\
Options:\n\
  --clear     Remove the agent's note.\n\
  -h, --help  Show this help.",
            false,
        ),
        "activity" => (
            "gstep activity — Show a time-ordered feed of steps and conflicts\n\
\n\
Usage:\n\
  gstep activity [--json] [--limit <n>]\n\
\n\
Options:\n\
  --json        Emit JSON output.\n\
  --limit <n>   Maximum number of events (default 20).\n\
  -h, --help    Show this help.",
            false,
        ),
        "gc" => (
            "gstep gc — Reclaim leftover gstep metadata\n\
\n\
Expires stale claims, deletes temp index files, and removes orphaned on-disk\n\
agent and view directories that no layer references.\n\
\n\
Usage:\n\
  gstep gc\n\
\n\
Options:\n\
  -h, --help  Show this help.",
            false,
        ),
        other => return Err(Error::new(format!("unknown command: {other}"))),
    };
    if show_selectors {
        println!("{body}\n\n{SELECTORS_HELP}");
    } else {
        println!("{body}");
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Context {
    root: PathBuf,
    git_dir: PathBuf,
    gstep_dir: PathBuf,
}

impl Context {
    fn discover() -> Result<Self> {
        let cwd = env::current_dir()?;
        let root = git_at(&cwd, &["rev-parse", "--show-toplevel"])?;
        let git_dir = git_at(&cwd, &["rev-parse", "--git-dir"])?;
        let git_dir = absolute_from(&cwd, Path::new(git_dir.trim()));
        let root = PathBuf::from(root.trim());
        let gstep_dir = git_dir.join("gstep");
        Ok(Self {
            root,
            git_dir,
            gstep_dir,
        })
    }

    fn state_path(&self) -> PathBuf {
        self.gstep_dir.join("state.json")
    }

    fn bindings_path(&self) -> PathBuf {
        self.gstep_dir.join("bindings.json")
    }
}

fn absolute_from(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn cmd_begin(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("begin requires a session name"));
    }

    let name = args[0].clone();
    let mut anchor = "git:HEAD".to_string();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--anchor" => {
                index += 1;
                anchor = args
                    .get(index)
                    .ok_or_else(|| Error::new("--anchor requires a selector"))?
                    .clone();
            }
            other => return Err(Error::new(format!("unknown begin option: {other}"))),
        }
        index += 1;
    }

    if !anchor.starts_with("git:") {
        return Err(Error::new("begin --anchor must be a git:<rev> selector"));
    }

    let ctx = Context::discover()?;
    fs::create_dir_all(&ctx.gstep_dir)?;
    ensure_shadow_repo(&ctx)?;
    let anchor_commit = resolve_git_commit(&ctx, &anchor[4..])?;
    let mut state = State {
        session: name,
        anchor: anchor_commit.clone(),
        current: None,
        next_step: 1,
        steps: Vec::new(),
        branches: Vec::new(),
        collab: None,
    };
    state.collab = Some(Collab {
        shared_head_tree: git_commit_tree(&ctx, &anchor_commit)?,
        next_conflict: 1,
        agents: Vec::new(),
        conflicts: Vec::new(),
        claims: Vec::new(),
    });
    save_state(&ctx, &state)?;

    println!("Started gstep session '{}'", state.session);
    println!("anchor: git:{}", short_oid(&ctx, &anchor_commit)?);

    if let Some(head) = head_commit(&ctx)?
        && head != anchor_commit
    {
        eprintln!(
            "Current Git HEAD is git:{}, but session anchor is git:{}.",
            short_oid(&ctx, &head)?,
            short_oid(&ctx, &anchor_commit)?
        );
        eprintln!("Use git checkout first, or use gstep materialize.");
    }

    Ok(())
}

fn cmd_fork(args: &[String]) -> Result<()> {
    cmd_agent_create(args)
}

fn cmd_agent_create(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("fork requires an agent name"));
    }
    let name = args[0].clone();
    validate_name(&name)?;
    let mut from = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--from" => {
                index += 1;
                from = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--from requires a selector"))?
                        .clone(),
                );
            }
            other => return Err(Error::new(format!("unknown fork option: {other}"))),
        }
        index += 1;
    }

    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    if state.find_agent(&name).is_some() {
        return Err(Error::new(format!("agent already exists: {name}")));
    }
    let base_tree = match from {
        Some(selector) => resolve_selector(&ctx, &state, &selector)?.tree,
        None => require_collab(&state)?.shared_head_tree.clone(),
    };
    let base_step = current_step_id(&state);
    let rel_base = format!("agents/{name}");
    let upper_dir = format!("{rel_base}/upper");
    let tombstones_path = format!("{rel_base}/tombstones");
    let index_path = format!("{rel_base}/index");
    fs::create_dir_all(ctx.gstep_dir.join(&upper_dir))?;
    fs::write(ctx.gstep_dir.join(&tombstones_path), "")?;
    let view_path = Some(default_agent_view_path(&ctx, &state, &name)?);

    require_collab_mut(&mut state)?.agents.push(Agent {
        name: name.clone(),
        base_tree,
        upper_dir,
        tombstones_path,
        index_path,
        view_path,
        conflict: None,
        created_at: current_timestamp(),
        note: None,
        last_active: None,
        base_step,
    });
    save_state(&ctx, &state)?;

    let agent = state.find_agent(&name).expect("agent was just created");
    println!("Created gstep agent {name}");
    println!("base tree: {}", agent.base_tree);
    println!("view path: {}", agent.view_path.as_deref().unwrap_or(""));
    Ok(())
}

/// The step id the session's current pointer resolves to, or None when the
/// current pointer is the git anchor / a branch (no concrete step yet). Used to
/// stamp an agent layer's `base_step` so "behind by N" can be computed.
fn current_step_id(state: &State) -> Option<String> {
    let current = state.current.as_deref()?;
    let name = current.strip_prefix("gstep:")?;
    if state.find_step(name).is_some() {
        Some(name.to_string())
    } else {
        None
    }
}

fn cmd_agent_status(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut all = false;
    let mut name = None;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            other if name.is_none() => name = Some(other.to_string()),
            other => {
                return Err(Error::new(format!(
                    "unexpected agent status argument: {other}"
                )));
            }
        }
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let collab = require_collab(&state)?;
    let agents = if let Some(name) = name.as_ref().filter(|_| !all) {
        vec![
            state
                .find_agent(name)
                .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?,
        ]
    } else {
        collab.agents.iter().collect::<Vec<_>>()
    };

    if json {
        let entries = agents
            .iter()
            .map(|agent| agent_status_json(&ctx, &state, agent))
            .collect::<Result<Vec<_>>>()?
            .join(",\n");
        println!(
            "{{\n  \"shared_head_tree\": {},\n  \"agents\": [\n{}\n  ]\n}}",
            json_string(&collab.shared_head_tree),
            entries
        );
        return Ok(());
    }

    let now = now_secs();
    println!("Shared head tree: {}", collab.shared_head_tree);
    for agent in &agents {
        let tree = agent_tree(&ctx, agent)?;
        let dirty = tree != agent.base_tree;
        println!();
        println!("Agent: {}", agent.name);
        println!("  base:     {}", agent.base_tree);
        println!("  view:     {tree}");
        println!("  dirty:    {}", if dirty { "yes" } else { "no" });
        let behind = agent_behind_by(&state, agent);
        println!(
            "  behind:   {}",
            if behind == 0 {
                "up to date".to_string()
            } else {
                format!("{behind} step(s) — run gstep rebase {}", agent.name)
            }
        );
        if let Some(note) = &agent.note {
            println!("  note:     {note}");
        }
        println!(
            "  active:   {}",
            agent
                .last_active
                .as_deref()
                .and_then(parse_unix_ts)
                .map(|ts| format!("{} ago", format_age(now.saturating_sub(ts))))
                .unwrap_or_else(|| "never".to_string())
        );
        println!(
            "  view path:{}",
            agent.view_path.as_deref().unwrap_or("not assigned")
        );
        // Inline per-agent diff against the shared head.
        if dirty {
            let changes = diff_name_status(&ctx, &collab.shared_head_tree, &tree)?;
            if !changes.is_empty() {
                println!("  changes vs shared:");
                for (status, path) in changes {
                    println!("    {status} {path}");
                }
            }
        }
        match &agent.conflict {
            Some(id) => {
                println!("  conflict: {id}");
                if let Some(conflict) = collab.conflicts.iter().find(|c| &c.id == id) {
                    println!("    paths: {}", conflict.paths.join(", "));
                }
            }
            None => println!("  conflict: none"),
        }
    }

    // Active claims across the collaboration.
    let active_claims: Vec<&Claim> = collab
        .claims
        .iter()
        .filter(|claim| !claim.is_expired(now))
        .collect();
    if !active_claims.is_empty() {
        println!();
        println!("Claims:");
        for claim in active_claims {
            let ttl = match claim.expires_at {
                Some(at) => format!(" (expires in {})", format_age(at.saturating_sub(now))),
                None => String::new(),
            };
            println!("  {} -> {}{ttl}", claim.agent, claim.glob);
        }
    }
    Ok(())
}

fn commit_agent_changes(
    ctx: &Context,
    state: &mut State,
    name: &str,
    message: String,
) -> Result<()> {
    // If the agent has a materialized view, fold its worktree edits (including
    // deletions) back into the overlay before computing the tree to commit.
    sync_agent_view(ctx, state, name)?;
    let agent = state
        .find_agent(name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?
        .clone();
    let agent_tree = agent_tree(ctx, &agent)?;
    let shared_tree = require_collab(state)?.shared_head_tree.clone();
    if agent_tree == agent.base_tree {
        println!("Agent {name} has no changes to commit");
        return Ok(());
    }

    // Surface (or, under enforcement, refuse) edits that collide with another
    // agent's working set or active claims before landing the merge.
    warn_overlaps(ctx, state, name, &agent)?;

    match merge_agent_tree(ctx, &agent.base_tree, &shared_tree, &agent_tree)? {
        MergeOutcome::Clean { tree } => {
            let parent = state
                .current
                .clone()
                .unwrap_or_else(|| format!("git:{}", state.anchor));
            let id = format!("step-{}", state.next_step);
            state.next_step += 1;
            state.steps.push(Step {
                id: id.clone(),
                parent,
                message,
                tree: tree.clone(),
                created_at: current_timestamp(),
                agent: Some(name.to_string()),
                session_id: None,
            });
            state.current = Some(format!("gstep:{id}"));
            if let Some(collab) = state.collab.as_mut() {
                collab.shared_head_tree = tree.clone();
                collab.conflicts.retain(|conflict| conflict.agent != name);
            }
            let now = current_timestamp();
            if let Some(agent_mut) = state.find_agent_mut(name) {
                agent_mut.base_tree = tree.clone();
                agent_mut.base_step = Some(id.clone());
                agent_mut.conflict = None;
                agent_mut.last_active = Some(now);
                clear_agent_overlay(ctx, agent_mut)?;
            }
            let agent_snapshot = state.find_agent(name).cloned();
            save_state(ctx, state)?;
            // Re-materialize the view onto the freshly advanced base so the
            // agent keeps a clean working copy after the merge.
            if let Some(agent_mut) = agent_snapshot {
                rematerialize_view_if_present(ctx, &agent_mut)?;
            }
            println!("Committed agent {name} as gstep:{id}");
            println!("shared head tree: {tree}");
        }
        MergeOutcome::Conflicted {
            tree,
            paths,
            message,
        } => {
            let conflict_id = {
                let collab = require_collab_mut(state)?;
                let id = format!("conflict-{}", collab.next_conflict);
                collab.next_conflict += 1;
                collab.conflicts.retain(|conflict| conflict.agent != name);
                collab.conflicts.push(Conflict {
                    id: id.clone(),
                    agent: name.to_string(),
                    base_tree: agent.base_tree.clone(),
                    shared_tree,
                    agent_tree,
                    marker_tree: tree,
                    paths,
                    message: message.clone(),
                    created_at: current_timestamp(),
                });
                id
            };
            let now = current_timestamp();
            if let Some(agent_mut) = state.find_agent_mut(name) {
                agent_mut.conflict = Some(conflict_id.clone());
                agent_mut.last_active = Some(now);
            }
            save_state(ctx, state)?;
            return Err(Error::new(format!(
                "agent {name} has merge conflicts ({conflict_id})\n\
                 Inspect with: gstep conflict-show {conflict_id}\n\
                 Resolve with: gstep resolve {conflict_id} [--ours|--theirs]\n{message}"
            )));
        }
    }
    Ok(())
}

/// `gstep agent-materialize <name>` — give the agent a working directory by
/// laying its current layer (base + overlay) into its view path. If the view
/// already holds unsynced edits, those are folded in first so nothing is lost.
fn cmd_agent_materialize(args: &[String]) -> Result<()> {
    let name = single_name_arg(args, "agent-materialize")?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    let agent = state
        .find_agent(&name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?
        .clone();
    let view = agent_view_dir(&agent)
        .ok_or_else(|| Error::new(format!("agent {name} has no view path assigned")))?;

    // Preserve any edits already present in the view before we re-lay the tree.
    if view.exists() {
        sync_agent_overlay(&ctx, &agent)?;
    }
    let agent = state.find_agent(&name).expect("agent exists").clone();
    let tree = agent_tree(&ctx, &agent)?;
    materialize_tree_clean(&ctx, &tree, &view)?;

    if let Some(agent_mut) = state.find_agent_mut(&name) {
        agent_mut.last_active = Some(current_timestamp());
    }
    save_state(&ctx, &state)?;
    println!("Materialized agent {name} view at {}", view.display());
    println!("view tree: {tree}");
    println!("Edit files there, then run: gstep agent-sync {name} (or commit as the agent).");
    Ok(())
}

/// `gstep agent-sync <name>` — fold the agent's view worktree edits (including
/// deletions) back into its overlay, so the next commit captures them.
fn cmd_agent_sync(args: &[String]) -> Result<()> {
    let name = single_name_arg(args, "agent-sync")?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    let agent = state
        .find_agent(&name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?
        .clone();
    let changes = sync_agent_overlay(&ctx, &agent)?.ok_or_else(|| {
        Error::new(format!(
            "agent {name} has no materialized view; run gstep agent-materialize {name} first"
        ))
    })?;

    if let Some(agent_mut) = state.find_agent_mut(&name) {
        agent_mut.last_active = Some(current_timestamp());
    }
    save_state(&ctx, &state)?;

    let modified = changes.iter().filter(|(status, _)| *status != 'D').count();
    let deleted = changes.iter().filter(|(status, _)| *status == 'D').count();
    println!("Synced agent {name} view into its overlay");
    println!("  changed: {modified}");
    println!("  deleted: {deleted}");
    for (status, path) in &changes {
        println!("  {status} {path}");
    }
    Ok(())
}

/// Parse a command that takes exactly one positional name argument.
fn single_name_arg(args: &[String], command: &str) -> Result<String> {
    let mut name = None;
    for arg in args {
        if name.is_none() && !arg.starts_with('-') {
            name = Some(arg.clone());
        } else {
            return Err(Error::new(format!("unexpected {command} argument: {arg}")));
        }
    }
    name.ok_or_else(|| Error::new(format!("{command} requires an agent name")))
}

/// Whether the running process should hard-refuse edits to another agent's
/// active claim, rather than only warning. Opt-in so the cooperative default is
/// unchanged.
fn claims_enforced() -> bool {
    matches!(
        env::var("GSTEP_ENFORCE_CLAIMS").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

/// Warn (or, under enforcement, refuse) when the committing agent's edits
/// collide with another agent's working set or an active claim it does not own.
/// Overlap and claim collisions are advisory by default because the agents are
/// cooperative; the merge step is still the source of truth.
fn warn_overlaps(ctx: &Context, state: &State, name: &str, agent: &Agent) -> Result<()> {
    let collab = require_collab(state)?;
    let this_tree = agent_tree(ctx, agent)?;
    let changed: BTreeSet<String> = diff_name_status(ctx, &agent.base_tree, &this_tree)?
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    if changed.is_empty() {
        return Ok(());
    }

    // Overlap with another agent's in-flight (uncommitted) edits.
    for other in &collab.agents {
        if other.name == name {
            continue;
        }
        let other_tree = agent_tree(ctx, other)?;
        if other_tree == other.base_tree {
            continue;
        }
        let other_changed: BTreeSet<String> = diff_name_status(ctx, &other.base_tree, &other_tree)?
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        let overlap: Vec<&String> = changed.intersection(&other_changed).collect();
        if !overlap.is_empty() {
            eprintln!(
                "warning: agent {name} and agent {} both edit: {}",
                other.name,
                overlap
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    // Collision with another agent's active path claim.
    let now = now_secs();
    let mut blocked = Vec::new();
    for claim in &collab.claims {
        if claim.agent == name || claim.is_expired(now) {
            continue;
        }
        let hits: Vec<&String> = changed
            .iter()
            .filter(|p| glob_match(&claim.glob, p))
            .collect();
        if hits.is_empty() {
            continue;
        }
        let joined = hits
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "warning: agent {name} edits paths claimed by agent {} ({}): {joined}",
            claim.agent, claim.glob
        );
        blocked.push(format!("{} (claim {})", joined, claim.agent));
    }
    if claims_enforced() && !blocked.is_empty() {
        return Err(Error::new(format!(
            "agent {name} edits claimed paths: {}\nclaim enforcement (GSTEP_ENFORCE_CLAIMS) is on",
            blocked.join("; ")
        )));
    }
    Ok(())
}

/// Minimal glob matcher over `/`-separated paths supporting `?`, `*` (any run
/// of non-`/` characters), and `**` (any run including `/`). Sufficient for
/// path leases like `src/**` or `auth/*.rs`.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    let mut p = 0;
    let mut t = 0;
    // Backtracking state for the most recent `*` (single-segment wildcard).
    let mut star: Option<(usize, usize)> = None;
    // Whether that pending star is a `**` (crosses `/`).
    let mut star_double = false;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            let double = p + 1 < pattern.len() && pattern[p + 1] == b'*';
            star = Some((p, t));
            star_double = double;
            p += if double { 2 } else { 1 };
        } else if let Some((sp, st)) = star {
            // Extend the wildcard by one char, unless a single `*` would have to
            // swallow a path separator.
            if !star_double && text[st] == b'/' {
                return false;
            }
            p = sp + if star_double { 2 } else { 1 };
            t = st + 1;
            star = Some((sp, t));
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn cmd_status(args: &[String]) -> Result<()> {
    let (json, all_agents) = parse_status_flags(args)?;
    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    if let Some(agent_name) = current_agent_name(&state)? {
        return cmd_current_agent_status(&ctx, &state, &agent_name, json);
    }
    if all_agents {
        let mut status_args = vec!["--all".to_string()];
        if json {
            status_args.push("--json".to_string());
        }
        return cmd_agent_status(&status_args);
    }
    let head = head_commit(&ctx)?;
    let branch = git_optional(&ctx, &["branch", "--show-current"])?.unwrap_or_default();
    let relation = match &head {
        Some(head) => relation_to_anchor(&ctx, &state.anchor, head)?,
        None => "unborn".to_string(),
    };
    let current_selector = state
        .current
        .clone()
        .unwrap_or_else(|| "gstep:base".to_string());
    let current_tree = resolve_selector(&ctx, &state, &current_selector)?.tree;
    let worktree_tree = write_worktree_tree(&ctx)?;
    let dirty = current_tree != worktree_tree;
    let bound = head
        .as_ref()
        .and_then(|commit| {
            load_bindings(&ctx)
                .ok()?
                .get(&format!("git:{commit}"))
                .cloned()
        })
        .map(|binding| binding.from);

    if json {
        let head_json = head.as_deref().unwrap_or("");
        let bound_json = bound
            .as_ref()
            .map(|value| json_string(value))
            .unwrap_or_else(|| "null".to_string());
        println!(
            "{{\n  \"git\": {{\n    \"head\": {},\n    \"branch\": {},\n    \"anchor\": {},\n    \"relation_to_anchor\": {}\n  }},\n  \"gstep\": {{\n    \"session\": {},\n    \"current_step\": {},\n    \"dirty\": {},\n    \"bound_to_git_commit\": {}\n  }}\n}}",
            if head_json.is_empty() {
                "null".to_string()
            } else {
                json_string(head_json)
            },
            json_string(branch.trim()),
            json_string(&state.anchor),
            json_string(&relation),
            json_string(&state.session),
            json_string(&current_selector),
            dirty,
            bound_json
        );
        return Ok(());
    }

    println!("Git:");
    match head {
        Some(head) => println!(
            "  current:  git:{}{}",
            short_oid(&ctx, &head)?,
            if branch.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", branch.trim())
            }
        ),
        None => println!("  current:  unborn"),
    }
    println!("  anchor:   git:{}", short_oid(&ctx, &state.anchor)?);
    println!("  relation: {relation}");
    println!();
    println!("Gstep:");
    println!("  session:  {}", state.session);
    println!("  current:  {current_selector}");
    println!("  dirty:    {}", if dirty { "yes" } else { "no" });
    println!(
        "  bound:    {}",
        bound.unwrap_or_else(|| "not yet".to_string())
    );
    println!();
    println!("Suggested:");
    println!("  gstep diff git:HEAD gstep:@");
    println!("  gstep bind git:HEAD --from {current_selector}");

    Ok(())
}

fn cmd_timeline(args: &[String]) -> Result<()> {
    let mut graph = false;
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--graph" => graph = true,
            "--include-git" => {}
            "--json" => json = true,
            other => return Err(Error::new(format!("unknown timeline option: {other}"))),
        }
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let bindings = load_bindings(&ctx).unwrap_or_default();
    let git_nodes = git_timeline_nodes(&ctx, &state)?;

    if json {
        print_timeline_json(&ctx, &state, &bindings, &git_nodes)?;
    } else {
        print_timeline_text(&ctx, &state, &bindings, &git_nodes, graph)?;
    }

    Ok(())
}

fn cmd_log(args: &[String]) -> Result<()> {
    let mut steps_only = false;
    let mut include_git = false;
    for arg in args {
        match arg.as_str() {
            "--steps-only" => steps_only = true,
            "--include-git" => include_git = true,
            other => return Err(Error::new(format!("unknown log option: {other}"))),
        }
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    if include_git && !steps_only {
        let bindings = load_bindings(&ctx).unwrap_or_default();
        let git_nodes = git_timeline_nodes(&ctx, &state)?;
        print_timeline_text(&ctx, &state, &bindings, &git_nodes, false)?;
        return Ok(());
    }

    for step in &state.steps {
        println!("S  {:<10} {}", step.id, first_line_or_empty(&step.message));
        println!("   parent: {}", step.parent);
    }

    Ok(())
}

fn cmd_show(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        return Err(Error::new("show requires exactly one selector"));
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let resolved = resolve_selector(&ctx, &state, &args[0])?;

    match resolved.kind {
        ResolvedKind::Git { commit } => {
            println!("Git macro step {}", resolved.selector);
            println!("commit {commit}");
            println!("tree {}", resolved.tree);
            println!("message {}", git_commit_message(&ctx, &commit)?);
        }
        ResolvedKind::GstepStep { id } => {
            let step = state
                .find_step(&id)
                .ok_or_else(|| Error::new(format!("missing step after resolution: {id}")))?;
            println!("Gstep micro step gstep:{}", step.id);
            println!("parent {}", step.parent);
            println!("tree {}", step.tree);
            println!("message {}", step.message);
            if let Some(agent) = &step.agent {
                println!("agent {agent}");
            }
            if let Some(session_id) = &step.session_id {
                println!("session {session_id}");
                println!(
                    "(run `gstep context gstep:{}` to recover its session)",
                    step.id
                );
            }
        }
        ResolvedKind::GstepBase => {
            println!("Gstep base");
            println!("anchor git:{}", short_oid(&ctx, &state.anchor)?);
            println!("tree {}", resolved.tree);
        }
        ResolvedKind::GstepBranch { name, target } => {
            println!("Gstep branch gstep:{name}");
            println!("target {target}");
            println!("tree {}", resolved.tree);
        }
        ResolvedKind::Worktree => {
            println!("Worktree snapshot");
            println!("tree {}", resolved.tree);
        }
    }

    println!("files:");
    for file in tree_files(&ctx, &resolved.tree)? {
        println!("  {file}");
    }

    Ok(())
}

fn cmd_diff(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut selectors = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => selectors.push(other.to_string()),
        }
    }
    if selectors.len() != 2 {
        return Err(Error::new("diff requires two selectors"));
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let left = resolve_selector(&ctx, &state, &selectors[0])?;
    let right = resolve_selector(&ctx, &state, &selectors[1])?;

    if json {
        print_diff_json(&ctx, &left, &right)?;
    } else {
        let output = git(&ctx, &["diff", left.tree.as_str(), right.tree.as_str()])?;
        print!("{output}");
    }

    Ok(())
}

fn cmd_commit(args: &[String]) -> Result<()> {
    let commit_args = parse_commit_args(args)?;
    let ctx = Context::discover()?;
    // Hold the state lock across the whole read-modify-write so concurrent
    // agent commits cannot clobber each other's step / shared-head updates.
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    if let Some(agent) = current_agent_name(&state)? {
        return commit_agent_changes(&ctx, &mut state, &agent, commit_args.message);
    }
    let tree = write_worktree_tree(&ctx)?;
    let parent = parent_for_new_step(&state);
    let id = format!("step-{}", state.next_step);
    state.next_step += 1;

    let identity = resolve_commit_identity(&ctx, &commit_args);
    let step = Step {
        id: id.clone(),
        parent,
        message: commit_args.message,
        tree,
        created_at: current_timestamp(),
        agent: identity.as_ref().map(|i| i.agent.clone()),
        session_id: identity.as_ref().and_then(|i| i.session_id.clone()),
    };
    state.current = Some(format!("gstep:{id}"));
    state.steps.push(step);
    save_state(&ctx, &state)?;

    println!("Created gstep micro step gstep:{id}");
    if let Some(i) = &identity {
        match &i.session_id {
            Some(sid) => println!("agent: {} session: {}", i.agent, sid),
            None => println!("agent: {} (no session id detected)", i.agent),
        }
    }
    Ok(())
}

fn cmd_branch(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("branch requires a name"));
    }

    let name = args[0].clone();
    validate_name(&name)?;
    let mut from = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--from" => {
                index += 1;
                from = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--from requires a selector"))?
                        .clone(),
                );
            }
            other => return Err(Error::new(format!("unknown branch option: {other}"))),
        }
        index += 1;
    }

    let ctx = Context::discover()?;
    let mut state = load_state(&ctx)?;
    if state.find_step(&name).is_some() || state.find_branch(&name).is_some() {
        return Err(Error::new(format!(
            "gstep selector already exists: gstep:{name}"
        )));
    }
    let source = from.unwrap_or_else(|| parent_for_new_step(&state));
    let target = canonical_selector(&ctx, &state, &source)?;
    state.branches.push(Branch { name, target });
    save_state(&ctx, &state)?;
    let branch = state.branches.last().expect("branch was just inserted");
    println!(
        "Created gstep branch gstep:{} from {}",
        branch.name, branch.target
    );
    Ok(())
}

fn cmd_checkout(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("checkout requires a selector"));
    }

    let mut as_worktree = false;
    let mut selector = None;
    for arg in args {
        if arg == "--as-worktree" {
            as_worktree = true;
        } else if selector.is_none() {
            selector = Some(arg.clone());
        } else {
            return Err(Error::new(format!("unexpected checkout argument: {arg}")));
        }
    }
    let selector = selector.ok_or_else(|| Error::new("checkout requires a selector"))?;

    if selector.starts_with("git:") && !as_worktree {
        return Err(Error::new(
            "Refusing to move Git HEAD through gstep checkout.\n\nUse:\n  git checkout <rev>\nor:\n  gstep materialize git:<rev> <path>\nor:\n  gstep checkout --as-worktree git:<rev>",
        ));
    }

    let ctx = Context::discover()?;
    let mut state = load_state(&ctx)?;
    let resolved = resolve_selector(&ctx, &state, &selector)?;
    checkout_tree_to_worktree(&ctx, &resolved.tree)?;

    if !as_worktree && selector.starts_with("gstep:") {
        state.current = Some(selector);
        save_state(&ctx, &state)?;
    }

    println!(
        "Checked out {} into the worktree without moving Git HEAD",
        resolved.selector
    );
    Ok(())
}

fn cmd_revert(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        return Err(Error::new("revert requires a gstep:<step> selector"));
    }
    let selector = &args[0];
    if !selector.starts_with("gstep:") {
        return Err(Error::new("revert only accepts gstep:<step> selectors"));
    }

    let ctx = Context::discover()?;
    let mut state = load_state(&ctx)?;
    let step_id = &selector[6..];
    let step = state
        .find_step(step_id)
        .ok_or_else(|| Error::new(format!("unknown gstep step: {selector}")))?
        .clone();
    let parent = resolve_selector(&ctx, &state, &step.parent)?;
    checkout_tree_to_worktree(&ctx, &parent.tree)?;
    state.current = if step.parent.starts_with("gstep:") {
        Some(step.parent.clone())
    } else {
        None
    };
    save_state(&ctx, &state)?;
    println!("Reverted worktree to parent {}", step.parent);
    Ok(())
}

fn cmd_materialize(args: &[String]) -> Result<()> {
    if args.len() != 2 {
        return Err(Error::new("materialize requires <selector> <path>"));
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let resolved = resolve_selector(&ctx, &state, &args[0])?;
    let path = PathBuf::from(&args[1]);
    let dest = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };
    materialize_tree(&ctx, &resolved.tree, &dest)?;
    println!("Materialized {} at {}", resolved.selector, dest.display());
    Ok(())
}

fn cmd_bind(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(Error::new("bind requires git:<rev>"));
    }

    let git_selector = args[0].clone();
    if !git_selector.starts_with("git:") {
        return Err(Error::new("bind target must be git:<rev>"));
    }

    let mut from = None;
    let mut git_notes = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--from" => {
                index += 1;
                from = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--from requires gstep:<step>"))?
                        .clone(),
                );
            }
            "--git-notes" => git_notes = true,
            other => return Err(Error::new(format!("unknown bind option: {other}"))),
        }
        index += 1;
    }

    let from = from.ok_or_else(|| Error::new("bind requires --from gstep:<step>"))?;
    if !from.starts_with("gstep:") {
        return Err(Error::new("bind --from must be a gstep:<step> selector"));
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let commit = resolve_git_commit(&ctx, &git_selector[4..])?;
    let canonical_from = canonical_selector(&ctx, &state, &from)?;
    if !canonical_from.starts_with("gstep:") {
        return Err(Error::new("bind --from must resolve to a gstep step"));
    }

    let mut bindings = load_bindings(&ctx).unwrap_or_default();
    let key = format!("git:{commit}");
    bindings.insert(
        key.clone(),
        Binding {
            from: canonical_from.clone(),
            session: state.session.clone(),
            bound_at: current_timestamp(),
        },
    );
    save_bindings(&ctx, &bindings)?;

    if git_notes {
        let note = format!(
            "gstep.from={canonical_from}\ngstep.session={}",
            state.session
        );
        git(
            &ctx,
            &[
                "notes",
                "--ref",
                "refs/notes/gstep",
                "add",
                "-f",
                "-m",
                note.as_str(),
                commit.as_str(),
            ],
        )?;
    }

    println!("Bound {key} from {canonical_from}");
    Ok(())
}

/// Turn a gstep micro step into a real Git commit in one shot: lay the step's
/// tree into the worktree, make a Git commit on the current branch, then bind
/// the new commit back to the step it came from. This is the
/// `checkout -> git commit -> bind` workflow collapsed into a single command.
fn cmd_promote(args: &[String]) -> Result<()> {
    let mut selector = None;
    let mut message = None;
    let mut git_notes = false;
    let mut no_bind = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-m" | "--message" => {
                index += 1;
                message = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("promote message flag requires a value"))?
                        .clone(),
                );
            }
            "--git-notes" => git_notes = true,
            "--no-bind" => no_bind = true,
            other if selector.is_none() && !other.starts_with('-') => {
                selector = Some(other.to_string());
            }
            other => return Err(Error::new(format!("unknown promote option: {other}"))),
        }
        index += 1;
    }

    let selector =
        selector.ok_or_else(|| Error::new("promote requires a gstep:<step> selector"))?;
    if !selector.starts_with("gstep:") {
        return Err(Error::new("promote only accepts gstep:<step> selectors"));
    }
    let message = message.ok_or_else(|| Error::new("promote requires -m <message>"))?;

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let resolved = resolve_selector(&ctx, &state, &selector)?;
    let canonical_from = canonical_selector(&ctx, &state, &selector)?;

    // Lay the step's tree into the worktree, then make a real Git commit so HEAD
    // advances on the current branch with exactly that content. Git hooks may
    // adjust files, so the final commit tree is whatever Git records.
    checkout_tree_to_worktree(&ctx, &resolved.tree)?;
    git(&ctx, &["add", "-A"])?;
    git(&ctx, &["commit", "-m", message.as_str()])?;
    let commit = resolve_git_commit(&ctx, "HEAD")?;
    let short = short_oid(&ctx, &commit)?;
    println!("Promoted {} to git:{short}", resolved.selector);

    // Record provenance so the new commit knows which step it came from. Only
    // bind when the source resolves to an actual gstep step (a branch may point
    // straight at a git base, which is already a commit and has nothing to bind).
    if !no_bind && canonical_from.starts_with("gstep:") {
        let mut bindings = load_bindings(&ctx).unwrap_or_default();
        let key = format!("git:{commit}");
        bindings.insert(
            key,
            Binding {
                from: canonical_from.clone(),
                session: state.session.clone(),
                bound_at: current_timestamp(),
            },
        );
        save_bindings(&ctx, &bindings)?;

        if git_notes {
            let note = format!(
                "gstep.from={canonical_from}\ngstep.session={}",
                state.session
            );
            git(
                &ctx,
                &[
                    "notes",
                    "--ref",
                    "refs/notes/gstep",
                    "add",
                    "-f",
                    "-m",
                    note.as_str(),
                    commit.as_str(),
                ],
            )?;
        }
        println!("Bound git:{short} from {canonical_from}");
    }
    Ok(())
}

fn cmd_close(args: &[String]) -> Result<()> {
    let mut prune = false;
    for arg in args {
        match arg.as_str() {
            "--prune" => prune = true,
            other => return Err(Error::new(format!("unknown close option: {other}"))),
        }
    }
    if !prune {
        return Err(Error::new("close currently requires --prune"));
    }

    let ctx = Context::discover()?;
    if ctx.gstep_dir.exists() {
        fs::remove_dir_all(&ctx.gstep_dir)?;
    }
    println!("Closed gstep session and pruned local gstep metadata");
    Ok(())
}

// ===== Conflict resolution loop (P1-1) =====

/// `gstep conflicts [--json]` — list the open merge conflicts recorded when
/// agent commits could not be merged cleanly into the shared head.
fn cmd_conflicts(args: &[String]) -> Result<()> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(Error::new(format!("unknown conflicts option: {other}"))),
        }
    }
    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let collab = require_collab(&state)?;
    let now = now_secs();

    if json {
        let entries = collab
            .conflicts
            .iter()
            .map(|conflict| {
                let paths = conflict
                    .paths
                    .iter()
                    .map(|path| json_string(path))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "    {{\"id\": {}, \"agent\": {}, \"paths\": [{}], \"created_at\": {}}}",
                    json_string(&conflict.id),
                    json_string(&conflict.agent),
                    paths,
                    json_string(&conflict.created_at)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        println!("{{\n  \"conflicts\": [\n{entries}\n  ]\n}}");
        return Ok(());
    }

    if collab.conflicts.is_empty() {
        println!("No open conflicts.");
        return Ok(());
    }
    println!("Open conflicts:");
    for conflict in &collab.conflicts {
        let age = parse_unix_ts(&conflict.created_at)
            .map(|ts| format!(" ({} ago)", format_age(now.saturating_sub(ts))))
            .unwrap_or_default();
        println!("  {} agent {}{age}", conflict.id, conflict.agent);
        println!("    paths: {}", conflict.paths.join(", "));
        println!(
            "    resolve: gstep resolve {} [--ours|--theirs|--from <selector>]",
            conflict.id
        );
    }
    Ok(())
}

/// `gstep conflict-show <id> [--checkout]` — show a conflict's detail, and with
/// `--checkout` lay the conflict-marker tree into the agent's view so it can be
/// resolved by hand in place.
fn cmd_conflict_show(args: &[String]) -> Result<()> {
    let mut id = None;
    let mut checkout = false;
    for arg in args {
        match arg.as_str() {
            "--checkout" => checkout = true,
            other if id.is_none() && !other.starts_with('-') => id = Some(other.to_string()),
            other => {
                return Err(Error::new(format!(
                    "unexpected conflict-show argument: {other}"
                )));
            }
        }
    }
    let id = id.ok_or_else(|| Error::new("conflict-show requires a conflict id"))?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    let conflict = find_conflict(require_collab(&state)?, &id)?.clone();

    println!("Conflict {}", conflict.id);
    println!("  agent:       {}", conflict.agent);
    println!("  base tree:   {}", conflict.base_tree);
    println!("  shared tree: {}", conflict.shared_tree);
    println!("  agent tree:  {}", conflict.agent_tree);
    println!("  marker tree: {}", conflict.marker_tree);
    println!("  paths:");
    for path in &conflict.paths {
        println!("    {path}");
    }
    println!("  message:");
    for line in conflict.message.lines() {
        println!("    {line}");
    }

    if checkout {
        let agent = state
            .find_agent(&conflict.agent)
            .ok_or_else(|| Error::new(format!("unknown agent: {}", conflict.agent)))?
            .clone();
        let view = agent_view_dir(&agent).ok_or_else(|| {
            Error::new(format!(
                "agent {} has no view path to check out into",
                conflict.agent
            ))
        })?;
        materialize_tree_clean(&ctx, &conflict.marker_tree, &view)?;
        if let Some(agent_mut) = state.find_agent_mut(&conflict.agent) {
            agent_mut.last_active = Some(current_timestamp());
        }
        save_state(&ctx, &state)?;
        println!();
        println!("Checked out conflict markers into {}", view.display());
        println!(
            "Edit the marked files, then run: gstep resolve {} (reads the resolved view)",
            conflict.id
        );
    }
    Ok(())
}

/// `gstep resolve <id> [--ours|--theirs|--from <selector>] [-m <msg>] [--force]`
/// — close a conflict. `--theirs` abandons the agent's change and resets it to
/// the shared head; `--ours` lands the agent's clean tree; the default (or
/// `--from`) lands a hand-resolved tree (read from the agent's view, or the
/// given selector) as a new shared step.
fn cmd_resolve(args: &[String]) -> Result<()> {
    let mut id = None;
    let mut mode = ResolveMode::Manual;
    let mut from = None;
    let mut message = None;
    let mut force = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--ours" => mode = ResolveMode::Ours,
            "--theirs" => mode = ResolveMode::Theirs,
            "--from" => {
                index += 1;
                from = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--from requires a selector"))?
                        .clone(),
                );
                mode = ResolveMode::From;
            }
            "-m" | "--message" => {
                index += 1;
                message = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("resolve message flag requires a value"))?
                        .clone(),
                );
            }
            "--force" => force = true,
            other if id.is_none() && !other.starts_with('-') => id = Some(other.to_string()),
            other => return Err(Error::new(format!("unexpected resolve argument: {other}"))),
        }
        index += 1;
    }
    let id = id.ok_or_else(|| Error::new("resolve requires a conflict id"))?;

    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    let conflict = find_conflict(require_collab(&state)?, &id)?.clone();
    let agent_name = conflict.agent.clone();
    let shared = require_collab(&state)?.shared_head_tree.clone();

    // --theirs: the agent abandons its change; just reset it onto the current
    // shared head. Nothing new lands in the shared timeline.
    if matches!(mode, ResolveMode::Theirs) {
        reset_agent_to_shared(&ctx, &mut state, &agent_name, &shared, &id)?;
        save_state(&ctx, &state)?;
        let agent = state.find_agent(&agent_name).cloned();
        if let Some(agent) = agent {
            rematerialize_view_if_present(&ctx, &agent)?;
        }
        println!("Resolved {id} with --theirs: agent {agent_name} reset to the shared head.");
        return Ok(());
    }

    // The resolved content that should become the new shared head.
    let resolved_tree = match mode {
        ResolveMode::Ours => conflict.agent_tree.clone(),
        ResolveMode::From => {
            let selector = from.as_ref().expect("--from sets a selector");
            resolve_selector(&ctx, &state, selector)?.tree
        }
        ResolveMode::Manual => {
            let agent = state
                .find_agent(&agent_name)
                .ok_or_else(|| Error::new(format!("unknown agent: {agent_name}")))?
                .clone();
            let view = agent_view_dir(&agent).ok_or_else(|| {
                Error::new(format!(
                    "agent {agent_name} has no view to read a manual resolution from; \
                     pass --ours/--theirs/--from <selector>, or run gstep conflict-show {id} --checkout first"
                ))
            })?;
            if !view.exists() {
                return Err(Error::new(format!(
                    "agent {agent_name} view is not materialized; run gstep conflict-show {id} --checkout, resolve the markers, then retry"
                )));
            }
            dir_tree(&ctx, &view)?
        }
        ResolveMode::Theirs => unreachable!("handled above"),
    };

    // Guard against landing unresolved conflict markers.
    if !force {
        let leftover = paths_with_markers(&ctx, &resolved_tree, &conflict.paths)?;
        if !leftover.is_empty() {
            return Err(Error::new(format!(
                "resolved tree still has conflict markers in: {}\nfix them, or pass --force to land as-is",
                leftover.join(", ")
            )));
        }
    }

    // Land the resolution as a new shared-head step authored by the agent.
    let parent = state
        .current
        .clone()
        .unwrap_or_else(|| format!("git:{}", state.anchor));
    let step_id = format!("step-{}", state.next_step);
    state.next_step += 1;
    let resolve_message = message.unwrap_or_else(|| format!("resolve {id} ({agent_name})"));
    state.steps.push(Step {
        id: step_id.clone(),
        parent,
        message: resolve_message,
        tree: resolved_tree.clone(),
        created_at: current_timestamp(),
        agent: Some(agent_name.clone()),
        session_id: None,
    });
    state.current = Some(format!("gstep:{step_id}"));
    if let Some(collab) = state.collab.as_mut() {
        collab.shared_head_tree = resolved_tree.clone();
        collab.conflicts.retain(|conflict| conflict.id != id);
    }
    let now = current_timestamp();
    if let Some(agent_mut) = state.find_agent_mut(&agent_name) {
        agent_mut.base_tree = resolved_tree.clone();
        agent_mut.base_step = Some(step_id.clone());
        agent_mut.conflict = None;
        agent_mut.last_active = Some(now);
        clear_agent_overlay(&ctx, agent_mut)?;
    }
    let agent = state.find_agent(&agent_name).cloned();
    save_state(&ctx, &state)?;
    if let Some(agent) = agent {
        rematerialize_view_if_present(&ctx, &agent)?;
    }
    println!("Resolved {id} as gstep:{step_id}");
    println!("shared head tree: {resolved_tree}");
    Ok(())
}

enum ResolveMode {
    Ours,
    Theirs,
    From,
    Manual,
}

/// Reset an agent onto the shared head: clear its overlay, conflict, and point
/// its base at `shared`. Used by `--theirs` resolution and by `agent-drop`'s
/// cleanup. Does not advance the shared timeline.
fn reset_agent_to_shared(
    ctx: &Context,
    state: &mut State,
    agent_name: &str,
    shared: &str,
    conflict_id: &str,
) -> Result<()> {
    if let Some(collab) = state.collab.as_mut() {
        collab
            .conflicts
            .retain(|conflict| conflict.id != conflict_id);
    }
    let latest_step = state.steps.last().map(|step| step.id.clone());
    let now = current_timestamp();
    if let Some(agent_mut) = state.find_agent_mut(agent_name) {
        agent_mut.base_tree = shared.to_string();
        agent_mut.base_step = latest_step;
        agent_mut.conflict = None;
        agent_mut.last_active = Some(now);
        clear_agent_overlay(ctx, agent_mut)?;
    }
    Ok(())
}

fn find_conflict<'a>(collab: &'a Collab, id: &str) -> Result<&'a Conflict> {
    collab
        .conflicts
        .iter()
        .find(|conflict| conflict.id == id)
        .ok_or_else(|| Error::new(format!("unknown conflict: {id}")))
}

/// Of `paths`, return those whose blob in `tree` still carries a Git conflict
/// marker (`<<<<<<<`), so a half-resolved tree is not silently landed.
fn paths_with_markers(ctx: &Context, tree: &str, paths: &[String]) -> Result<Vec<String>> {
    let mut leftover = Vec::new();
    for path in paths {
        let spec = format!("{tree}:{path}");
        if let Some(contents) = git_optional(ctx, &["show", spec.as_str()])?
            && contents.lines().any(|line| line.starts_with("<<<<<<<"))
        {
            leftover.push(path.clone());
        }
    }
    Ok(leftover)
}

// ===== Claims / leases (P1-2) =====

/// `gstep claim <agent> <glob> [--ttl <secs>] [--release]` — take or release a
/// path lease so peers get an up-front warning before editing the same files.
fn cmd_claim(args: &[String]) -> Result<()> {
    let mut positional = Vec::new();
    let mut ttl = None;
    let mut release = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--ttl" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| Error::new("--ttl requires a value in seconds"))?;
                ttl = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| Error::new("--ttl must be a whole number of seconds"))?,
                );
            }
            "--release" => release = true,
            other if !other.starts_with('-') => positional.push(other.to_string()),
            other => return Err(Error::new(format!("unknown claim option: {other}"))),
        }
        index += 1;
    }
    if positional.len() != 2 {
        return Err(Error::new("claim requires <agent> <glob>"));
    }
    let agent = positional[0].clone();
    let glob = positional[1].clone();

    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    if state.find_agent(&agent).is_none() {
        return Err(Error::new(format!("unknown agent: {agent}")));
    }
    let collab = require_collab_mut(&mut state)?;
    if release {
        let before = collab.claims.len();
        collab
            .claims
            .retain(|claim| !(claim.agent == agent && claim.glob == glob));
        let removed = before - collab.claims.len();
        save_state(&ctx, &state)?;
        println!("Released {removed} claim(s) for agent {agent} on {glob}");
        return Ok(());
    }
    // Replace any identical existing lease so re-claiming refreshes the TTL.
    collab
        .claims
        .retain(|claim| !(claim.agent == agent && claim.glob == glob));
    let expires_at = ttl.map(|secs| now_secs() + secs);
    collab.claims.push(Claim {
        agent: agent.clone(),
        glob: glob.clone(),
        created_at: current_timestamp(),
        expires_at,
    });
    save_state(&ctx, &state)?;
    match ttl {
        Some(secs) => println!("Agent {agent} claimed {glob} (expires in {secs}s)"),
        None => println!("Agent {agent} claimed {glob}"),
    }
    Ok(())
}

/// `gstep claims [--json]` — list active path leases (expired ones are hidden).
fn cmd_claims(args: &[String]) -> Result<()> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(Error::new(format!("unknown claims option: {other}"))),
        }
    }
    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let collab = require_collab(&state)?;
    let now = now_secs();
    let active: Vec<&Claim> = collab
        .claims
        .iter()
        .filter(|claim| !claim.is_expired(now))
        .collect();

    if json {
        let entries = active
            .iter()
            .map(|claim| {
                let expires = claim
                    .expires_at
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    "    {{\"agent\": {}, \"glob\": {}, \"expires_at\": {}}}",
                    json_string(&claim.agent),
                    json_string(&claim.glob),
                    expires
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        println!("{{\n  \"claims\": [\n{entries}\n  ]\n}}");
        return Ok(());
    }

    if active.is_empty() {
        println!("No active claims.");
        return Ok(());
    }
    println!("Active claims:");
    for claim in active {
        let ttl = match claim.expires_at {
            Some(at) => format!(" (expires in {})", format_age(at.saturating_sub(now))),
            None => String::new(),
        };
        println!("  {} -> {}{ttl}", claim.agent, claim.glob);
    }
    Ok(())
}

// ===== Lifecycle: drop, gc, rebase (P2) =====

/// `gstep agent-drop <name>` — remove an agent layer and reclaim its on-disk
/// overlay, view, index, conflicts, and claims.
fn cmd_agent_drop(args: &[String]) -> Result<()> {
    let name = single_name_arg(args, "agent-drop")?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    let agent = state
        .find_agent(&name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?
        .clone();

    // Reclaim on-disk artifacts before dropping the record. The overlay,
    // tombstones, and index all live under agents/<name>, so remove that whole
    // directory rather than its individual files.
    let upper_dir = ctx.gstep_dir.join(&agent.upper_dir);
    if let Some(agent_base) = upper_dir.parent() {
        let _ = fs::remove_dir_all(agent_base);
    } else {
        let _ = fs::remove_dir_all(&upper_dir);
    }
    if let Some(view) = agent_view_dir(&agent)
        && view.exists()
    {
        let _ = fs::remove_dir_all(&view);
    }

    let collab = require_collab_mut(&mut state)?;
    collab.agents.retain(|candidate| candidate.name != name);
    collab.conflicts.retain(|conflict| conflict.agent != name);
    collab.claims.retain(|claim| claim.agent != name);
    save_state(&ctx, &state)?;
    println!("Dropped agent {name} and reclaimed its layer");
    Ok(())
}

/// `gstep gc` — reclaim leftover state: expire stale claims, delete temp index
/// files, and remove orphaned on-disk agent/view directories with no record.
fn cmd_gc(args: &[String]) -> Result<()> {
    if let Some(arg) = args.first() {
        return Err(Error::new(format!("unknown gc option: {arg}")));
    }
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;

    // 1. Expire claims whose TTL has passed.
    let now = now_secs();
    let mut expired = 0;
    if let Some(collab) = state.collab.as_mut() {
        let before = collab.claims.len();
        collab.claims.retain(|claim| !claim.is_expired(now));
        expired = before - collab.claims.len();
    }
    save_state(&ctx, &state)?;

    // 2. Remove temp index files left behind by interrupted operations.
    let mut tmp_removed = 0;
    let tmp_dir = ctx.gstep_dir.join("tmp");
    if let Ok(entries) = fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            if fs::remove_file(entry.path()).is_ok() {
                tmp_removed += 1;
            }
        }
    }

    // 3. Remove on-disk agent dirs with no corresponding layer in state.
    let known: BTreeSet<String> = state
        .collab
        .as_ref()
        .map(|collab| collab.agents.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default();
    let mut orphan_agents = 0;
    let agents_dir = ctx.gstep_dir.join("agents");
    if let Ok(entries) = fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !known.contains(&name) && fs::remove_dir_all(entry.path()).is_ok() {
                orphan_agents += 1;
            }
        }
    }

    // 4. Remove orphaned view dirs (no agent points at them).
    let live_views: BTreeSet<PathBuf> = state
        .collab
        .as_ref()
        .map(|collab| collab.agents.iter().filter_map(agent_view_dir).collect())
        .unwrap_or_default();
    let mut orphan_views = 0;
    let views_root = ctx.gstep_dir.join("views");
    if let Ok(sessions) = fs::read_dir(&views_root) {
        for session in sessions.flatten() {
            if let Ok(views) = fs::read_dir(session.path()) {
                for view in views.flatten() {
                    if !live_views.contains(&view.path()) && fs::remove_dir_all(view.path()).is_ok()
                    {
                        orphan_views += 1;
                    }
                }
            }
        }
    }

    println!("Garbage collected gstep metadata:");
    println!("  expired claims:  {expired}");
    println!("  temp files:      {tmp_removed}");
    println!("  orphan agents:   {orphan_agents}");
    println!("  orphan views:    {orphan_views}");
    Ok(())
}

/// `gstep rebase <name>` (a.k.a. pull) — replay an idle agent's uncommitted
/// changes on top of the current shared head without committing, so it stops
/// being behind. A clean replay updates the layer's base and overlay; an
/// unmergeable one records a conflict to resolve.
fn cmd_agent_rebase(args: &[String]) -> Result<()> {
    let name = single_name_arg(args, "rebase")?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    state
        .find_agent(&name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?;

    // Fold any view edits in first so the replay sees the agent's real state.
    sync_agent_view(&ctx, &state, &name)?;
    let agent = state.find_agent(&name).expect("agent exists").clone();
    let shared = require_collab(&state)?.shared_head_tree.clone();
    if agent.base_tree == shared {
        println!("Agent {name} is already on the shared head; nothing to rebase");
        return Ok(());
    }
    let layer_tree = agent_tree(&ctx, &agent)?;

    match merge_agent_tree(&ctx, &agent.base_tree, &shared, &layer_tree)? {
        MergeOutcome::Clean { tree } => {
            // Point the layer at the new base, then rebuild its overlay so the
            // agent's uncommitted delta (merged - shared) survives the move.
            let latest_step = state.steps.last().map(|step| step.id.clone());
            if let Some(agent_mut) = state.find_agent_mut(&name) {
                agent_mut.base_tree = shared.clone();
                agent_mut.base_step = latest_step;
                agent_mut.conflict = None;
                agent_mut.last_active = Some(current_timestamp());
            }
            let rebased = state.find_agent(&name).expect("agent exists").clone();
            rebuild_overlay_from_tree(&ctx, &rebased, &tree)?;
            rematerialize_view_if_present(&ctx, &rebased)?;
            save_state(&ctx, &state)?;
            println!("Rebased agent {name} onto the shared head");
            println!("layer tree: {tree}");
        }
        MergeOutcome::Conflicted {
            tree,
            paths,
            message,
        } => {
            let conflict_id = {
                let collab = require_collab_mut(&mut state)?;
                let id = format!("conflict-{}", collab.next_conflict);
                collab.next_conflict += 1;
                collab.conflicts.retain(|conflict| conflict.agent != name);
                collab.conflicts.push(Conflict {
                    id: id.clone(),
                    agent: name.clone(),
                    base_tree: agent.base_tree.clone(),
                    shared_tree: shared.clone(),
                    agent_tree: layer_tree.clone(),
                    marker_tree: tree,
                    paths,
                    message: message.clone(),
                    created_at: current_timestamp(),
                });
                id
            };
            if let Some(agent_mut) = state.find_agent_mut(&name) {
                agent_mut.conflict = Some(conflict_id.clone());
                agent_mut.last_active = Some(current_timestamp());
            }
            save_state(&ctx, &state)?;
            return Err(Error::new(format!(
                "agent {name} cannot rebase cleanly ({conflict_id})\n\
                 Resolve with: gstep resolve {conflict_id} [--ours|--theirs]\n{message}"
            )));
        }
    }
    Ok(())
}

/// Rebuild an agent's overlay (upper + tombstones) so that `agent_tree` equals
/// `tree`, given the agent's base is already set. Materializes `tree` into a
/// scratch dir and reuses the same view→overlay reconciliation as sync.
fn rebuild_overlay_from_tree(ctx: &Context, agent: &Agent, tree: &str) -> Result<()> {
    let scratch = ctx.gstep_dir.join("tmp").join(format!(
        "rebase-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&scratch);
    materialize_tree(ctx, tree, &scratch)?;
    let changes = diff_name_status(ctx, &agent.base_tree, tree)?;
    clear_agent_overlay(ctx, agent)?;
    let upper_dir = ctx.gstep_dir.join(&agent.upper_dir);
    let mut tombstones = Vec::new();
    for (status, path) in &changes {
        if *status == 'D' {
            tombstones.push(path.clone());
        } else {
            copy_into_upper(&scratch, &upper_dir, path)?;
        }
    }
    let mut body = tombstones.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    fs::write(ctx.gstep_dir.join(&agent.tombstones_path), body)?;
    let _ = fs::remove_dir_all(&scratch);
    Ok(())
}

// ===== Per-agent intent notes (P3) =====

/// `gstep note <agent> [<text...>] [--clear]` — set or clear the agent's
/// advertised intent, shown to peers in status and `context --agent`.
fn cmd_note(args: &[String]) -> Result<()> {
    let mut agent = None;
    let mut words = Vec::new();
    let mut clear = false;
    for arg in args {
        match arg.as_str() {
            "--clear" => clear = true,
            other if agent.is_none() => agent = Some(other.to_string()),
            other => words.push(other.to_string()),
        }
    }
    let agent = agent.ok_or_else(|| Error::new("note requires an agent name"))?;
    let ctx = Context::discover()?;
    let _lock = StateLock::acquire(&ctx)?;
    let mut state = load_state(&ctx)?;
    if state.find_agent(&agent).is_none() {
        return Err(Error::new(format!("unknown agent: {agent}")));
    }
    let note = if clear {
        None
    } else if words.is_empty() {
        return Err(Error::new(
            "note requires text, or pass --clear to remove it",
        ));
    } else {
        Some(words.join(" "))
    };
    if let Some(agent_mut) = state.find_agent_mut(&agent) {
        agent_mut.note = note.clone();
        agent_mut.last_active = Some(current_timestamp());
    }
    save_state(&ctx, &state)?;
    match note {
        Some(text) => println!("Set note for agent {agent}: {text}"),
        None => println!("Cleared note for agent {agent}"),
    }
    Ok(())
}

// ===== Activity feed (P2) =====

/// `gstep activity [--json] [--limit N]` — a time-ordered feed of recent steps
/// (with their authoring agent) and recorded conflicts, newest first.
fn cmd_activity(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut limit = 20usize;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--limit" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| Error::new("--limit requires a value"))?;
                limit = value
                    .parse()
                    .map_err(|_| Error::new("--limit must be a whole number"))?;
            }
            other => return Err(Error::new(format!("unknown activity option: {other}"))),
        }
        index += 1;
    }

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let now = now_secs();

    struct Event {
        ts: u64,
        kind: &'static str,
        id: String,
        agent: String,
        detail: String,
    }
    let mut events = Vec::new();
    for step in &state.steps {
        events.push(Event {
            ts: parse_unix_ts(&step.created_at).unwrap_or(0),
            kind: "step",
            id: step.id.clone(),
            agent: step.agent.clone().unwrap_or_default(),
            detail: first_line_or_empty(&step.message).to_string(),
        });
    }
    if let Some(collab) = state.collab.as_ref() {
        for conflict in &collab.conflicts {
            events.push(Event {
                ts: parse_unix_ts(&conflict.created_at).unwrap_or(0),
                kind: "conflict",
                id: conflict.id.clone(),
                agent: conflict.agent.clone(),
                detail: conflict.paths.join(", "),
            });
        }
    }
    // Newest first; ties keep insertion order deterministic.
    events.sort_by_key(|event| std::cmp::Reverse(event.ts));
    events.truncate(limit);

    if json {
        let entries = events
            .iter()
            .map(|event| {
                format!(
                    "    {{\"kind\": {}, \"id\": {}, \"agent\": {}, \"detail\": {}, \"ts\": {}}}",
                    json_string(event.kind),
                    json_string(&event.id),
                    json_string(&event.agent),
                    json_string(&event.detail),
                    event.ts
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        println!("{{\n  \"activity\": [\n{entries}\n  ]\n}}");
        return Ok(());
    }

    if events.is_empty() {
        println!("No activity yet.");
        return Ok(());
    }
    println!("Activity (newest first):");
    for event in events {
        let age = if event.ts == 0 {
            String::new()
        } else {
            format!(" {} ago", format_age(now.saturating_sub(event.ts)))
        };
        let agent = if event.agent.is_empty() {
            String::new()
        } else {
            format!(" [{}]", event.agent)
        };
        println!("  {:<9} {}{agent}{age}", event.kind, event.id);
        if !event.detail.is_empty() {
            println!("      {}", event.detail);
        }
    }
    Ok(())
}

fn cmd_mcp(args: &[String]) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::new("mcp does not accept command-line arguments"));
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_mcp_message(&line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_mcp_message(input: &str) -> Option<String> {
    let message = match parse_json(input) {
        Ok(Json::Object(object)) => object,
        Ok(_) => return Some(mcp_error(None, -32600, "Invalid Request")),
        Err(error) => return Some(mcp_error(None, -32700, &error.0)),
    };
    let id = message.get("id").cloned();
    let method = match message.get("method") {
        Some(Json::String(method)) => method.as_str(),
        _ => {
            return id
                .as_ref()
                .map(|id| mcp_error(Some(id), -32600, "Invalid Request"));
        }
    };

    match method {
        "initialize" => id.as_ref().map(mcp_initialize_response),
        "notifications/initialized" => None,
        "ping" => id.as_ref().map(|id| mcp_success(id, "{}")),
        "tools/list" => id.as_ref().map(mcp_tools_list_response),
        "tools/call" => {
            let id = id.as_ref()?;
            match mcp_call_tool(&message) {
                Ok(result) => Some(mcp_success(id, &result)),
                Err(error) => Some(mcp_error(Some(id), -32602, &error.0)),
            }
        }
        _ => id
            .as_ref()
            .map(|id| mcp_error(Some(id), -32601, "Method not found")),
    }
}

fn mcp_initialize_response(id: &Json) -> String {
    mcp_success(
        id,
        "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{\"tools\":{\"listChanged\":false}},\"serverInfo\":{\"name\":\"gstep\",\"version\":\"0.1.0\"}}",
    )
}

fn mcp_tools_list_response(id: &Json) -> String {
    mcp_success(id, &format!("{{\"tools\":[{}]}}", mcp_tools().join(",")))
}

fn mcp_tools() -> Vec<String> {
    vec![
        mcp_tool(
            "gstep_begin",
            "Begin a gstep session anchored to a Git commit.",
            &[
                ("name", "string", "Session name."),
                ("anchor", "string", "Optional git:<rev> anchor."),
            ],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_status",
            "Show Git macro step and gstep micro step status, or status for an agent context.",
            &[
                ("agent", "string", "Optional current agent context."),
                ("all", "boolean", "Show all agent layers."),
            ],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_fork",
            "Create an agent layer from the collaboration shared head or a selector.",
            &[
                ("name", "string", "Agent name."),
                ("from", "string", "Optional source selector."),
            ],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_timeline",
            "Show the combined Git commit and gstep micro step timeline.",
            &[("json", "boolean", "Return JSON timeline output.")],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_show",
            "Show a Git or gstep selector.",
            &[(
                "selector",
                "string",
                "Selector such as git:HEAD, gstep:@, or gstep:step-1.",
            )],
            &["selector"],
            true,
            false,
        ),
        mcp_tool(
            "gstep_diff",
            "Diff two Git/gstep/worktree selectors.",
            &[
                ("left", "string", "Left selector."),
                ("right", "string", "Right selector."),
                ("json", "boolean", "Return JSON name-status output."),
            ],
            &["left", "right"],
            true,
            false,
        ),
        mcp_tool(
            "gstep_commit",
            "Create a gstep micro step from the current worktree, or commit the current agent layer when one is active. The committing code agent and its session id are recorded automatically (claude via environment, codex via the active session); pass agent/session to override.",
            &[
                ("message", "string", "Micro step message."),
                (
                    "agent",
                    "string",
                    "Optional code agent name override, e.g. claude or codex.",
                ),
                (
                    "session",
                    "string",
                    "Optional session id override for the committing agent.",
                ),
            ],
            &["message"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_context",
            "Recover context: for a committed step, the originating agent's session digest so a different agent can continue it; with `agent`, the live state of an uncommitted agent layer (its note/intent, dirty status, changed paths, and conflict) for real-time coordination.",
            &[
                ("selector", "string", "Step selector (default gstep:@)."),
                (
                    "agent",
                    "string",
                    "Read a live, uncommitted agent layer instead of a committed step.",
                ),
            ],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_branch",
            "Create a gstep branch or variant.",
            &[
                ("name", "string", "Branch name."),
                ("from", "string", "Optional source selector."),
            ],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_checkout",
            "Checkout a gstep selector, or explicitly materialize a selector into the worktree.",
            &[
                ("selector", "string", "Selector to check out."),
                (
                    "as_worktree",
                    "boolean",
                    "Allow any selector to be written to the worktree without moving Git HEAD.",
                ),
            ],
            &["selector"],
            false,
            true,
        ),
        mcp_tool(
            "gstep_materialize",
            "Export a selector to a separate path without changing the current repository.",
            &[
                ("selector", "string", "Selector to export."),
                ("path", "string", "Destination path."),
            ],
            &["selector", "path"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_promote",
            "Turn a gstep micro step into a real Git commit on the current branch in one shot: lays the step's tree into the worktree, commits it with Git, and binds the new commit back to the step. Use when a checkpoint is ready to become a permanent commit.",
            &[
                (
                    "selector",
                    "string",
                    "Step selector to promote, for example gstep:@ or gstep:step-3.",
                ),
                ("message", "string", "Git commit message."),
                (
                    "git_notes",
                    "boolean",
                    "Also write provenance to refs/notes/gstep.",
                ),
                (
                    "no_bind",
                    "boolean",
                    "Skip recording the gstep->commit binding.",
                ),
            ],
            &["selector", "message"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_bind",
            "Bind a Git commit to the gstep micro step it came from.",
            &[
                ("git", "string", "Git selector, for example git:HEAD."),
                ("from", "string", "Source gstep selector."),
                (
                    "git_notes",
                    "boolean",
                    "Also write metadata to refs/notes/gstep.",
                ),
            ],
            &["git", "from"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_agent_materialize",
            "Lay an agent layer (base + overlay) into its view directory so the agent has a working copy to edit. Folds any unsynced view edits in first. Run before an agent starts editing.",
            &[("name", "string", "Agent layer name.")],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_agent_sync",
            "Reconcile an agent's view worktree edits (including deletions) back into its overlay so the next commit captures them. Commit auto-syncs, so this is mainly for inspecting the captured change set.",
            &[("name", "string", "Agent layer name.")],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_agent_drop",
            "Remove an agent layer and reclaim its overlay, view, index, conflicts, and claims.",
            &[("name", "string", "Agent layer name.")],
            &["name"],
            false,
            true,
        ),
        mcp_tool(
            "gstep_rebase",
            "Replay an idle agent's uncommitted changes onto the current shared head without committing, so it stops being behind. A clean replay updates the layer; an unmergeable one records a conflict to resolve.",
            &[("name", "string", "Agent layer name.")],
            &["name"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_conflicts",
            "List open merge conflicts recorded when agent commits could not merge cleanly into the shared head.",
            &[("json", "boolean", "Return JSON output.")],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_conflict_show",
            "Show a conflict's detail, and optionally check the conflict-marker tree out into the agent's view for hand resolution.",
            &[
                ("id", "string", "Conflict id, e.g. conflict-1."),
                (
                    "checkout",
                    "boolean",
                    "Lay the conflict markers into the agent's view for editing.",
                ),
            ],
            &["id"],
            true,
            false,
        ),
        mcp_tool(
            "gstep_resolve",
            "Resolve an open conflict. ours lands the agent's clean tree; theirs abandons it and resets to the shared head; otherwise a hand-resolved tree is landed (from the agent's view, or the given selector).",
            &[
                ("id", "string", "Conflict id."),
                ("ours", "boolean", "Land the agent's own tree."),
                (
                    "theirs",
                    "boolean",
                    "Abandon the agent's change; reset to shared head.",
                ),
                (
                    "from",
                    "string",
                    "Land this selector's tree as the resolution.",
                ),
                ("message", "string", "Message for the resolution step."),
                ("force", "boolean", "Land even if conflict markers remain."),
            ],
            &["id"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_claim",
            "Take or release a path lease for an agent so peers are warned before editing the same files.",
            &[
                ("agent", "string", "Agent taking the lease."),
                ("glob", "string", "Path glob (supports ?, *, **)."),
                ("ttl", "number", "Lease expiry in seconds."),
                (
                    "release",
                    "boolean",
                    "Release the matching lease instead of taking it.",
                ),
            ],
            &["agent", "glob"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_claims",
            "List active path leases (expired ones are hidden).",
            &[("json", "boolean", "Return JSON output.")],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_note",
            "Set or clear an agent's advertised intent (e.g. 'refactoring auth, don't touch'), shown to peers in status and context.",
            &[
                ("agent", "string", "Agent name."),
                ("text", "string", "Intent text to set."),
                ("clear", "boolean", "Clear the agent's note."),
            ],
            &["agent"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_activity",
            "Show a time-ordered feed of recent steps (with their authoring agent) and recorded conflicts, newest first.",
            &[
                ("json", "boolean", "Return JSON output."),
                ("limit", "number", "Maximum number of events (default 20)."),
            ],
            &[],
            true,
            false,
        ),
        mcp_tool(
            "gstep_gc",
            "Reclaim leftover gstep metadata: expire stale claims, delete temp index files, and remove orphaned agent and view directories.",
            &[],
            &[],
            false,
            true,
        ),
    ]
}

fn mcp_tool(
    name: &str,
    description: &str,
    properties: &[(&str, &str, &str)],
    required: &[&str],
    read_only: bool,
    destructive: bool,
) -> String {
    let properties = properties
        .iter()
        .map(|(name, kind, description)| {
            format!(
                "{}:{{\"type\":{},\"description\":{}}}",
                json_string(name),
                json_string(kind),
                json_string(description)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let required = required
        .iter()
        .map(|name| json_string(name))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"name\":{},\"title\":{},\"description\":{},\"inputSchema\":{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}]}},\"annotations\":{{\"readOnlyHint\":{},\"destructiveHint\":{},\"openWorldHint\":false}}}}",
        json_string(name),
        json_string(name),
        json_string(description),
        properties,
        required,
        read_only,
        destructive
    )
}

fn mcp_call_tool(message: &BTreeMap<String, Json>) -> Result<String> {
    let params = match message.get("params") {
        Some(Json::Object(params)) => params,
        _ => return Err(Error::new("tools/call requires object params")),
    };
    let name = object_string(params, "name")?;
    let empty = BTreeMap::new();
    let arguments = match params.get("arguments") {
        Some(Json::Object(arguments)) => arguments,
        Some(_) => return Err(Error::new("tools/call arguments must be an object")),
        None => &empty,
    };

    let invocation = mcp_tool_args(&name, arguments)?;
    let output = run_current_exe(&invocation.args, &invocation.envs)?;
    Ok(mcp_tool_result(&output))
}

struct McpInvocation {
    args: Vec<String>,
    envs: Vec<(String, String)>,
}

impl McpInvocation {
    fn new() -> Self {
        Self {
            args: Vec::new(),
            envs: Vec::new(),
        }
    }

    fn set_agent(&mut self, agent: Option<String>) {
        if let Some(agent) = agent {
            self.envs.push(("GSTEP_AGENT".to_string(), agent));
        }
    }
}

fn mcp_tool_args(name: &str, arguments: &BTreeMap<String, Json>) -> Result<McpInvocation> {
    let mut invocation = McpInvocation::new();
    match name {
        "gstep_begin" => {
            invocation.args.push("begin".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
            if let Some(anchor) = optional_string_arg(arguments, "anchor")? {
                invocation.args.push("--anchor".to_string());
                invocation.args.push(anchor);
            }
        }
        "gstep_status" => {
            invocation.args.push("status".to_string());
            if optional_bool_arg(arguments, "all")?.unwrap_or(false) {
                invocation.args.push("--all".to_string());
            }
            invocation.args.push("--json".to_string());
            invocation.set_agent(optional_string_arg(arguments, "agent")?);
        }
        "gstep_fork" => {
            invocation.args.push("fork".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
            if let Some(source) = optional_string_arg(arguments, "from")? {
                invocation.args.push("--from".to_string());
                invocation.args.push(source);
            }
        }
        "gstep_timeline" => {
            invocation.args.push("timeline".to_string());
            if optional_bool_arg(arguments, "json")?.unwrap_or(true) {
                invocation.args.push("--json".to_string());
            }
        }
        "gstep_show" => {
            invocation.args.push("show".to_string());
            invocation.args.push(required_arg(arguments, "selector")?);
        }
        "gstep_diff" => {
            invocation.args.push("diff".to_string());
            invocation.args.push(required_arg(arguments, "left")?);
            invocation.args.push(required_arg(arguments, "right")?);
            if optional_bool_arg(arguments, "json")?.unwrap_or(false) {
                invocation.args.push("--json".to_string());
            }
        }
        "gstep_commit" => {
            invocation.args.push("commit".to_string());
            invocation.args.push("-m".to_string());
            invocation.args.push(required_arg(arguments, "message")?);
            if let Some(agent) = optional_string_arg(arguments, "agent")? {
                invocation.args.push("--agent".to_string());
                invocation.args.push(agent);
            }
            if let Some(session) = optional_string_arg(arguments, "session")? {
                invocation.args.push("--session".to_string());
                invocation.args.push(session);
            }
        }
        "gstep_context" => {
            invocation.args.push("context".to_string());
            if let Some(selector) = optional_string_arg(arguments, "selector")? {
                invocation.args.push(selector);
            }
            if let Some(agent) = optional_string_arg(arguments, "agent")? {
                invocation.args.push("--agent".to_string());
                invocation.args.push(agent);
            }
            invocation.args.push("--json".to_string());
        }
        "gstep_branch" => {
            invocation.args.push("branch".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
            if let Some(source) = optional_string_arg(arguments, "from")? {
                invocation.args.push("--from".to_string());
                invocation.args.push(source);
            }
        }
        "gstep_checkout" => {
            invocation.args.push("checkout".to_string());
            if optional_bool_arg(arguments, "as_worktree")?.unwrap_or(false) {
                invocation.args.push("--as-worktree".to_string());
            }
            invocation.args.push(required_arg(arguments, "selector")?);
        }
        "gstep_materialize" => {
            invocation.args.push("materialize".to_string());
            invocation.args.push(required_arg(arguments, "selector")?);
            invocation.args.push(required_arg(arguments, "path")?);
        }
        "gstep_promote" => {
            invocation.args.push("promote".to_string());
            invocation.args.push(required_arg(arguments, "selector")?);
            invocation.args.push("-m".to_string());
            invocation.args.push(required_arg(arguments, "message")?);
            if optional_bool_arg(arguments, "git_notes")?.unwrap_or(false) {
                invocation.args.push("--git-notes".to_string());
            }
            if optional_bool_arg(arguments, "no_bind")?.unwrap_or(false) {
                invocation.args.push("--no-bind".to_string());
            }
        }
        "gstep_bind" => {
            invocation.args.push("bind".to_string());
            invocation.args.push(required_arg(arguments, "git")?);
            invocation.args.push("--from".to_string());
            invocation.args.push(required_arg(arguments, "from")?);
            if optional_bool_arg(arguments, "git_notes")?.unwrap_or(false) {
                invocation.args.push("--git-notes".to_string());
            }
        }
        "gstep_agent_materialize" => {
            invocation.args.push("agent-materialize".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
        }
        "gstep_agent_sync" => {
            invocation.args.push("agent-sync".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
        }
        "gstep_agent_drop" => {
            invocation.args.push("agent-drop".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
        }
        "gstep_rebase" => {
            invocation.args.push("rebase".to_string());
            invocation.args.push(required_arg(arguments, "name")?);
        }
        "gstep_conflicts" => {
            invocation.args.push("conflicts".to_string());
            if optional_bool_arg(arguments, "json")?.unwrap_or(true) {
                invocation.args.push("--json".to_string());
            }
        }
        "gstep_conflict_show" => {
            invocation.args.push("conflict-show".to_string());
            invocation.args.push(required_arg(arguments, "id")?);
            if optional_bool_arg(arguments, "checkout")?.unwrap_or(false) {
                invocation.args.push("--checkout".to_string());
            }
        }
        "gstep_resolve" => {
            invocation.args.push("resolve".to_string());
            invocation.args.push(required_arg(arguments, "id")?);
            if optional_bool_arg(arguments, "ours")?.unwrap_or(false) {
                invocation.args.push("--ours".to_string());
            }
            if optional_bool_arg(arguments, "theirs")?.unwrap_or(false) {
                invocation.args.push("--theirs".to_string());
            }
            if let Some(from) = optional_string_arg(arguments, "from")? {
                invocation.args.push("--from".to_string());
                invocation.args.push(from);
            }
            if let Some(message) = optional_string_arg(arguments, "message")? {
                invocation.args.push("-m".to_string());
                invocation.args.push(message);
            }
            if optional_bool_arg(arguments, "force")?.unwrap_or(false) {
                invocation.args.push("--force".to_string());
            }
        }
        "gstep_claim" => {
            invocation.args.push("claim".to_string());
            invocation.args.push(required_arg(arguments, "agent")?);
            invocation.args.push(required_arg(arguments, "glob")?);
            if let Some(ttl) = optional_number_arg(arguments, "ttl")? {
                invocation.args.push("--ttl".to_string());
                invocation.args.push(ttl);
            }
            if optional_bool_arg(arguments, "release")?.unwrap_or(false) {
                invocation.args.push("--release".to_string());
            }
        }
        "gstep_claims" => {
            invocation.args.push("claims".to_string());
            if optional_bool_arg(arguments, "json")?.unwrap_or(true) {
                invocation.args.push("--json".to_string());
            }
        }
        "gstep_note" => {
            invocation.args.push("note".to_string());
            invocation.args.push(required_arg(arguments, "agent")?);
            if optional_bool_arg(arguments, "clear")?.unwrap_or(false) {
                invocation.args.push("--clear".to_string());
            } else {
                invocation.args.push(required_arg(arguments, "text")?);
            }
        }
        "gstep_activity" => {
            invocation.args.push("activity".to_string());
            if optional_bool_arg(arguments, "json")?.unwrap_or(true) {
                invocation.args.push("--json".to_string());
            }
            if let Some(limit) = optional_number_arg(arguments, "limit")? {
                invocation.args.push("--limit".to_string());
                invocation.args.push(limit);
            }
        }
        "gstep_gc" => {
            invocation.args.push("gc".to_string());
        }
        _ => return Err(Error::new(format!("unknown tool: {name}"))),
    }
    Ok(invocation)
}

/// Read an argument that may arrive as a JSON number or string, returning its
/// textual form for the CLI subprocess.
fn optional_number_arg(arguments: &BTreeMap<String, Json>, name: &str) -> Result<Option<String>> {
    match arguments.get(name) {
        Some(Json::Number(value)) => Ok(Some(value.to_string())),
        Some(Json::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(Error::new(format!("argument must be a number: {name}"))),
        None => Ok(None),
    }
}

fn required_arg(arguments: &BTreeMap<String, Json>, name: &str) -> Result<String> {
    optional_string_arg(arguments, name)?
        .ok_or_else(|| Error::new(format!("missing required argument: {name}")))
}

fn optional_string_arg(arguments: &BTreeMap<String, Json>, name: &str) -> Result<Option<String>> {
    match arguments.get(name) {
        Some(Json::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(Error::new(format!("argument must be a string: {name}"))),
        None => Ok(None),
    }
}

fn optional_bool_arg(arguments: &BTreeMap<String, Json>, name: &str) -> Result<Option<bool>> {
    match arguments.get(name) {
        Some(Json::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(Error::new(format!("argument must be a boolean: {name}"))),
        None => Ok(None),
    }
}

fn run_current_exe(args: &[String], envs: &[(String, String)]) -> Result<Output> {
    let executable = env::current_exe()?;
    let mut command = Command::new(executable);
    command.current_dir(env::current_dir()?).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    Ok(command.output()?)
}

fn mcp_tool_result(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let is_error = !output.status.success();
    let prefer_stderr =
        (is_error && !stderr.trim().is_empty()) || (stdout.is_empty() && !stderr.is_empty());
    let text = if prefer_stderr {
        stderr.to_string()
    } else {
        stdout.to_string()
    };
    format!(
        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{}}}",
        json_string(&text),
        is_error
    )
}

fn mcp_success(id: &Json, result: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
        json_value(id),
        result
    )
}

fn mcp_error(id: Option<&Json>, code: i64, message: &str) -> String {
    let id = id.map(json_value).unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":{}}}}}",
        id,
        code,
        json_string(message)
    )
}

fn parse_status_flags(args: &[String]) -> Result<(bool, bool)> {
    let mut json = false;
    let mut all_agents = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--all" => all_agents = true,
            other => return Err(Error::new(format!("unknown status option: {other}"))),
        }
    }
    Ok((json, all_agents))
}

struct CommitArgs {
    message: String,
    agent: Option<String>,
    session_id: Option<String>,
}

fn parse_commit_args(args: &[String]) -> Result<CommitArgs> {
    let mut message = None;
    let mut agent = None;
    let mut session_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-m" | "--message" => {
                index += 1;
                message = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("commit message flag requires a value"))?
                        .clone(),
                );
            }
            "--agent" => {
                index += 1;
                agent = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--agent requires a value"))?
                        .clone(),
                );
            }
            "--session" => {
                index += 1;
                session_id = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--session requires a value"))?
                        .clone(),
                );
            }
            other => return Err(Error::new(format!("unknown commit option: {other}"))),
        }
        index += 1;
    }
    let message = message.ok_or_else(|| Error::new("commit requires -m <message>"))?;
    Ok(CommitArgs {
        message,
        agent,
        session_id,
    })
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::new(
            "gstep names may only contain ASCII letters, numbers, '-' and '_'",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct State {
    session: String,
    anchor: String,
    current: Option<String>,
    next_step: usize,
    steps: Vec<Step>,
    branches: Vec<Branch>,
    collab: Option<Collab>,
}

impl State {
    fn find_step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|step| step.id == id)
    }

    fn find_branch(&self, name: &str) -> Option<&Branch> {
        self.branches.iter().find(|branch| branch.name == name)
    }

    fn find_agent(&self, name: &str) -> Option<&Agent> {
        self.collab
            .as_ref()?
            .agents
            .iter()
            .find(|agent| agent.name == name)
    }

    fn find_agent_mut(&mut self, name: &str) -> Option<&mut Agent> {
        self.collab
            .as_mut()?
            .agents
            .iter_mut()
            .find(|agent| agent.name == name)
    }
}

#[derive(Clone, Debug)]
struct Step {
    id: String,
    parent: String,
    message: String,
    tree: String,
    created_at: String,
    // Cross-agent handoff: which code agent created this step, and its session id.
    // Both optional for backward compatibility with states written before this feature.
    agent: Option<String>,
    session_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Branch {
    name: String,
    target: String,
}

#[derive(Clone, Debug)]
struct Collab {
    shared_head_tree: String,
    next_conflict: usize,
    agents: Vec<Agent>,
    conflicts: Vec<Conflict>,
    // Path leases agents take out to coordinate ("I own auth/**") so peers get
    // an up-front warning instead of a guaranteed conflict at commit time.
    claims: Vec<Claim>,
}

#[derive(Clone, Debug)]
struct Claim {
    agent: String,
    glob: String,
    created_at: String,
    // Absolute Unix-second expiry; None means the lease never expires until
    // explicitly released.
    expires_at: Option<u64>,
}

impl Claim {
    fn is_expired(&self, now: u64) -> bool {
        self.expires_at.map(|at| now >= at).unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
struct Agent {
    name: String,
    base_tree: String,
    upper_dir: String,
    tombstones_path: String,
    index_path: String,
    view_path: Option<String>,
    conflict: Option<String>,
    created_at: String,
    // Free-form intent the agent advertises to its peers ("refactoring auth,
    // don't touch") — surfaced in status and `context --agent`.
    note: Option<String>,
    // Timestamp of this layer's last write (materialize / sync / commit), used
    // for liveness and stale-layer detection. None until first activity.
    last_active: Option<String>,
    // The step id this layer's base_tree corresponds to, so status can report
    // how many steps the layer is behind the shared head. None means the
    // anchor/base (no step yet).
    base_step: Option<String>,
}

#[derive(Clone, Debug)]
struct Conflict {
    id: String,
    agent: String,
    base_tree: String,
    // The shared head at conflict time (the "theirs" side that already landed).
    shared_tree: String,
    // The agent's clean tree (the "ours" side), recoverable for --ours resolves.
    agent_tree: String,
    // The merge-tree output carrying conflict markers, materialized for manual
    // resolution. Falls back to agent_tree for states written before this field.
    marker_tree: String,
    paths: Vec<String>,
    message: String,
    created_at: String,
}

#[derive(Clone, Debug)]
struct Binding {
    from: String,
    session: String,
    bound_at: String,
}

type Bindings = BTreeMap<String, Binding>;

fn load_state(ctx: &Context) -> Result<State> {
    let path = ctx.state_path();
    let contents = fs::read_to_string(&path).map_err(|_| {
        Error::new(format!(
            "No gstep session found at {}. Run gstep begin <name> first.",
            path.display()
        ))
    })?;
    State::from_json(&contents)
}

fn save_state(ctx: &Context, state: &State) -> Result<()> {
    fs::create_dir_all(&ctx.gstep_dir)?;
    write_atomic(&ctx.state_path(), state.to_json().as_bytes())
}

/// Write `contents` durably: stream into a sibling temp file, then rename over
/// the destination. A crash mid-write leaves the old file intact rather than a
/// truncated one (the rename is atomic on the same filesystem).
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::new("cannot write to a path without a parent directory"))?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().and_then(OsStr::to_str).unwrap_or("file"),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, contents)?;
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(Error::from(error));
    }
    Ok(())
}

/// Time, in seconds, after which an unreleased lock is presumed abandoned by a
/// dead process. Kept generous: gstep operations are short.
const LOCK_STALE_SECS: u64 = 30;

/// Exclusive advisory lock around the whole-file read-modify-write of
/// `state.json`. `load_state`/`save_state` are not atomic together, so two
/// agents committing at once could otherwise clobber each other's step,
/// shared-head, or conflict updates. The lock is a single `O_EXCL`-created
/// lockfile; a lock older than `LOCK_STALE_SECS` is reclaimed so a crashed
/// process cannot wedge the session permanently. Released on drop.
struct StateLock {
    path: PathBuf,
}

impl StateLock {
    fn acquire(ctx: &Context) -> Result<Self> {
        fs::create_dir_all(&ctx.gstep_dir)?;
        let path = ctx.gstep_dir.join("state.lock");
        let mut waited = 0u64;
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid:{} at:{}", std::process::id(), now_secs());
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if waited >= 5_000 {
                        return Err(Error::new(
                            "could not acquire gstep state lock (another gstep is committing); \
                             retry, or remove .git/gstep/state.lock if it is stale",
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    waited += 50;
                }
                Err(error) => return Err(Error::from(error)),
            }
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A lockfile is stale once it is older than `LOCK_STALE_SECS`, which means the
/// process that created it almost certainly died without releasing it.
fn lock_is_stale(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|age| age.as_secs() > LOCK_STALE_SECS)
        .unwrap_or(false)
}

fn load_bindings(ctx: &Context) -> Result<Bindings> {
    let path = ctx.bindings_path();
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let contents = fs::read_to_string(path)?;
    bindings_from_json(&contents)
}

fn save_bindings(ctx: &Context, bindings: &Bindings) -> Result<()> {
    fs::create_dir_all(&ctx.gstep_dir)?;
    write_atomic(&ctx.bindings_path(), bindings_to_json(bindings).as_bytes())
}

fn require_collab(state: &State) -> Result<&Collab> {
    state
        .collab
        .as_ref()
        .ok_or_else(|| Error::new("No gstep agent timeline found. Run gstep begin <name> first."))
}

fn require_collab_mut(state: &mut State) -> Result<&mut Collab> {
    state
        .collab
        .as_mut()
        .ok_or_else(|| Error::new("No gstep agent timeline found. Run gstep begin <name> first."))
}

fn current_agent_name(state: &State) -> Result<Option<String>> {
    let Some(collab) = state.collab.as_ref() else {
        return Ok(None);
    };
    if let Some(name) = env::var("GSTEP_AGENT").ok().filter(|name| !name.is_empty()) {
        if collab.agents.iter().any(|agent| agent.name == name) {
            return Ok(Some(name));
        }
        return Err(Error::new(format!(
            "unknown current agent from GSTEP_AGENT: {name}"
        )));
    }
    let cwd = env::current_dir()?;
    for agent in &collab.agents {
        if let Some(view_path) = &agent.view_path
            && cwd.starts_with(view_path)
        {
            return Ok(Some(agent.name.clone()));
        }
    }
    Ok(None)
}

impl State {
    fn to_json(&self) -> String {
        let current = self
            .current
            .as_ref()
            .map(|value| json_string(value))
            .unwrap_or_else(|| "null".to_string());
        let steps = self
            .steps
            .iter()
            .map(|step| {
                let agent = step
                    .agent
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string());
                let session_id = step
                    .session_id
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    "    {{\"id\": {}, \"parent\": {}, \"message\": {}, \"tree\": {}, \"created_at\": {}, \"agent\": {}, \"session_id\": {}}}",
                    json_string(&step.id),
                    json_string(&step.parent),
                    json_string(&step.message),
                    json_string(&step.tree),
                    json_string(&step.created_at),
                    agent,
                    session_id
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        let branches = self
            .branches
            .iter()
            .map(|branch| {
                format!(
                    "    {{\"name\": {}, \"target\": {}}}",
                    json_string(&branch.name),
                    json_string(&branch.target)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");

        let collab = self
            .collab
            .as_ref()
            .map(Collab::to_json)
            .unwrap_or_else(|| "null".to_string());

        format!(
            "{{\n  \"session\": {},\n  \"anchor\": {},\n  \"current\": {},\n  \"next_step\": {},\n  \"steps\": [\n{}\n  ],\n  \"branches\": [\n{}\n  ],\n  \"collab\": {}\n}}\n",
            json_string(&self.session),
            json_string(&self.anchor),
            current,
            self.next_step,
            steps,
            branches,
            collab
        )
    }

    fn from_json(input: &str) -> Result<Self> {
        let Json::Object(object) = parse_json(input)? else {
            return Err(Error::new("state.json must contain a JSON object"));
        };

        let session = object_string(&object, "session")?;
        let anchor = object_string(&object, "anchor")?;
        let current = object_optional_string(&object, "current")?;
        let next_step = object_number(&object, "next_step")? as usize;
        let steps = object_array(&object, "steps")?
            .iter()
            .map(|value| {
                let Json::Object(step) = value else {
                    return Err(Error::new("step entry must be a JSON object"));
                };
                Ok(Step {
                    id: object_string(step, "id")?,
                    parent: object_string(step, "parent")?,
                    message: object_string(step, "message")?,
                    tree: object_string(step, "tree")?,
                    created_at: object_string(step, "created_at")?,
                    agent: object_optional_string(step, "agent")?,
                    session_id: object_optional_string(step, "session_id")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let branches = object_array(&object, "branches")?
            .iter()
            .map(|value| {
                let Json::Object(branch) = value else {
                    return Err(Error::new("branch entry must be a JSON object"));
                };
                Ok(Branch {
                    name: object_string(branch, "name")?,
                    target: object_string(branch, "target")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let collab = match object.get("collab") {
            Some(Json::Object(collab)) => Some(Collab::from_object(collab)?),
            Some(Json::Null) | None => None,
            Some(_) => return Err(Error::new("collab must be a JSON object or null")),
        };

        Ok(Self {
            session,
            anchor,
            current,
            next_step,
            steps,
            branches,
            collab,
        })
    }
}

impl Collab {
    fn to_json(&self) -> String {
        let agents = self
            .agents
            .iter()
            .map(Agent::to_json)
            .collect::<Vec<_>>()
            .join(",\n");
        let conflicts = self
            .conflicts
            .iter()
            .map(Conflict::to_json)
            .collect::<Vec<_>>()
            .join(",\n");
        let claims = self
            .claims
            .iter()
            .map(Claim::to_json)
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            "{{\"shared_head_tree\": {}, \"next_conflict\": {}, \"agents\": [\n{}\n  ], \"conflicts\": [\n{}\n  ], \"claims\": [\n{}\n  ]}}",
            json_string(&self.shared_head_tree),
            self.next_conflict,
            agents,
            conflicts,
            claims
        )
    }

    fn from_object(object: &BTreeMap<String, Json>) -> Result<Self> {
        let shared_head_tree = object_string(object, "shared_head_tree")?;
        let next_conflict = object_number(object, "next_conflict")? as usize;
        let agents = object_array(object, "agents")?
            .iter()
            .map(|value| {
                let Json::Object(agent) = value else {
                    return Err(Error::new("agent entry must be a JSON object"));
                };
                Agent::from_object(agent)
            })
            .collect::<Result<Vec<_>>>()?;
        let conflicts = object_array(object, "conflicts")?
            .iter()
            .map(|value| {
                let Json::Object(conflict) = value else {
                    return Err(Error::new("conflict entry must be a JSON object"));
                };
                Conflict::from_object(conflict)
            })
            .collect::<Result<Vec<_>>>()?;
        // claims were added after the initial collab schema; tolerate states
        // written before the field existed.
        let claims = match object.get("claims") {
            Some(Json::Array(values)) => values
                .iter()
                .map(|value| {
                    let Json::Object(claim) = value else {
                        return Err(Error::new("claim entry must be a JSON object"));
                    };
                    Claim::from_object(claim)
                })
                .collect::<Result<Vec<_>>>()?,
            Some(Json::Null) | None => Vec::new(),
            Some(_) => return Err(Error::new("claims must be a JSON array")),
        };
        Ok(Self {
            shared_head_tree,
            next_conflict,
            agents,
            conflicts,
            claims,
        })
    }
}

impl Claim {
    fn to_json(&self) -> String {
        let expires_at = self
            .expires_at
            .map(|at| at.to_string())
            .unwrap_or_else(|| "null".to_string());
        format!(
            "    {{\"agent\": {}, \"glob\": {}, \"created_at\": {}, \"expires_at\": {}}}",
            json_string(&self.agent),
            json_string(&self.glob),
            json_string(&self.created_at),
            expires_at
        )
    }

    fn from_object(object: &BTreeMap<String, Json>) -> Result<Self> {
        let expires_at = match object.get("expires_at") {
            Some(Json::Number(value)) => Some(*value as u64),
            Some(Json::Null) | None => None,
            Some(_) => return Err(Error::new("claim expires_at must be a number or null")),
        };
        Ok(Self {
            agent: object_string(object, "agent")?,
            glob: object_string(object, "glob")?,
            created_at: object_string(object, "created_at")?,
            expires_at,
        })
    }
}

impl Agent {
    fn to_json(&self) -> String {
        format!(
            "    {{\"name\": {}, \"base_tree\": {}, \"upper_dir\": {}, \"tombstones_path\": {}, \"index_path\": {}, \"view_path\": {}, \"conflict\": {}, \"created_at\": {}, \"note\": {}, \"last_active\": {}, \"base_step\": {}}}",
            json_string(&self.name),
            json_string(&self.base_tree),
            json_string(&self.upper_dir),
            json_string(&self.tombstones_path),
            json_string(&self.index_path),
            optional_json_string(self.view_path.as_deref()),
            optional_json_string(self.conflict.as_deref()),
            json_string(&self.created_at),
            optional_json_string(self.note.as_deref()),
            optional_json_string(self.last_active.as_deref()),
            optional_json_string(self.base_step.as_deref())
        )
    }

    fn from_object(object: &BTreeMap<String, Json>) -> Result<Self> {
        Ok(Self {
            name: object_string(object, "name")?,
            base_tree: object_string(object, "base_tree")?,
            upper_dir: object_string(object, "upper_dir")?,
            tombstones_path: object_string(object, "tombstones_path")?,
            index_path: object_string(object, "index_path")?,
            view_path: object_optional_string(object, "view_path")?
                .or(object_optional_string(object, "mount_path")?),
            conflict: object_optional_string(object, "conflict")?,
            created_at: object_string(object, "created_at")?,
            note: object_optional_string(object, "note")?,
            last_active: object_optional_string(object, "last_active")?,
            base_step: object_optional_string(object, "base_step")?,
        })
    }
}

impl Conflict {
    fn to_json(&self) -> String {
        let paths = self
            .paths
            .iter()
            .map(|path| json_string(path))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "    {{\"id\": {}, \"agent\": {}, \"base_tree\": {}, \"shared_tree\": {}, \"agent_tree\": {}, \"marker_tree\": {}, \"paths\": [{}], \"message\": {}, \"created_at\": {}}}",
            json_string(&self.id),
            json_string(&self.agent),
            json_string(&self.base_tree),
            json_string(&self.shared_tree),
            json_string(&self.agent_tree),
            json_string(&self.marker_tree),
            paths,
            json_string(&self.message),
            json_string(&self.created_at)
        )
    }

    fn from_object(object: &BTreeMap<String, Json>) -> Result<Self> {
        let paths = object_array(object, "paths")?
            .iter()
            .map(|value| match value {
                Json::String(path) => Ok(path.clone()),
                _ => Err(Error::new("conflict path must be a JSON string")),
            })
            .collect::<Result<Vec<_>>>()?;
        let agent_tree = object_string(object, "agent_tree")?;
        // marker_tree was split out from agent_tree later; older states stored
        // only the marker tree under agent_tree, so fall back to it.
        let marker_tree =
            object_optional_string(object, "marker_tree")?.unwrap_or_else(|| agent_tree.clone());
        Ok(Self {
            id: object_string(object, "id")?,
            agent: object_string(object, "agent")?,
            base_tree: object_string(object, "base_tree")?,
            shared_tree: object_string(object, "shared_tree")?,
            agent_tree,
            marker_tree,
            paths,
            message: object_string(object, "message")?,
            created_at: object_string(object, "created_at")?,
        })
    }
}

fn bindings_to_json(bindings: &Bindings) -> String {
    let entries = bindings
        .iter()
        .map(|(git, binding)| {
            format!(
                "  {}: {{\"from\": {}, \"session\": {}, \"bound_at\": {}}}",
                json_string(git),
                json_string(&binding.from),
                json_string(&binding.session),
                json_string(&binding.bound_at)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("{{\n{}\n}}\n", entries)
}

fn bindings_from_json(input: &str) -> Result<Bindings> {
    let Json::Object(object) = parse_json(input)? else {
        return Err(Error::new("bindings.json must contain a JSON object"));
    };
    let mut bindings = BTreeMap::new();
    for (key, value) in object {
        let Json::Object(binding) = value else {
            return Err(Error::new("binding entry must be a JSON object"));
        };
        bindings.insert(
            key,
            Binding {
                from: object_string(&binding, "from")?,
                session: object_string(&binding, "session")?,
                bound_at: object_string(&binding, "bound_at")?,
            },
        );
    }
    Ok(bindings)
}

fn ensure_shadow_repo(ctx: &Context) -> Result<()> {
    let info_dir = ctx
        .gstep_dir
        .join("shadow.git")
        .join("objects")
        .join("info");
    fs::create_dir_all(&info_dir)?;
    let object_dir = ctx.git_dir.join("objects");
    fs::write(
        info_dir.join("alternates"),
        format!("{}\n", object_dir.display()),
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
struct Resolved {
    selector: String,
    tree: String,
    kind: ResolvedKind,
}

#[derive(Clone, Debug)]
enum ResolvedKind {
    Git { commit: String },
    GstepStep { id: String },
    GstepBase,
    GstepBranch { name: String, target: String },
    Worktree,
}

fn resolve_selector(ctx: &Context, state: &State, selector: &str) -> Result<Resolved> {
    resolve_selector_inner(ctx, state, selector, 0)
}

fn resolve_selector_inner(
    ctx: &Context,
    state: &State,
    selector: &str,
    depth: usize,
) -> Result<Resolved> {
    if depth > 8 {
        return Err(Error::new("selector resolution exceeded recursion limit"));
    }

    if let Some(rev) = selector.strip_prefix("git:") {
        let commit = resolve_git_commit(ctx, rev)?;
        let tree = git_commit_tree(ctx, &commit)?;
        return Ok(Resolved {
            selector: format!("git:{}", short_oid(ctx, &commit)?),
            tree,
            kind: ResolvedKind::Git { commit },
        });
    }

    if selector == "worktree" {
        return Ok(Resolved {
            selector: "worktree".to_string(),
            tree: write_worktree_tree(ctx)?,
            kind: ResolvedKind::Worktree,
        });
    }

    if selector == "gstep:base" {
        return Ok(Resolved {
            selector: "gstep:base".to_string(),
            tree: git_commit_tree(ctx, &state.anchor)?,
            kind: ResolvedKind::GstepBase,
        });
    }

    if selector == "gstep:@" {
        let current = state
            .current
            .as_deref()
            .ok_or_else(|| Error::new("no current gstep step; create one with gstep commit"))?;
        return resolve_selector_inner(ctx, state, current, depth + 1);
    }

    if let Some(name) = selector.strip_prefix("gstep:") {
        if let Some(step) = state.find_step(name) {
            return Ok(Resolved {
                selector: format!("gstep:{}", step.id),
                tree: step.tree.clone(),
                kind: ResolvedKind::GstepStep {
                    id: step.id.clone(),
                },
            });
        }
        if let Some(branch) = state.find_branch(name) {
            let target = resolve_selector_inner(ctx, state, &branch.target, depth + 1)?;
            return Ok(Resolved {
                selector: format!("gstep:{}", branch.name),
                tree: target.tree,
                kind: ResolvedKind::GstepBranch {
                    name: branch.name.clone(),
                    target: branch.target.clone(),
                },
            });
        }
        return Err(Error::new(format!("unknown gstep selector: {selector}")));
    }

    Err(Error::new(format!("unknown selector: {selector}")))
}

fn canonical_selector(ctx: &Context, state: &State, selector: &str) -> Result<String> {
    if selector == "gstep:@" {
        return state
            .current
            .clone()
            .ok_or_else(|| Error::new("gstep:@ is not set yet"));
    }
    if selector == "gstep:base" {
        return Ok(format!("git:{}", state.anchor));
    }
    if let Some(rev) = selector.strip_prefix("git:") {
        return Ok(format!("git:{}", resolve_git_commit(ctx, rev)?));
    }
    if let Some(name) = selector.strip_prefix("gstep:") {
        if state.find_step(name).is_some() {
            return Ok(format!("gstep:{name}"));
        }
        if let Some(branch) = state.find_branch(name) {
            return Ok(branch.target.clone());
        }
    }
    if selector == "worktree" {
        return Err(Error::new(
            "worktree is not a stable selector for this command; create a gstep commit first",
        ));
    }
    Err(Error::new(format!("unknown selector: {selector}")))
}

fn parent_for_new_step(state: &State) -> String {
    match state.current.as_deref() {
        Some(current) => {
            if let Some(name) = current.strip_prefix("gstep:")
                && let Some(branch) = state.find_branch(name)
            {
                return branch.target.clone();
            }
            current.to_string()
        }
        None => format!("git:{}", state.anchor),
    }
}

fn default_agent_view_path(ctx: &Context, state: &State, agent: &str) -> Result<String> {
    Ok(ctx
        .gstep_dir
        .join("views")
        .join(view_path_component(&state.session))
        .join(view_path_component(agent))
        .display()
        .to_string())
}

fn view_path_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn agent_status_json(ctx: &Context, state: &State, agent: &Agent) -> Result<String> {
    let collab = require_collab(state)?;
    let tree = agent_tree(ctx, agent)?;
    Ok(format!(
        "    {{\"name\": {}, \"base_tree\": {}, \"shared_head_tree\": {}, \"view_tree\": {}, \"dirty\": {}, \"behind\": {}, \"view_path\": {}, \"note\": {}, \"last_active\": {}, \"conflict\": {}}}",
        json_string(&agent.name),
        json_string(&agent.base_tree),
        json_string(&collab.shared_head_tree),
        json_string(&tree),
        tree != agent.base_tree,
        agent_behind_by(state, agent),
        optional_json_string(agent.view_path.as_deref()),
        optional_json_string(agent.note.as_deref()),
        optional_json_string(agent.last_active.as_deref()),
        optional_json_string(agent.conflict.as_deref())
    ))
}

/// How many committed steps the agent's base is behind the shared head. Steps
/// are appended in commit order, so this is the count of steps recorded after
/// the one the agent's base points at (all of them, if its base predates the
/// first step).
fn agent_behind_by(state: &State, agent: &Agent) -> usize {
    match &agent.base_step {
        Some(base_step) => {
            match state.steps.iter().position(|step| &step.id == base_step) {
                Some(position) => state.steps.len().saturating_sub(position + 1),
                // base_step no longer exists (e.g. dropped history); treat as
                // up to date rather than guessing.
                None => 0,
            }
        }
        None => state.steps.len(),
    }
}

fn cmd_current_agent_status(ctx: &Context, state: &State, name: &str, json: bool) -> Result<()> {
    let collab = require_collab(state)?;
    let agent = state
        .find_agent(name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?;
    if json {
        println!(
            "{{\n  \"shared_head_tree\": {},\n  \"agent\": {}\n}}",
            json_string(&collab.shared_head_tree),
            agent_status_json(ctx, state, agent)?
        );
        return Ok(());
    }
    let tree = agent_tree(ctx, agent)?;
    let behind = agent_behind_by(state, agent);
    println!("Agent:");
    println!("  name:     {}", agent.name);
    println!("  base:     {}", agent.base_tree);
    println!("  shared:   {}", collab.shared_head_tree);
    println!("  view:     {tree}");
    println!(
        "  dirty:    {}",
        if tree != agent.base_tree { "yes" } else { "no" }
    );
    println!(
        "  behind:   {}",
        if behind == 0 {
            "up to date".to_string()
        } else {
            format!("{behind} step(s)")
        }
    );
    if let Some(note) = &agent.note {
        println!("  note:     {note}");
    }
    println!(
        "  conflict: {}",
        agent.conflict.as_deref().unwrap_or("none")
    );
    Ok(())
}

fn agent_tree(ctx: &Context, agent: &Agent) -> Result<String> {
    fs::create_dir_all(ctx.gstep_dir.join("tmp"))?;
    let index = temp_index_path(ctx);
    let index_ref = index.as_os_str();
    git_env(
        ctx,
        &["read-tree", agent.base_tree.as_str()],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;

    for path in read_tombstones(ctx, agent)? {
        remove_index_path(ctx, index_ref, &path)?;
    }

    let upper_dir = ctx.gstep_dir.join(&agent.upper_dir);
    if upper_dir.exists() {
        for path in list_upper_files(&upper_dir)? {
            add_upper_path_to_index(ctx, index_ref, &upper_dir, &path)?;
        }
    }

    let tree = git_env(ctx, &["write-tree"], &[("GIT_INDEX_FILE", index_ref)])?;
    let _ = fs::remove_file(index);
    Ok(tree.trim().to_string())
}

fn read_tombstones(ctx: &Context, agent: &Agent) -> Result<Vec<String>> {
    let path = ctx.gstep_dir.join(&agent.tombstones_path);
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn list_upper_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_upper_files(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_upper_files(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            collect_upper_files(root, &path, files)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| Error::new(error.to_string()))?;
        files.push(path_to_git_path(relative)?);
    }
    Ok(())
}

fn add_upper_path_to_index(
    ctx: &Context,
    index_ref: &OsStr,
    upper_dir: &Path,
    path: &str,
) -> Result<()> {
    let full_path = upper_dir.join(path);
    let metadata = fs::symlink_metadata(&full_path)?;
    let (mode, oid) = if metadata.file_type().is_symlink() {
        let target = fs::read_link(&full_path)?;
        let bytes = target.to_string_lossy().into_owned();
        ("120000", hash_blob_bytes(ctx, bytes.as_bytes())?)
    } else {
        let mode = executable_mode(&metadata);
        let full_path = full_path
            .to_str()
            .ok_or_else(|| Error::new("upper file path is not valid UTF-8"))?;
        (
            mode,
            git(ctx, &["hash-object", "-w", full_path])?
                .trim()
                .to_string(),
        )
    };
    let cacheinfo = format!("{mode},{oid},{path}");
    git_env(
        ctx,
        &["update-index", "--add", "--cacheinfo", cacheinfo.as_str()],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;
    Ok(())
}

fn executable_mode(metadata: &fs::Metadata) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 != 0 {
            return "100755";
        }
    }
    "100644"
}

fn remove_index_path(ctx: &Context, index_ref: &OsStr, path: &str) -> Result<()> {
    let bytes = git_env_bytes(
        ctx,
        &["ls-files", "-z", "--", path],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;
    let entries = split_nul(&bytes);
    if entries.is_empty() {
        git_env(
            ctx,
            &["update-index", "--force-remove", "--", path],
            &[("GIT_INDEX_FILE", index_ref)],
        )?;
        return Ok(());
    }
    for entry in entries {
        git_env(
            ctx,
            &["update-index", "--force-remove", "--", entry.as_str()],
            &[("GIT_INDEX_FILE", index_ref)],
        )?;
    }
    Ok(())
}

fn path_to_git_path(path: &Path) -> Result<String> {
    let parts = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Error::new("path is not valid UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(parts.join("/"))
}

fn clear_agent_overlay(ctx: &Context, agent: &Agent) -> Result<()> {
    let upper_dir = ctx.gstep_dir.join(&agent.upper_dir);
    if upper_dir.exists() {
        fs::remove_dir_all(&upper_dir)?;
    }
    fs::create_dir_all(&upper_dir)?;
    fs::write(ctx.gstep_dir.join(&agent.tombstones_path), "")?;
    Ok(())
}

// ===== Agent write path: materialize a layer's view, sync edits back =====
//
// An agent edits files inside its `view_path` (a plain materialized worktree).
// `agent_tree` is reconstructed as base + upper − tombstones; the job of sync is
// to turn the agent's worktree edits — including deletions — back into that
// overlay, so a commit captures exactly what the agent did. Materialize is the
// inverse: lay the layer's current tree into the view as a clean working copy.

/// The agent layer's view directory, if one is assigned.
fn agent_view_dir(agent: &Agent) -> Option<PathBuf> {
    agent.view_path.as_ref().map(PathBuf::from)
}

/// Hash a plain directory's contents into a Git tree, reusing the same
/// symlink/exec-bit handling as the overlay index builder.
fn dir_tree(ctx: &Context, dir: &Path) -> Result<String> {
    fs::create_dir_all(ctx.gstep_dir.join("tmp"))?;
    let index = temp_index_path(ctx);
    let index_ref = index.as_os_str();
    git_env(
        ctx,
        &["read-tree", "--empty"],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;
    for path in list_upper_files(dir)? {
        add_upper_path_to_index(ctx, index_ref, dir, &path)?;
    }
    let tree = git_env(ctx, &["write-tree"], &[("GIT_INDEX_FILE", index_ref)])?;
    let _ = fs::remove_file(index);
    Ok(tree.trim().to_string())
}

/// `git diff --name-status` between two trees, as (status_char, path) pairs.
/// Renames are left undetected (no -M), so they surface as a delete + add.
fn diff_name_status(
    ctx: &Context,
    left_tree: &str,
    right_tree: &str,
) -> Result<Vec<(char, String)>> {
    let bytes = git_bytes(ctx, &["diff", "--name-status", "-z", left_tree, right_tree])?;
    let fields = split_nul(&bytes);
    let mut changes = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = &fields[index];
        let code = status.chars().next().unwrap_or('?');
        // Rename/copy entries carry two path fields; everything else carries one.
        let path_count = if matches!(code, 'R' | 'C') { 2 } else { 1 };
        if let Some(path) = fields.get(index + path_count) {
            changes.push((code, path.clone()));
        }
        index += 1 + path_count;
    }
    Ok(changes)
}

/// Copy a single file from the view into the overlay's upper dir, preserving
/// symlinks and the executable bit so the rebuilt tree matches the worktree.
fn copy_into_upper(view: &Path, upper_dir: &Path, path: &str) -> Result<()> {
    let source = view.join(path);
    let dest = upper_dir.join(path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let metadata = fs::symlink_metadata(&source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&source)?;
        let _ = fs::remove_file(&dest);
        symlink_file(&target, &dest)?;
    } else {
        fs::copy(&source, &dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            fs::set_permissions(&dest, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(Error::from)
}

#[cfg(not(unix))]
fn symlink_file(target: &Path, link: &Path) -> Result<()> {
    // Best effort on non-Unix: fall back to copying the link's target bytes.
    fs::write(link, target.to_string_lossy().as_bytes()).map_err(Error::from)
}

/// Reconcile the agent's view worktree into its overlay (upper + tombstones).
/// Returns the base→view changes, or None when the agent has no materialized
/// view (in which case the overlay is left untouched — callers that write the
/// upper dir directly still work). Rebuilds the overlay from scratch so it is
/// idempotent and never leaves stale upper files behind.
fn sync_agent_overlay(ctx: &Context, agent: &Agent) -> Result<Option<Vec<(char, String)>>> {
    let Some(view) = agent_view_dir(agent) else {
        return Ok(None);
    };
    if !view.exists() {
        return Ok(None);
    }
    let view_tree = dir_tree(ctx, &view)?;
    let changes = diff_name_status(ctx, &agent.base_tree, &view_tree)?;
    clear_agent_overlay(ctx, agent)?;
    let upper_dir = ctx.gstep_dir.join(&agent.upper_dir);
    let mut tombstones = Vec::new();
    for (status, path) in &changes {
        if *status == 'D' {
            tombstones.push(path.clone());
        } else {
            copy_into_upper(&view, &upper_dir, path)?;
        }
    }
    let mut body = tombstones.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    fs::write(ctx.gstep_dir.join(&agent.tombstones_path), body)?;
    Ok(Some(changes))
}

/// Thin wrapper used by commit: fold any view edits into the named agent's
/// overlay before the tree to commit is computed.
fn sync_agent_view(ctx: &Context, state: &State, name: &str) -> Result<()> {
    if let Some(agent) = state.find_agent(name).cloned() {
        sync_agent_overlay(ctx, &agent)?;
    }
    Ok(())
}

/// Lay `tree` into `dir`, deleting any files present in `dir` that the tree no
/// longer contains. The generic counterpart of `checkout_tree_to_worktree`,
/// usable for an arbitrary directory (such as an agent view).
fn materialize_tree_clean(ctx: &Context, tree: &str, dir: &Path) -> Result<()> {
    let target_files = tree_files(ctx, tree)?.into_iter().collect::<BTreeSet<_>>();
    if dir.exists() {
        for file in list_upper_files(dir)? {
            if !target_files.contains(&file) {
                let path = dir.join(&file);
                if path.is_file() || path.is_symlink() {
                    fs::remove_file(&path)?;
                    prune_empty_parents(dir, &path)?;
                }
            }
        }
    }
    materialize_tree(ctx, tree, dir)
}

/// Re-lay the agent's current tree into its view, if a view is materialized.
fn rematerialize_view_if_present(ctx: &Context, agent: &Agent) -> Result<()> {
    let Some(view) = agent_view_dir(agent) else {
        return Ok(());
    };
    if !view.exists() {
        return Ok(());
    }
    let tree = agent_tree(ctx, agent)?;
    materialize_tree_clean(ctx, &tree, &view)
}

#[derive(Clone, Debug)]
enum MergeOutcome {
    Clean {
        tree: String,
    },
    Conflicted {
        tree: String,
        paths: Vec<String>,
        message: String,
    },
}

fn merge_agent_tree(
    ctx: &Context,
    base_tree: &str,
    shared_tree: &str,
    agent_tree: &str,
) -> Result<MergeOutcome> {
    let base_commit = commit_tree_for_merge(ctx, base_tree, "base")?;
    let shared_commit = commit_tree_for_merge(ctx, shared_tree, "shared")?;
    let agent_commit = commit_tree_for_merge(ctx, agent_tree, "agent")?;
    let merge_base = format!("--merge-base={base_commit}");
    let output = run_git_raw(
        &ctx.root,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--messages",
            merge_base.as_str(),
            shared_commit.as_str(),
            agent_commit.as_str(),
        ],
        &[],
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0) => Ok(MergeOutcome::Clean {
            tree: first_output_line(&stdout)?,
        }),
        Some(1) => {
            let (tree, paths, message) = parse_merge_conflict_output(&stdout)?;
            Ok(MergeOutcome::Conflicted {
                tree,
                paths,
                message,
            })
        }
        _ => Err(Error::new(format!(
            "git merge-tree failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

fn commit_tree_for_merge(ctx: &Context, tree: &str, label: &str) -> Result<String> {
    let message = format!("gstep merge input {label}");
    let envs = [
        ("GIT_AUTHOR_NAME", OsStr::new("gstep")),
        ("GIT_AUTHOR_EMAIL", OsStr::new("gstep@example.invalid")),
        ("GIT_COMMITTER_NAME", OsStr::new("gstep")),
        ("GIT_COMMITTER_EMAIL", OsStr::new("gstep@example.invalid")),
        ("GIT_CONFIG_GLOBAL", OsStr::new("/dev/null")),
    ];
    Ok(
        git_env(ctx, &["commit-tree", tree, "-m", message.as_str()], &envs)?
            .trim()
            .to_string(),
    )
}

fn first_output_line(output: &str) -> Result<String> {
    output
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::new("git merge-tree did not return a tree"))
}

fn parse_merge_conflict_output(output: &str) -> Result<(String, Vec<String>, String)> {
    let mut lines = output.lines();
    let tree = lines
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .ok_or_else(|| Error::new("git merge-tree conflict output did not include a tree"))?
        .to_string();
    let mut paths = Vec::new();
    let mut messages = Vec::new();
    let mut in_messages = false;
    for line in lines {
        if line.trim().is_empty() {
            in_messages = true;
            continue;
        }
        if in_messages {
            messages.push(line.to_string());
        } else {
            paths.push(line.to_string());
        }
    }
    let message = if messages.is_empty() {
        output.to_string()
    } else {
        messages.join("\n")
    };
    Ok((tree, paths, message))
}

fn write_worktree_tree(ctx: &Context) -> Result<String> {
    fs::create_dir_all(ctx.gstep_dir.join("tmp"))?;
    let index = temp_index_path(ctx);
    let index_ref = index.as_os_str();
    if let Some(head_tree) = git_optional(ctx, &["rev-parse", "--verify", "HEAD^{tree}"])? {
        git_env(
            ctx,
            &["read-tree", head_tree.trim()],
            &[("GIT_INDEX_FILE", index_ref)],
        )?;
    } else {
        git_env(
            ctx,
            &["read-tree", "--empty"],
            &[("GIT_INDEX_FILE", index_ref)],
        )?;
    }
    git_env(
        ctx,
        &["add", "-A", "--", "."],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;
    let tree = git_env(ctx, &["write-tree"], &[("GIT_INDEX_FILE", index_ref)])?;
    let _ = fs::remove_file(index);
    Ok(tree.trim().to_string())
}

fn temp_index_path(ctx: &Context) -> PathBuf {
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    ctx.gstep_dir
        .join("tmp")
        .join(format!("index-{}-{count}", std::process::id()))
}

fn materialize_tree(ctx: &Context, tree: &str, dest: &Path) -> Result<()> {
    fs::create_dir_all(ctx.gstep_dir.join("tmp"))?;
    fs::create_dir_all(dest)?;
    let index = temp_index_path(ctx);
    let index_ref = index.as_os_str();
    git_env(ctx, &["read-tree", tree], &[("GIT_INDEX_FILE", index_ref)])?;
    let prefix = trailing_slash(dest);
    git_env(
        ctx,
        &["checkout-index", "-a", "-f", "--prefix", prefix.as_str()],
        &[("GIT_INDEX_FILE", index_ref)],
    )?;
    let _ = fs::remove_file(index);
    Ok(())
}

fn checkout_tree_to_worktree(ctx: &Context, tree: &str) -> Result<()> {
    let target_files = tree_files(ctx, tree)?.into_iter().collect::<BTreeSet<_>>();
    for file in worktree_files(ctx)? {
        if !target_files.contains(&file) {
            let path = ctx.root.join(&file);
            if path.is_file() || path.is_symlink() {
                fs::remove_file(&path)?;
                prune_empty_parents(&ctx.root, &path)?;
            }
        }
    }
    materialize_tree(ctx, tree, &ctx.root)
}

fn trailing_slash(path: &Path) -> String {
    let mut value = path.display().to_string();
    if !value.ends_with(std::path::MAIN_SEPARATOR) {
        value.push(std::path::MAIN_SEPARATOR);
    }
    value
}

fn prune_empty_parents(root: &Path, path: &Path) -> Result<()> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == root {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(Error::from(error)),
        }
    }
    Ok(())
}

fn tree_files(ctx: &Context, tree: &str) -> Result<Vec<String>> {
    let bytes = git_bytes(ctx, &["ls-tree", "-r", "-z", "--name-only", tree])?;
    Ok(split_nul(&bytes))
}

fn worktree_files(ctx: &Context) -> Result<Vec<String>> {
    let bytes = git_bytes(
        ctx,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    Ok(split_nul(&bytes))
}

fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

fn print_timeline_text(
    ctx: &Context,
    state: &State,
    bindings: &Bindings,
    git_nodes: &[GitNode],
    graph: bool,
) -> Result<()> {
    for node in git_nodes {
        println!(
            "G  {:<10} {}",
            short_oid(ctx, &node.commit)?,
            first_line_or_empty(&node.message)
        );
        if let Some(binding) = bindings.get(&format!("git:{}", node.commit)) {
            println!("   bound-from: {}", binding.from);
        }
        for step in state
            .steps
            .iter()
            .filter(|step| step.parent == format!("git:{}", node.commit))
        {
            print_step_subtree(state, step, graph, 0);
        }
    }

    for step in &state.steps {
        if !step.parent.starts_with("git:")
            && state
                .find_step(step.parent.trim_start_matches("gstep:"))
                .is_none()
        {
            print_step_subtree(state, step, graph, 0);
        }
    }

    Ok(())
}

fn print_step_subtree(state: &State, step: &Step, graph: bool, depth: usize) {
    let indent = if graph {
        format!("{}+- ", "   ".repeat(depth))
    } else {
        "S  ".to_string()
    };
    println!(
        "{}{:<10} {}",
        indent,
        step.id,
        first_line_or_empty(&step.message)
    );
    let selector = format!("gstep:{}", step.id);
    for child in state.steps.iter().filter(|child| child.parent == selector) {
        print_step_subtree(state, child, graph, depth + 1);
    }
}

fn print_timeline_json(
    ctx: &Context,
    state: &State,
    bindings: &Bindings,
    git_nodes: &[GitNode],
) -> Result<()> {
    let mut nodes = Vec::new();
    for node in git_nodes {
        let selector = format!("git:{}", short_oid(ctx, &node.commit)?);
        let binding = bindings.get(&format!("git:{}", node.commit));
        let bound = binding
            .map(|binding| format!(", \"bound_from\": {}", json_string(&binding.from)))
            .unwrap_or_default();
        nodes.push(format!(
            "    {{\"kind\": \"git\", \"id\": {}, \"selector\": {}, \"message\": {}, \"readonly\": true{}}}",
            json_string(&short_oid(ctx, &node.commit)?),
            json_string(&selector),
            json_string(&node.message),
            bound
        ));
    }
    for step in &state.steps {
        nodes.push(format!(
            "    {{\"kind\": \"gstep\", \"id\": {}, \"selector\": {}, \"parent\": {}, \"message\": {}, \"ephemeral\": true}}",
            json_string(&step.id),
            json_string(&format!("gstep:{}", step.id)),
            json_string(&step.parent),
            json_string(&step.message)
        ));
    }
    for branch in &state.branches {
        nodes.push(format!(
            "    {{\"kind\": \"gstep-branch\", \"id\": {}, \"selector\": {}, \"parent\": {}, \"message\": {}, \"ephemeral\": true}}",
            json_string(&branch.name),
            json_string(&format!("gstep:{}", branch.name)),
            json_string(&branch.target),
            json_string("branch")
        ));
    }
    println!("{{\n  \"nodes\": [\n{}\n  ]\n}}", nodes.join(",\n"));
    Ok(())
}

#[derive(Clone, Debug)]
struct GitNode {
    commit: String,
    message: String,
}

fn git_timeline_nodes(ctx: &Context, state: &State) -> Result<Vec<GitNode>> {
    let mut commits = Vec::new();
    commits.push(state.anchor.clone());
    if let Some(head) = head_commit(ctx)? {
        let range = format!("{}..{}", state.anchor, head);
        if let Some(output) = git_optional(ctx, &["rev-list", "--reverse", range.as_str()])? {
            commits.extend(
                output
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned),
            );
        }
    }

    commits
        .into_iter()
        .map(|commit| {
            Ok(GitNode {
                message: git_commit_message(ctx, &commit)?,
                commit,
            })
        })
        .collect()
}

fn print_diff_json(ctx: &Context, left: &Resolved, right: &Resolved) -> Result<()> {
    let bytes = git_bytes(
        ctx,
        &[
            "diff",
            "--name-status",
            "-z",
            left.tree.as_str(),
            right.tree.as_str(),
        ],
    )?;
    let parts = split_nul(&bytes);
    let mut index = 0;
    let mut files = Vec::new();
    while index < parts.len() {
        let status = &parts[index];
        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            if index + 1 > parts.len() {
                break;
            }
            let old_path = parts.get(index).cloned().unwrap_or_default();
            let new_path = parts.get(index + 1).cloned().unwrap_or_default();
            index += 2;
            files.push(format!(
                "    {{\"status\": {}, \"path\": {}, \"new_path\": {}}}",
                json_string(status),
                json_string(&old_path),
                json_string(&new_path)
            ));
        } else {
            let path = parts.get(index).cloned().unwrap_or_default();
            index += 1;
            files.push(format!(
                "    {{\"status\": {}, \"path\": {}}}",
                json_string(status),
                json_string(&path)
            ));
        }
    }

    println!(
        "{{\n  \"from\": {},\n  \"to\": {},\n  \"files\": [\n{}\n  ]\n}}",
        json_string(&left.selector),
        json_string(&right.selector),
        files.join(",\n")
    );
    Ok(())
}

fn relation_to_anchor(ctx: &Context, anchor: &str, head: &str) -> Result<String> {
    if anchor == head {
        return Ok("current is anchor".to_string());
    }
    if git_success(ctx, &["merge-base", "--is-ancestor", anchor, head])? {
        return Ok("current is descendant of anchor".to_string());
    }
    if git_success(ctx, &["merge-base", "--is-ancestor", head, anchor])? {
        return Ok("current is ancestor of anchor".to_string());
    }
    Ok("current is not related to anchor".to_string())
}

fn resolve_git_commit(ctx: &Context, rev: &str) -> Result<String> {
    let spec = format!("{rev}^{{commit}}");
    Ok(git(ctx, &["rev-parse", "--verify", spec.as_str()])?
        .trim()
        .to_string())
}

fn git_commit_tree(ctx: &Context, commit: &str) -> Result<String> {
    let spec = format!("{commit}^{{tree}}");
    Ok(git(ctx, &["rev-parse", spec.as_str()])?.trim().to_string())
}

fn git_commit_message(ctx: &Context, commit: &str) -> Result<String> {
    Ok(git(ctx, &["log", "-1", "--format=%s", commit])?
        .trim()
        .to_string())
}

fn head_commit(ctx: &Context) -> Result<Option<String>> {
    git_optional(ctx, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map(|value| value.map(|value| value.trim().to_string()))
}

fn short_oid(ctx: &Context, oid: &str) -> Result<String> {
    Ok(git(ctx, &["rev-parse", "--short", oid])?.trim().to_string())
}

fn first_line_or_empty(value: &str) -> &str {
    value.lines().next().unwrap_or("")
}

fn current_timestamp() -> String {
    format!("unix:{}", now_secs())
}

/// Seconds since the Unix epoch, saturating to 0 if the clock is before it.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Parse a `unix:<secs>` timestamp string back into seconds.
fn parse_unix_ts(value: &str) -> Option<u64> {
    value.strip_prefix("unix:").and_then(|n| n.parse().ok())
}

/// Render an age in seconds as a terse human string (e.g. `12s`, `4m`, `2h`).
fn format_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

// ===== Cross-agent handoff: agent identity + session context =====
//
// When a code agent (claude, codex) creates a micro step, gstep records which
// agent it was and that agent's session id. A *different* agent can later run
// `gstep context` to locate the original session transcript, parse it (the two
// agents use different on-disk formats), and read a digest of what was being
// done so it can continue the work.

struct AgentIdentity {
    agent: String,
    session_id: Option<String>,
}

/// Determine which code agent is creating a step, and its session id.
/// Priority: explicit flags > Claude env var > active Codex session > none.
/// Never guesses: returns `None` rather than risk attaching the wrong session,
/// because a misattributed session would later feed a future agent the wrong
/// conversation.
fn resolve_commit_identity(ctx: &Context, args: &CommitArgs) -> Option<AgentIdentity> {
    if let Some(agent) = &args.agent {
        return Some(AgentIdentity {
            agent: agent.clone(),
            session_id: args.session_id.clone(),
        });
    }
    // Claude Code exports its session id into the environment, which the gstep
    // CLI/MCP subprocess it spawns inherits. This is authoritative.
    if let Ok(sid) = env::var("CLAUDE_CODE_SESSION_ID")
        && !sid.trim().is_empty()
    {
        return Some(AgentIdentity {
            agent: "claude".to_string(),
            session_id: Some(sid),
        });
    }
    // Codex does not expose its session id to subprocesses, so fall back to
    // detecting the Codex rollout that is actively being written for this
    // working directory.
    if let Some(sid) = detect_active_codex_session(ctx) {
        return Some(AgentIdentity {
            agent: "codex".to_string(),
            session_id: Some(sid),
        });
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn claude_projects_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".claude").join("projects"))
}

fn codex_home() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("CODEX_HOME") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    home_dir().map(|home| home.join(".codex"))
}

/// Recursively collect files under `root` whose file name satisfies `predicate`.
fn find_files(root: &Path, predicate: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            find_files(&path, predicate, out);
        } else if let Some(name) = path.file_name().and_then(|name| name.to_str())
            && predicate(name)
        {
            out.push(path);
        }
    }
}

fn modified_secs_ago(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|duration| duration.as_secs())
}

/// Find the Codex rollout for the current working directory that was written
/// most recently, but only if it was touched recently enough to plausibly be
/// the session running right now. Returns its session id.
///
/// Known limitation: a plain-shell `gstep commit` (no agent running) made in a
/// directory where Codex was active within the window will still be attributed
/// to that Codex session. The attached session is real and for the same cwd, so
/// the harm is low, but it can over-attribute. Reached only when no Claude
/// session id is present in the environment.
fn detect_active_codex_session(ctx: &Context) -> Option<String> {
    const ACTIVE_WINDOW_SECS: u64 = 180;
    let sessions = codex_home()?.join("sessions");
    let mut files = Vec::new();
    find_files(
        &sessions,
        &|name| name.starts_with("rollout-") && name.ends_with(".jsonl"),
        &mut files,
    );
    let mut best: Option<(u64, String)> = None;
    for file in files {
        let Some(age) = modified_secs_ago(&file) else {
            continue;
        };
        if age > ACTIVE_WINDOW_SECS {
            continue;
        }
        let Some((id, cwd)) = codex_rollout_meta(&file) else {
            continue;
        };
        if !same_path(&cwd, &ctx.root) {
            continue;
        }
        if best
            .as_ref()
            .map(|(best_age, _)| age < *best_age)
            .unwrap_or(true)
        {
            best = Some((age, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Compare a path string (possibly Windows-style) against a `Path` loosely:
/// normalize separators, drop trailing slashes, and lowercase, so that minor
/// representation differences do not cause a miss.
fn same_path(a: &str, b: &Path) -> bool {
    fn norm(value: &str) -> String {
        value
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    }
    norm(a) == norm(&b.display().to_string())
}

/// Read the leading `session_meta` record of a Codex rollout, returning
/// `(session_id, cwd)`.
fn codex_rollout_meta(path: &Path) -> Option<(String, String)> {
    let file = fs::File::open(path).ok()?;
    let reader = io::BufReader::new(file);
    for line in reader.lines().take(5).map_while(|line| line.ok()) {
        let Ok(Json::Object(object)) = parse_json(&line) else {
            continue;
        };
        if object_string(&object, "type").ok().as_deref() != Some("session_meta") {
            continue;
        }
        let Some(Json::Object(payload)) = object.get("payload") else {
            continue;
        };
        let id = object_string(payload, "id").ok()?;
        let cwd = object_string(payload, "cwd").unwrap_or_default();
        return Some((id, cwd));
    }
    None
}

/// Locate the transcript file on disk for a recorded `(agent, session_id)`.
/// Resolved at read time (not stored) because session files get archived/moved.
fn locate_transcript(agent: &str, session_id: &str) -> Option<PathBuf> {
    match agent {
        "claude" => {
            // The per-project directory name is derived from a munged cwd and is
            // unstable, so search every project dir for the unique session file.
            let projects = claude_projects_dir()?;
            let target = format!("{session_id}.jsonl");
            let mut files = Vec::new();
            find_files(&projects, &|name| name == target, &mut files);
            files.into_iter().next()
        }
        "codex" => {
            let home = codex_home()?;
            let needle = format!("-{session_id}.jsonl");
            let mut files = Vec::new();
            for sub in ["sessions", "archived_sessions"] {
                find_files(
                    &home.join(sub),
                    &|name| name.starts_with("rollout-") && name.ends_with(needle.as_str()),
                    &mut files,
                );
            }
            files.into_iter().next()
        }
        _ => None,
    }
}

#[derive(Clone)]
struct Turn {
    role: String,
    text: String,
}

/// Parse an agent transcript into an ordered list of user/assistant turns,
/// normalizing the two on-disk formats into one shape.
fn parse_transcript(agent: &str, path: &Path) -> Result<Vec<Turn>> {
    let file = fs::File::open(path).map_err(|error| {
        Error::new(format!(
            "cannot open transcript {}: {error}",
            path.display()
        ))
    })?;
    let reader = io::BufReader::new(file);
    let mut turns = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(Json::Object(object)) = parse_json(&line) else {
            continue;
        };
        match agent {
            "claude" => parse_claude_line(&object, &mut turns),
            "codex" => parse_codex_line(&object, &mut turns),
            _ => {}
        }
    }
    Ok(turns)
}

fn parse_claude_line(object: &BTreeMap<String, Json>, turns: &mut Vec<Turn>) {
    let kind = match object.get("type") {
        Some(Json::String(value)) => value.as_str(),
        _ => return,
    };
    if kind != "user" && kind != "assistant" {
        return;
    }
    let Some(Json::Object(message)) = object.get("message") else {
        return;
    };
    let role = match message.get("role") {
        Some(Json::String(value)) => value.clone(),
        _ => kind.to_string(),
    };
    let text = match message.get("content") {
        Some(Json::String(value)) => value.clone(),
        Some(Json::Array(blocks)) => extract_text_blocks(blocks),
        _ => String::new(),
    };
    let text = text.trim().to_string();
    if !text.is_empty() {
        turns.push(Turn { role, text });
    }
}

/// Pull just the human-readable text out of a Claude content-block array,
/// dropping thinking, tool calls, and tool results to keep the digest legible.
fn extract_text_blocks(blocks: &[Json]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        if let Json::Object(block) = block
            && let Some(Json::String(kind)) = block.get("type")
            && kind == "text"
            && let Some(Json::String(text)) = block.get("text")
        {
            parts.push(text.clone());
        }
    }
    parts.join("\n")
}

fn parse_codex_line(object: &BTreeMap<String, Json>, turns: &mut Vec<Turn>) {
    if object_string(object, "type").ok().as_deref() != Some("event_msg") {
        return;
    }
    let Some(Json::Object(payload)) = object.get("payload") else {
        return;
    };
    let role = match payload.get("type") {
        Some(Json::String(kind)) if kind == "user_message" => "user",
        Some(Json::String(kind)) if kind == "agent_message" => "assistant",
        _ => return,
    };
    if let Some(Json::String(message)) = payload.get("message") {
        let text = message.trim().to_string();
        if !text.is_empty() {
            turns.push(Turn {
                role: role.to_string(),
                text,
            });
        }
    }
}

const DIGEST_MAX_TURNS: usize = 12;
const DIGEST_MAX_TURN_CHARS: usize = 1200;
const DIGEST_MAX_TOTAL_CHARS: usize = 8000;

/// Build a bounded, handoff-oriented digest: keep the very first user turn (the
/// original task — what "continue" most needs) plus the most recent turns,
/// under a total size cap.
fn build_digest(turns: &[Turn]) -> Vec<Turn> {
    if turns.is_empty() {
        return Vec::new();
    }
    let window_start = turns.len().saturating_sub(DIGEST_MAX_TURNS);
    // The original task, only if it falls outside the recent window (otherwise
    // it is already included below).
    let head = turns
        .iter()
        .position(|turn| turn.role == "user")
        .filter(|index| *index < window_start)
        .map(|index| truncate_turn(&turns[index]));
    let head_len = head.as_ref().map(|turn| turn.text.len()).unwrap_or(0);

    let mut budget = DIGEST_MAX_TOTAL_CHARS.saturating_sub(head_len);
    let mut recent = Vec::new();
    for turn in turns[window_start..].iter().rev() {
        let turn = truncate_turn(turn);
        if turn.text.len() > budget && !recent.is_empty() {
            break;
        }
        budget = budget.saturating_sub(turn.text.len());
        recent.push(turn);
    }
    recent.reverse();

    let mut digest = Vec::new();
    if let Some(head) = head {
        digest.push(head);
    }
    digest.extend(recent);
    digest
}

fn truncate_turn(turn: &Turn) -> Turn {
    let text = if turn.text.chars().count() > DIGEST_MAX_TURN_CHARS {
        let truncated: String = turn.text.chars().take(DIGEST_MAX_TURN_CHARS).collect();
        format!("{truncated}…[truncated]")
    } else {
        turn.text.clone()
    };
    Turn {
        role: turn.role.clone(),
        text,
    }
}

/// `gstep context --agent <name>` — read a live, uncommitted agent layer: its
/// advertised intent (note), liveness, dirty status, the paths it has changed
/// vs the shared head, and any open conflict. This is the real-time coordination
/// channel for agents working at the same time, as opposed to the step-scoped
/// transcript handoff.
fn cmd_agent_context(name: &str, json: bool) -> Result<()> {
    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let collab = require_collab(&state)?;
    let agent = state
        .find_agent(name)
        .ok_or_else(|| Error::new(format!("unknown agent: {name}")))?;
    let tree = agent_tree(&ctx, agent)?;
    let dirty = tree != agent.base_tree;
    let changes = if dirty {
        diff_name_status(&ctx, &collab.shared_head_tree, &tree)?
    } else {
        Vec::new()
    };
    let behind = agent_behind_by(&state, agent);

    if json {
        let changed = changes
            .iter()
            .map(|(status, path)| {
                format!(
                    "{{\"status\": {}, \"path\": {}}}",
                    json_string(&status.to_string()),
                    json_string(path)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{{\n  \"agent\": {},\n  \"note\": {},\n  \"dirty\": {},\n  \"behind\": {},\n  \"last_active\": {},\n  \"conflict\": {},\n  \"changes\": [{}]\n}}",
            json_string(name),
            optional_json_string(agent.note.as_deref()),
            dirty,
            behind,
            optional_json_string(agent.last_active.as_deref()),
            optional_json_string(agent.conflict.as_deref()),
            changed
        );
        return Ok(());
    }

    println!("Context for agent {name}");
    println!("  note:     {}", agent.note.as_deref().unwrap_or("(none)"));
    println!("  dirty:    {}", if dirty { "yes" } else { "no" });
    println!(
        "  behind:   {}",
        if behind == 0 {
            "up to date".to_string()
        } else {
            format!("{behind} step(s)")
        }
    );
    let now = now_secs();
    println!(
        "  active:   {}",
        agent
            .last_active
            .as_deref()
            .and_then(parse_unix_ts)
            .map(|ts| format!("{} ago", format_age(now.saturating_sub(ts))))
            .unwrap_or_else(|| "never".to_string())
    );
    println!(
        "  conflict: {}",
        agent.conflict.as_deref().unwrap_or("none")
    );
    if !changes.is_empty() {
        println!("  changes vs shared:");
        for (status, path) in changes {
            println!("    {status} {path}");
        }
    }
    Ok(())
}

fn cmd_context(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut selector = None;
    let mut agent = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--agent" => {
                index += 1;
                agent = Some(
                    args.get(index)
                        .ok_or_else(|| Error::new("--agent requires an agent name"))?
                        .clone(),
                );
            }
            other if selector.is_none() && !other.starts_with('-') => {
                selector = Some(other.to_string())
            }
            other => {
                return Err(Error::new(format!("unexpected context argument: {other}")));
            }
        }
        index += 1;
    }

    // `context --agent <name>` reads a *live, uncommitted* layer's intent and
    // working state, rather than a committed step's session transcript.
    if let Some(agent) = agent {
        return cmd_agent_context(&agent, json);
    }
    let selector = selector.unwrap_or_else(|| "gstep:@".to_string());

    let ctx = Context::discover()?;
    let state = load_state(&ctx)?;
    let resolved = resolve_selector(&ctx, &state, &selector)?;
    let step = match &resolved.kind {
        ResolvedKind::GstepStep { id } => state.find_step(id),
        _ => None,
    };
    let step = step.ok_or_else(|| {
        Error::new(format!(
            "context is only available for gstep micro steps; {} is not a step",
            resolved.selector
        ))
    })?;

    let step_selector = format!("gstep:{}", step.id);
    let agent = step.agent.clone();
    let session_id = step.session_id.clone();

    let (agent, session_id) = match (agent, session_id) {
        (Some(agent), Some(session_id)) => (agent, session_id),
        _ => {
            if json {
                println!(
                    "{{\n  \"step\": {},\n  \"agent\": null,\n  \"session_id\": null,\n  \"transcript\": null,\n  \"turns\": []\n}}",
                    json_string(&step_selector)
                );
            } else {
                println!("{step_selector} has no recorded agent/session context.");
            }
            return Ok(());
        }
    };

    let transcript = locate_transcript(&agent, &session_id);
    let turns = match &transcript {
        Some(path) => parse_transcript(&agent, path)?,
        None => Vec::new(),
    };
    let digest = build_digest(&turns);

    if json {
        let turns_json = digest
            .iter()
            .map(|turn| {
                format!(
                    "{{\"role\": {}, \"text\": {}}}",
                    json_string(&turn.role),
                    json_string(&turn.text)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let transcript_json = transcript
            .as_ref()
            .map(|path| json_string(&path.display().to_string()))
            .unwrap_or_else(|| "null".to_string());
        println!(
            "{{\n  \"step\": {},\n  \"agent\": {},\n  \"session_id\": {},\n  \"transcript\": {},\n  \"turns\": [{}]\n}}",
            json_string(&step_selector),
            json_string(&agent),
            json_string(&session_id),
            transcript_json,
            turns_json
        );
        return Ok(());
    }

    println!("Context for {step_selector}");
    println!("  agent:      {agent}");
    println!("  session:    {session_id}");
    match &transcript {
        Some(path) => println!("  transcript: {}", path.display()),
        None => println!("  transcript: not found locally (session may be on another machine)"),
    }
    println!("  message:    {}", step.message);
    println!();
    if digest.is_empty() {
        println!("(no readable conversation turns recovered)");
    } else {
        println!("--- recovered context ({} turns) ---", digest.len());
        for turn in &digest {
            println!("[{}]", turn.role);
            println!("{}", turn.text);
            println!();
        }
    }
    Ok(())
}

fn git_at(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = run_git(cwd, args, &[])?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn git(ctx: &Context, args: &[&str]) -> Result<String> {
    let output = run_git(&ctx.root, args, &[])?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_bytes(ctx: &Context, args: &[&str]) -> Result<Vec<u8>> {
    Ok(run_git(&ctx.root, args, &[])?.stdout)
}

fn git_env(ctx: &Context, args: &[&str], envs: &[(&str, &OsStr)]) -> Result<String> {
    let output = run_git(&ctx.root, args, envs)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_env_bytes(ctx: &Context, args: &[&str], envs: &[(&str, &OsStr)]) -> Result<Vec<u8>> {
    Ok(run_git(&ctx.root, args, envs)?.stdout)
}

fn hash_blob_bytes(ctx: &Context, bytes: &[u8]) -> Result<String> {
    let output = run_git_with_input(&ctx.root, &["hash-object", "-w", "--stdin"], &[], bytes)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_optional(ctx: &Context, args: &[&str]) -> Result<Option<String>> {
    let output = run_git_raw(&ctx.root, args, &[])?;
    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn git_success(ctx: &Context, args: &[&str]) -> Result<bool> {
    Ok(run_git_raw(&ctx.root, args, &[])?.status.success())
}

fn run_git(cwd: &Path, args: &[&str], envs: &[(&str, &OsStr)]) -> Result<Output> {
    let output = run_git_raw(cwd, args, envs)?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::new(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        )))
    }
}

fn run_git_with_input(
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &OsStr)],
    input: &[u8],
) -> Result<Output> {
    let output = run_git_raw_with_input(cwd, args, envs, Some(input))?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::new(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        )))
    }
}

fn run_git_raw(cwd: &Path, args: &[&str], envs: &[(&str, &OsStr)]) -> Result<Output> {
    run_git_raw_with_input(cwd, args, envs, None)
}

fn run_git_raw_with_input(
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &OsStr)],
    input: Option<&[u8]>,
) -> Result<Output> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd);
    for arg in args {
        command.arg(arg);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    if input.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(input) = input {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::new("failed to open git stdin"))?;
        stdin.write_all(input)?;
    }
    Ok(child.wait_with_output()?)
}

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

fn parse_json(input: &str) -> Result<Json> {
    let mut parser = JsonParser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(Error::new("unexpected trailing JSON input"));
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn parse_value(&mut self) -> Result<Json> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => {
                self.expect_literal(b"null")?;
                Ok(Json::Null)
            }
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok(Json::Bool(false))
            }
            Some(b'"') => Ok(Json::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(Error::new("invalid JSON value")),
        }
    }

    fn parse_array(&mut self) -> Result<Json> {
        self.consume(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.try_consume(b']') {
                break;
            }
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.try_consume(b']') {
                break;
            }
            self.consume(b',')?;
        }
        Ok(Json::Array(values))
    }

    fn parse_object(&mut self) -> Result<Json> {
        self.consume(b'{')?;
        let mut values = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.try_consume(b'}') {
                break;
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.consume(b':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_ws();
            if self.try_consume(b'}') {
                break;
            }
            self.consume(b',')?;
        }
        Ok(Json::Object(values))
    }

    fn parse_string(&mut self) -> Result<String> {
        self.consume(b'"')?;
        let mut output = Vec::new();
        loop {
            let Some(byte) = self.next() else {
                return Err(Error::new("unterminated JSON string"));
            };
            match byte {
                b'"' => break,
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| Error::new("unterminated JSON escape"))?;
                    match escaped {
                        b'"' | b'\\' | b'/' => output.push(escaped),
                        b'b' => output.push(8),
                        b'f' => output.push(12),
                        b'n' => output.push(b'\n'),
                        b'r' => output.push(b'\r'),
                        b't' => output.push(b'\t'),
                        b'u' => {
                            for _ in 0..4 {
                                self.next()
                                    .ok_or_else(|| Error::new("unterminated unicode escape"))?;
                            }
                            output.push(b'?');
                        }
                        _ => return Err(Error::new("invalid JSON escape")),
                    }
                }
                other => output.push(other),
            }
        }
        String::from_utf8(output).map_err(|error| Error::new(error.to_string()))
    }

    fn parse_number(&mut self) -> Result<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let number = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|error| Error::new(error.to_string()))?
            .parse::<i64>()
            .map_err(|error| Error::new(error.to_string()))?;
        Ok(Json::Number(number))
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<()> {
        if self.bytes.get(self.pos..self.pos + literal.len()) == Some(literal) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(Error::new("invalid JSON literal"))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> Result<()> {
        match self.next() {
            Some(byte) if byte == expected => Ok(()),
            _ => Err(Error::new(format!(
                "expected JSON byte '{}'",
                expected as char
            ))),
        }
    }

    fn try_consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.pos += 1;
        Some(value)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

fn object_string(object: &BTreeMap<String, Json>, key: &str) -> Result<String> {
    match object.get(key) {
        Some(Json::String(value)) => Ok(value.clone()),
        _ => Err(Error::new(format!("missing or invalid JSON string: {key}"))),
    }
}

fn object_optional_string(object: &BTreeMap<String, Json>, key: &str) -> Result<Option<String>> {
    match object.get(key) {
        Some(Json::String(value)) => Ok(Some(value.clone())),
        Some(Json::Null) | None => Ok(None),
        _ => Err(Error::new(format!("invalid optional JSON string: {key}"))),
    }
}

fn object_number(object: &BTreeMap<String, Json>, key: &str) -> Result<i64> {
    match object.get(key) {
        Some(Json::Number(value)) => Ok(*value),
        _ => Err(Error::new(format!("missing or invalid JSON number: {key}"))),
    }
}

fn object_array<'a>(object: &'a BTreeMap<String, Json>, key: &str) -> Result<&'a [Json]> {
    match object.get(key) {
        Some(Json::Array(value)) => Ok(value),
        _ => Err(Error::new(format!("missing or invalid JSON array: {key}"))),
    }
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn optional_json_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn json_value(value: &Json) -> String {
    match value {
        Json::Null => "null".to_string(),
        Json::Bool(value) => value.to_string(),
        Json::Number(value) => value.to_string(),
        Json::String(value) => json_string(value),
        Json::Array(values) => format!(
            "[{}]",
            values.iter().map(json_value).collect::<Vec<_>>().join(",")
        ),
        Json::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", json_string(key), json_value(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if other.is_control() => escaped.push('?'),
            other => escaped.push(other),
        }
    }
    escaped
}
