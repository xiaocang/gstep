use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_gstep")
}

#[test]
fn commit_timeline_status_and_bind_keep_git_history_clean() {
    let repo = TestRepo::new("commit-timeline");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base commit"]);

    repo.gstep(&["begin", "refactor"]);
    let alternates = fs::read_to_string(
        repo.path
            .join(".git/gstep/shadow.git/objects/info/alternates"),
    )
    .unwrap();
    assert!(alternates.trim_end().ends_with(".git/objects"));
    repo.write("app.txt", "micro\n");
    repo.write("notes.txt", "scratch\n");
    repo.gstep(&["commit", "-m", "micro one"]);

    let status = repo.gstep(&["status", "--json"]);
    assert!(status.contains("\"session\": \"refactor\""));
    assert!(status.contains("\"current_step\": \"gstep:step-1\""));
    assert!(status.contains("\"dirty\": false"));

    let timeline = repo.gstep(&["timeline", "--json"]);
    assert!(timeline.contains("\"kind\": \"git\""));
    assert!(timeline.contains("\"kind\": \"gstep\""));
    assert!(timeline.contains("\"message\": \"base commit\""));
    assert!(timeline.contains("\"message\": \"micro one\""));

    repo.gstep(&["bind", "git:HEAD", "--from", "gstep:step-1"]);
    let bound_status = repo.gstep(&["status", "--json"]);
    assert!(bound_status.contains("\"bound_to_git_commit\": \"gstep:step-1\""));
    repo.gstep(&["bind", "git:HEAD", "--from", "gstep:step-1", "--git-notes"]);
    let note = repo.git(&["notes", "--ref", "refs/notes/gstep", "show", "HEAD"]);
    assert!(note.contains("gstep.from=gstep:step-1"));

    let git_log = repo.git(&["log", "--oneline"]);
    assert_eq!(git_log.lines().count(), 1);
}

#[test]
fn selectors_diff_materialize_and_checkout_do_not_move_git_head() {
    let repo = TestRepo::new("selectors");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let head_before = repo.git(&["rev-parse", "HEAD"]);

    repo.gstep(&["begin", "try-change"]);
    repo.write("app.txt", "micro\n");
    repo.gstep(&["commit", "-m", "change app"]);

    let diff = repo.gstep(&["diff", "git:HEAD", "gstep:@"]);
    assert!(diff.contains("-base"));
    assert!(diff.contains("+micro"));

    let out = repo.path.join("materialized");
    repo.gstep(&["materialize", "gstep:@", out.to_str().unwrap()]);
    assert_eq!(fs::read_to_string(out.join("app.txt")).unwrap(), "micro\n");

    let refused = repo.gstep_fail(&["checkout", "git:HEAD"]);
    assert!(refused.contains("Refusing to move Git HEAD"));

    repo.gstep(&["checkout", "--as-worktree", "git:HEAD"]);
    assert_eq!(repo.read("app.txt"), "base\n");
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head_before);

    repo.gstep(&["checkout", "gstep:step-1"]);
    assert_eq!(repo.read("app.txt"), "micro\n");
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head_before);

    repo.write("app.txt", "dirty\n");
    let dirty = repo.gstep(&["diff", "gstep:@", "worktree", "--json"]);
    assert!(dirty.contains("\"status\": \"M\""));
    assert!(dirty.contains("\"path\": \"app.txt\""));
}

#[test]
fn branch_show_log_and_revert_cover_micro_step_navigation() {
    let repo = TestRepo::new("branch-revert");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    repo.gstep(&["begin", "variants", "--anchor", "git:HEAD"]);
    repo.gstep(&["branch", "alt", "--from", "git:HEAD"]);
    repo.gstep(&["checkout", "gstep:alt"]);
    repo.write("app.txt", "alt\n");
    repo.gstep(&["commit", "-m", "alt change"]);

    let show = repo.gstep(&["show", "gstep:step-1"]);
    assert!(show.contains("Gstep micro step gstep:step-1"));
    assert!(show.contains("message alt change"));
    assert!(show.contains("app.txt"));

    let log = repo.gstep(&["log", "--include-git"]);
    assert!(log.contains("base"));
    assert!(log.contains("alt change"));

    repo.gstep(&["revert", "gstep:step-1"]);
    assert_eq!(repo.read("app.txt"), "base\n");

    repo.gstep(&["close", "--prune"]);
    assert!(!repo.path.join(".git/gstep").exists());
}

#[test]
fn git_to_git_selectors_work_from_historical_anchor() {
    let repo = TestRepo::new("git-selectors");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    repo.write("app.txt", "second\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "second"]);

    repo.gstep(&["begin", "from-history", "--anchor", "git:HEAD~1"]);

    let show = repo.gstep(&["show", "git:HEAD"]);
    assert!(show.contains("Git macro step git:"));
    assert!(show.contains("message second"));

    let diff = repo.gstep(&["diff", "git:HEAD~1", "git:HEAD", "--json"]);
    assert!(diff.contains("\"status\": \"M\""));
    assert!(diff.contains("\"path\": \"app.txt\""));

    let timeline = repo.gstep(&["timeline", "--graph", "--include-git"]);
    assert!(timeline.contains("base"));
    assert!(timeline.contains("second"));

    let out = repo.path.join("old-tree");
    repo.gstep(&["materialize", "git:HEAD~1", out.to_str().unwrap()]);
    assert_eq!(fs::read_to_string(out.join("app.txt")).unwrap(), "base\n");
}

#[test]
fn mcp_server_lists_and_calls_project_tools() {
    let repo = TestRepo::new("mcp");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    let output = repo.gstep_mcp(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"gstep_begin","arguments":{"name":"mcp-session"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"gstep_status","arguments":{}}}
"#,
    );

    assert!(output.contains("\"protocolVersion\":\"2025-11-25\""));
    assert!(output.contains("\"name\":\"gstep_status\""));
    assert!(output.contains("\"name\":\"gstep_fork\""));
    assert!(output.contains("\"isError\":false"));
    assert!(output.contains("mcp-session"));
}

#[test]
fn commit_records_agent_identity_and_context_reads_it_back() {
    let repo = TestRepo::new("handoff");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    repo.gstep(&["begin", "handoff"]);
    repo.write("app.txt", "work\n");
    // Explicit agent/session is the cross-agent contract; auto-detection is
    // environment-dependent so the test pins it deterministically.
    let commit = repo.gstep(&[
        "commit",
        "-m",
        "checkpoint",
        "--agent",
        "codex",
        "--session",
        "sess-abc-123",
    ]);
    assert!(commit.contains("agent: codex session: sess-abc-123"));

    // The identity is surfaced on show.
    let show = repo.gstep(&["show", "gstep:@"]);
    assert!(show.contains("agent codex"));
    assert!(show.contains("session sess-abc-123"));

    // A different agent reads the recorded identity back via context. The
    // transcript for a synthetic session does not exist on disk, so the digest
    // is empty, but the provenance (agent + session id) round-trips.
    let context = repo.gstep(&["context", "gstep:@", "--json"]);
    assert!(context.contains("\"agent\": \"codex\""));
    assert!(context.contains("\"session_id\": \"sess-abc-123\""));
    assert!(context.contains("\"transcript\": null"));

    // Steps committed without identity report no recorded context rather than
    // erroring (backward compatible with pre-feature states).
    repo.write("app.txt", "more\n");
    repo.gstep(&["commit", "-m", "anon"]);
    let anon = repo.gstep(&["context", "gstep:@"]);
    assert!(anon.contains("no recorded agent/session context"));
}

#[test]
fn commit_auto_detects_active_codex_session() {
    let repo = TestRepo::new("codex-detect");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    repo.gstep(&["begin", "codex-detect"]);

    // The working-directory string gstep will compute for this repo. On
    // git-for-windows this comes back with forward slashes.
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    // Real Codex records cwd with native separators; on Windows that is
    // backslashes, which must reconcile against git's forward-slash toplevel.
    let codex_cwd = if cfg!(windows) {
        root.replace('/', "\\")
    } else {
        root.clone()
    };

    // A synthetic CODEX_HOME containing one freshly written rollout for this
    // cwd — enough to exercise the whole detection chain (walk, recency gate,
    // session_meta parse, path match).
    let codex_home = repo.path.join("fake-codex");
    let sessions = codex_home
        .join("sessions")
        .join("2026")
        .join("06")
        .join("09");
    fs::create_dir_all(&sessions).unwrap();
    let id = "019eaa12-test-7080-9361-abcdefabcdef";
    let meta = format!(
        "{{\"timestamp\":\"2026-06-09T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"{}\"}}}}\n",
        codex_cwd.replace('\\', "\\\\")
    );
    fs::write(
        sessions.join(format!("rollout-2026-06-09T00-00-00-{id}.jsonl")),
        meta,
    )
    .unwrap();

    repo.write("app.txt", "codex work\n");
    let commit = repo.gstep_with_codex_home(&["commit", "-m", "codex auto"], &codex_home);
    assert!(
        commit.contains(&format!("agent: codex session: {id}")),
        "expected codex auto-detection, got:\n{commit}"
    );
}

#[test]
fn agent_timeline_uses_native_commit_for_current_agent() {
    let repo = TestRepo::new("agent-native");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    repo.gstep(&["begin", "team"]);
    repo.gstep(&["fork", "alpha"]);
    repo.write(".git/gstep/agents/alpha/upper/app.txt", "alpha\n");

    let commit = repo.gstep_agent("alpha", &["commit", "-m", "alpha change"]);
    assert!(commit.contains("Committed agent alpha as gstep:step-1"));

    let status = repo.gstep_agent("alpha", &["status", "--json"]);
    assert!(status.contains("\"dirty\": false"));
    assert!(status.contains("\"name\": \"alpha\""));

    let out = repo.path.join("merged");
    repo.gstep(&["materialize", "gstep:@", out.to_str().unwrap()]);
    assert_eq!(fs::read_to_string(out.join("app.txt")).unwrap(), "alpha\n");

    let no_agent_command = repo.gstep_fail(&["agent", "status"]);
    assert!(no_agent_command.contains("unknown command: agent"));
}

#[test]
fn agent_timeline_reports_conflicts_at_commit_time() {
    let repo = TestRepo::new("agent-conflict");
    repo.write("app.txt", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    repo.gstep(&["begin", "team"]);
    repo.gstep(&["fork", "alpha"]);
    repo.gstep(&["fork", "beta"]);

    repo.write(".git/gstep/agents/alpha/upper/app.txt", "alpha\n");
    repo.gstep_agent("alpha", &["commit", "-m", "alpha change"]);

    repo.write(".git/gstep/agents/beta/upper/app.txt", "beta\n");
    let conflict = repo.gstep_agent_fail("beta", &["commit", "-m", "beta change"]);
    assert!(conflict.contains("agent beta has merge conflicts"));
    assert!(conflict.contains("app.txt"));

    let status = repo.gstep(&["status", "--all", "--json"]);
    assert!(status.contains("\"conflict\": \"conflict-1\""));
}

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new(name: &str) -> Self {
        let unique = format!(
            "gstep-test-{}-{}-{}",
            name,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        let repo = Self { path };
        repo.git(&["init"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "Test User"]);
        repo
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.path.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.path.join(path)).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert_success("git", args, &output);
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string()
    }

    fn gstep(&self, args: &[&str]) -> String {
        self.gstep_with_env(args, &[])
    }

    fn gstep_agent(&self, agent: &str, args: &[&str]) -> String {
        self.gstep_with_env(args, &[("GSTEP_AGENT", agent)])
    }

    /// Build a gstep command with the host's code-agent environment scrubbed,
    /// so auto-detection of the committing agent stays deterministic no matter
    /// what agent (if any) is running the test suite.
    fn gstep_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(bin());
        command
            .current_dir(&self.path)
            .args(args)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CODEX_HOME");
        command
    }

    /// Run gstep with the Claude env scrubbed and `CODEX_HOME` pointed at a
    /// synthetic Codex home, to exercise Codex session auto-detection.
    fn gstep_with_codex_home(&self, args: &[&str], codex_home: &PathBuf) -> String {
        let output = self
            .gstep_command(args)
            .env("CODEX_HOME", codex_home)
            .output()
            .unwrap();
        assert_success("gstep", args, &output);
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn gstep_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> String {
        let mut command = self.gstep_command(args);
        for (key, value) in envs {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert_success("gstep", args, &output);
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn gstep_fail(&self, args: &[&str]) -> String {
        self.gstep_fail_with_env(args, &[])
    }

    fn gstep_agent_fail(&self, agent: &str, args: &[&str]) -> String {
        self.gstep_fail_with_env(args, &[("GSTEP_AGENT", agent)])
    }

    fn gstep_fail_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> String {
        let mut command = self.gstep_command(args);
        for (key, value) in envs {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert!(
            !output.status.success(),
            "expected gstep {:?} to fail, stdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    fn gstep_mcp(&self, input: &str) -> String {
        let mut child = Command::new(bin())
            .current_dir(&self.path)
            .arg("mcp")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CODEX_HOME")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(input.as_bytes()).unwrap();
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        assert_success("gstep mcp", &[], &output);
        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        if should_keep_temp_repos() {
            return;
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_success(command: &str, args: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "{} {:?} failed\nstdout:\n{}\nstderr:\n{}",
        command,
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn should_keep_temp_repos() -> bool {
    std::env::var_os("GSTEP_KEEP_TEST_REPOS").is_some()
        || SystemTime::now().duration_since(UNIX_EPOCH).is_err()
}
