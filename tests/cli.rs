//! End-to-end tests for the named-loop store and the `--until` stop mode,
//! driving the real `loopgen` binary against a stub `claude` script
//! (passed via `--claude-bin`) and an isolated store (`LOOPGEN_DIR`).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// A scratch workspace: a temp dir holding the stub claude, the store, and
/// any files the stub or check commands touch.
struct Env {
    dir: TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Write an executable stub `claude` whose body is `script` (sh). The
    /// stub bumps `calls` on every invocation and saves the prompt (`$2`)
    /// to `prompt.<n>`, so tests can assert on call counts and prompts.
    fn stub_claude(&self, script: &str) -> PathBuf {
        let path = self.path("claude-stub");
        let body = format!(
            "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nn=$(( $(cat calls 2>/dev/null || echo 0) + 1 ))\necho $n > calls\nprintf '%s' \"$2\" > prompt.$n\n{script}\n"
        );
        fs::write(&path, body).expect("write stub");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        path
    }

    fn calls(&self) -> u32 {
        fs::read_to_string(self.path("calls"))
            .map(|s| s.trim().parse().unwrap_or(0))
            .unwrap_or(0)
    }

    fn loopgen(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_loopgen"))
            .args(args)
            .current_dir(self.dir.path())
            .env("LOOPGEN_DIR", self.path("store"))
            .env("NO_COLOR", "1")
            .output()
            .expect("run loopgen")
    }
}

fn status_json(status: &str) -> String {
    format!("printf '{{\"result\":\"LOOP_STATUS: {status} | iter n/m | stub\"}}'")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

// ── --until (command-check stop mode) ───────────────────────────────────

#[test]
fn until_stops_as_soon_as_check_passes() {
    let env = Env::new();
    // The model never claims DONE; it "fixes" things on the 2nd call.
    let stub = env.stub_claude(&format!(
        "[ \"$n\" -ge 2 ] && touch fixed\n{}",
        status_json("CONTINUE")
    ));
    let out = env.loopgen(&[
        "make it pass",
        "--until",
        "test -f fixed",
        "--max",
        "5",
        "--claude-bin",
        s(&stub),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(env.calls(), 2);
    let so = stdout(&out);
    assert!(so.contains("[iter 1/5] CONTINUE"), "{so}");
    assert!(so.contains("[iter 2/5] DONE — until check passed"), "{so}");
    assert!(so.contains("DONE (until check passed)"), "{so}");
}

#[test]
fn until_downgrades_premature_done() {
    let env = Env::new();
    let stub = env.stub_claude(&status_json("DONE"));
    let out = env.loopgen(&[
        "make it pass",
        "--until",
        "false",
        "--max",
        "2",
        "--claude-bin",
        s(&stub),
    ]);
    assert_eq!(out.status.code(), Some(3), "stdout: {}", stdout(&out));
    assert_eq!(env.calls(), 2);
    assert!(stdout(&out).contains("CONTINUE — until check failing (exit 1)"));
}

#[test]
fn until_failure_output_is_fed_into_next_prompt() {
    let env = Env::new();
    let stub = env.stub_claude(&status_json("CONTINUE"));
    let out = env.loopgen(&[
        "make it pass",
        "--until",
        "printf 'FAIL_%s' TOKEN42; exit 7",
        "--max",
        "2",
        "--claude-bin",
        s(&stub),
    ]);
    assert_eq!(out.status.code(), Some(3));
    let first = fs::read_to_string(env.path("prompt.1")).unwrap();
    let second = fs::read_to_string(env.path("prompt.2")).unwrap();
    assert!(!first.contains("FAIL_TOKEN42"));
    assert!(second.contains("FAIL_TOKEN42"), "{second}");
    assert!(second.contains("exit 7"));
    // The harness tells the model about the check.
    assert!(first.contains("The runner also checks `printf 'FAIL_%s' TOKEN42; exit 7`"));
}

#[test]
fn blocked_wins_over_passing_until() {
    let env = Env::new();
    let stub = env.stub_claude(&status_json("BLOCKED"));
    let out = env.loopgen(&["make it pass", "--until", "true", "--claude-bin", s(&stub)]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(env.calls(), 1);
}

#[test]
fn verify_still_gates_when_until_passes() {
    let env = Env::new();
    let stub = env.stub_claude(&status_json("CONTINUE"));
    let out = env.loopgen(&[
        "make it pass",
        "--until",
        "true",
        "--verify",
        "false",
        "--max",
        "2",
        "--claude-bin",
        s(&stub),
    ]);
    assert_eq!(out.status.code(), Some(3), "stdout: {}", stdout(&out));
    assert!(stdout(&out).contains("verify failed (exit 1)"));
}

// ── Named-loop store ────────────────────────────────────────────────────

#[test]
fn save_as_list_show_run_remove_roundtrip() {
    let env = Env::new();
    let stub = env.stub_claude(&format!(
        "[ \"$n\" -ge 3 ] && touch fixed\n{}",
        status_json("CONTINUE")
    ));

    // Save (does not run claude).
    let out = env.loopgen(&[
        "get the tests green",
        "--until",
        "test -f fixed",
        "--max",
        "6",
        "--claude-bin",
        s(&stub),
        "--save-as",
        "fix-tests",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("saved loop 'fix-tests'"));
    assert_eq!(env.calls(), 0);
    assert!(env.path("store/loops/fix-tests.toml").is_file());

    // List.
    let out = env.loopgen(&["--list"]);
    assert_eq!(out.status.code(), Some(0));
    let so = stdout(&out);
    assert!(so.contains("fix-tests"), "{so}");
    assert!(so.contains("get the tests green"), "{so}");

    // Show.
    let out = env.loopgen(&["--show", "fix-tests"]);
    assert_eq!(out.status.code(), Some(0));
    let so = stdout(&out);
    assert!(so.contains("goal = \"get the tests green\""), "{so}");
    assert!(so.contains("until = \"test -f fixed\""), "{so}");
    assert!(so.contains("max = 6"), "{so}");

    // Dry-run by name renders the stored goal.
    let out = env.loopgen(&["--run", "fix-tests", "--dry-run"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("## Goal\nget the tests green"));
    assert_eq!(env.calls(), 0);

    // Real run by name uses the stored until + claude_bin.
    let out = env.loopgen(&["--run", "fix-tests"]);
    assert_eq!(out.status.code(), Some(0), "stdout: {}", stdout(&out));
    assert_eq!(env.calls(), 3);
    assert!(stdout(&out).contains("[iter 3/6] DONE"));

    // Remove.
    let out = env.loopgen(&["--remove", "fix-tests"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!env.path("store/loops/fix-tests.toml").exists());
    let out = env.loopgen(&["--list"]);
    assert!(stdout(&out).contains("no saved loops"));
}

#[test]
fn cli_flags_override_named_loop() {
    let env = Env::new();
    let stub = env.stub_claude(&status_json("CONTINUE"));
    let out = env.loopgen(&[
        "stubborn goal",
        "--max",
        "6",
        "--claude-bin",
        s(&stub),
        "--save-as",
        "stubborn",
    ]);
    assert_eq!(out.status.code(), Some(0));

    let out = env.loopgen(&["--run", "stubborn", "--max", "2"]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(env.calls(), 2);
    assert!(stdout(&out).contains("[iter 2/2]"));
}

#[test]
fn run_missing_loop_fails_cleanly() {
    let env = Env::new();
    let out = env.loopgen(&["--run", "ghost"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("no loop named 'ghost'"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn save_as_rejects_path_like_names() {
    let env = Env::new();
    let out = env.loopgen(&["goal", "--save-as", "../escape"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("invalid loop name"));
    assert!(!env.path("store/escape.toml").exists());
    assert!(!env.path("escape.toml").exists());
}

#[test]
fn manage_flags_cannot_combine_with_goal() {
    let env = Env::new();
    let out = env.loopgen(&["goal", "--list"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("run on their own"));
}

#[test]
fn export_bash_from_named_loop_includes_until() {
    let env = Env::new();
    let out = env.loopgen(&["exported goal", "--until", "cargo test", "--save-as", "exp"]);
    assert_eq!(out.status.code(), Some(0));
    let out = env.loopgen(&["--run", "exp", "--export-bash"]);
    assert_eq!(out.status.code(), Some(0));
    let so = stdout(&out);
    assert!(so.starts_with("#!/usr/bin/env bash"));
    assert!(so.contains("UNTIL='cargo test'"));
    assert!(so.contains("exported goal"));
}
