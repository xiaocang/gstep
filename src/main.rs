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

    match args[0].as_str() {
        "begin" => cmd_begin(&args[1..]),
        "fork" => cmd_fork(&args[1..]),
        "status" => cmd_status(&args[1..]),
        "timeline" => cmd_timeline(&args[1..]),
        "log" => cmd_log(&args[1..]),
        "show" => cmd_show(&args[1..]),
        "context" => cmd_context(&args[1..]),
        "diff" => cmd_diff(&args[1..]),
        "commit" => cmd_commit(&args[1..]),
        "branch" => cmd_branch(&args[1..]),
        "checkout" => cmd_checkout(&args[1..]),
        "revert" => cmd_revert(&args[1..]),
        "materialize" => cmd_materialize(&args[1..]),
        "bind" => cmd_bind(&args[1..]),
        "mcp" => cmd_mcp(&args[1..]),
        "close" => cmd_close(&args[1..]),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
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
  gstep bind git:<rev> --from gstep:<step> [--git-notes]\n\
  gstep mcp\n\
  gstep close --prune\n\
\n\
Selectors: git:<rev>, gstep:@, gstep:base, gstep:<step-or-branch>, worktree"
    );
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
    let mut state = load_state(&ctx)?;
    if state.find_agent(&name).is_some() {
        return Err(Error::new(format!("agent already exists: {name}")));
    }
    let base_tree = match from {
        Some(selector) => resolve_selector(&ctx, &state, &selector)?.tree,
        None => require_collab(&state)?.shared_head_tree.clone(),
    };
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
    });
    save_state(&ctx, &state)?;

    let agent = state.find_agent(&name).expect("agent was just created");
    println!("Created gstep agent {name}");
    println!("base tree: {}", agent.base_tree);
    println!("view path: {}", agent.view_path.as_deref().unwrap_or(""));
    Ok(())
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
            .map(|agent| agent_status_json(&ctx, collab, agent))
            .collect::<Result<Vec<_>>>()?
            .join(",\n");
        println!(
            "{{\n  \"shared_head_tree\": {},\n  \"agents\": [\n{}\n  ]\n}}",
            json_string(&collab.shared_head_tree),
            entries
        );
        return Ok(());
    }

    println!("Shared head tree: {}", collab.shared_head_tree);
    for agent in agents {
        let tree = agent_tree(&ctx, agent)?;
        println!();
        println!("Agent: {}", agent.name);
        println!("  base:     {}", agent.base_tree);
        println!("  view:     {tree}");
        println!(
            "  dirty:    {}",
            if tree != agent.base_tree { "yes" } else { "no" }
        );
        println!(
            "  view path:{}",
            agent.view_path.as_deref().unwrap_or("not assigned")
        );
        println!(
            "  conflict: {}",
            agent.conflict.as_deref().unwrap_or("none")
        );
    }
    Ok(())
}

fn commit_agent_changes(
    ctx: &Context,
    state: &mut State,
    name: &str,
    message: String,
) -> Result<()> {
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
            });
            state.current = Some(format!("gstep:{id}"));
            if let Some(collab) = state.collab.as_mut() {
                collab.shared_head_tree = tree.clone();
                collab.conflicts.retain(|conflict| conflict.agent != name);
            }
            if let Some(agent_mut) = state.find_agent_mut(name) {
                agent_mut.base_tree = tree.clone();
                agent_mut.conflict = None;
                clear_agent_overlay(ctx, agent_mut)?;
            }
            save_state(ctx, state)?;
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
                    agent_tree: tree,
                    paths,
                    message: message.clone(),
                    created_at: current_timestamp(),
                });
                id
            };
            if let Some(agent_mut) = state.find_agent_mut(name) {
                agent_mut.conflict = Some(conflict_id.clone());
            }
            save_state(ctx, state)?;
            return Err(Error::new(format!(
                "agent {name} has merge conflicts ({conflict_id})\n{message}"
            )));
        }
    }
    Ok(())
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
                println!("(run `gstep context gstep:{}` to recover its session)", step.id);
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
    match &identity {
        Some(i) => match &i.session_id {
            Some(sid) => println!("agent: {} session: {}", i.agent, sid),
            None => println!("agent: {} (no session id detected)", i.agent),
        },
        None => {}
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
                ("agent", "string", "Optional code agent name override, e.g. claude or codex."),
                ("session", "string", "Optional session id override for the committing agent."),
            ],
            &["message"],
            false,
            false,
        ),
        mcp_tool(
            "gstep_context",
            "Recover the originating agent's session context for a gstep micro step, so a different agent can read what was being done and continue. Returns the agent, session id, transcript path, and a digest of conversation turns.",
            &[("selector", "string", "Step selector (default gstep:@).")],
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
        "gstep_bind" => {
            invocation.args.push("bind".to_string());
            invocation.args.push(required_arg(arguments, "git")?);
            invocation.args.push("--from".to_string());
            invocation.args.push(required_arg(arguments, "from")?);
            if optional_bool_arg(arguments, "git_notes")?.unwrap_or(false) {
                invocation.args.push("--git-notes".to_string());
            }
        }
        _ => return Err(Error::new(format!("unknown tool: {name}"))),
    }
    Ok(invocation)
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
}

#[derive(Clone, Debug)]
struct Conflict {
    id: String,
    agent: String,
    base_tree: String,
    shared_tree: String,
    agent_tree: String,
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
    fs::write(ctx.state_path(), state.to_json())?;
    Ok(())
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
    fs::write(ctx.bindings_path(), bindings_to_json(bindings))?;
    Ok(())
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
        format!(
            "{{\"shared_head_tree\": {}, \"next_conflict\": {}, \"agents\": [\n{}\n  ], \"conflicts\": [\n{}\n  ]}}",
            json_string(&self.shared_head_tree),
            self.next_conflict,
            agents,
            conflicts
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
        Ok(Self {
            shared_head_tree,
            next_conflict,
            agents,
            conflicts,
        })
    }
}

impl Agent {
    fn to_json(&self) -> String {
        format!(
            "    {{\"name\": {}, \"base_tree\": {}, \"upper_dir\": {}, \"tombstones_path\": {}, \"index_path\": {}, \"view_path\": {}, \"conflict\": {}, \"created_at\": {}}}",
            json_string(&self.name),
            json_string(&self.base_tree),
            json_string(&self.upper_dir),
            json_string(&self.tombstones_path),
            json_string(&self.index_path),
            optional_json_string(self.view_path.as_deref()),
            optional_json_string(self.conflict.as_deref()),
            json_string(&self.created_at)
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
            "    {{\"id\": {}, \"agent\": {}, \"base_tree\": {}, \"shared_tree\": {}, \"agent_tree\": {}, \"paths\": [{}], \"message\": {}, \"created_at\": {}}}",
            json_string(&self.id),
            json_string(&self.agent),
            json_string(&self.base_tree),
            json_string(&self.shared_tree),
            json_string(&self.agent_tree),
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
        Ok(Self {
            id: object_string(object, "id")?,
            agent: object_string(object, "agent")?,
            base_tree: object_string(object, "base_tree")?,
            shared_tree: object_string(object, "shared_tree")?,
            agent_tree: object_string(object, "agent_tree")?,
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

fn agent_status_json(ctx: &Context, collab: &Collab, agent: &Agent) -> Result<String> {
    let tree = agent_tree(ctx, agent)?;
    Ok(format!(
        "    {{\"name\": {}, \"base_tree\": {}, \"shared_head_tree\": {}, \"view_tree\": {}, \"dirty\": {}, \"view_path\": {}, \"conflict\": {}}}",
        json_string(&agent.name),
        json_string(&agent.base_tree),
        json_string(&collab.shared_head_tree),
        json_string(&tree),
        tree != agent.base_tree,
        optional_json_string(agent.view_path.as_deref()),
        optional_json_string(agent.conflict.as_deref())
    ))
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
            agent_status_json(ctx, collab, agent)?
        );
        return Ok(());
    }
    let tree = agent_tree(ctx, agent)?;
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
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
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
    if let Ok(sid) = env::var("CLAUDE_CODE_SESSION_ID") {
        if !sid.trim().is_empty() {
            return Some(AgentIdentity {
                agent: "claude".to_string(),
                session_id: Some(sid),
            });
        }
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
        } else if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if predicate(name) {
                out.push(path);
            }
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
        value.replace('\\', "/").trim_end_matches('/').to_lowercase()
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
    let file = fs::File::open(path)
        .map_err(|error| Error::new(format!("cannot open transcript {}: {error}", path.display())))?;
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
        if let Json::Object(block) = block {
            if let Some(Json::String(kind)) = block.get("type") {
                if kind == "text" {
                    if let Some(Json::String(text)) = block.get("text") {
                        parts.push(text.clone());
                    }
                }
            }
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

fn cmd_context(args: &[String]) -> Result<()> {
    let mut json = false;
    let mut selector = None;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other if selector.is_none() => selector = Some(other.to_string()),
            other => {
                return Err(Error::new(format!("unexpected context argument: {other}")));
            }
        }
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
