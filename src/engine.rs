//! The loop engine: composes per-iteration prompts, drives `claude -p`, parses
//! the `LOOP_STATUS` contract, and applies the termination rules.

use std::io::ErrorKind;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::cli::Config;
use crate::harness::render_harness;
use crate::status::{parse_status, Status};

/// Terminal result of a completed loop, mapped to a process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopOutcome {
    /// Goal reported DONE (and verified, if a verify command was supplied).
    Done,
    /// Loop stopped because the model reported BLOCKED.
    Blocked,
    /// Hit `--max` without reaching DONE.
    MaxReached,
}

impl LoopOutcome {
    /// Map the outcome to its process exit code.
    ///
    /// `0` = DONE, `2` = BLOCKED, `3` = reached `--max` without DONE.
    /// (`1` is reserved for internal/setup errors, handled in `main`.)
    pub fn exit_code(self) -> i32 {
        match self {
            LoopOutcome::Done => 0,
            LoopOutcome::Blocked => 2,
            LoopOutcome::MaxReached => 3,
        }
    }
}

/// Shape of `claude -p --output-format json` we care about.
#[derive(Deserialize)]
struct ClaudeJson {
    result: Option<String>,
}

/// Compose the per-iteration prompt from the harness, running state, and the
/// current iteration counter.
fn compose_prompt(harness: &str, state: &str, iter: u32, max: u32) -> String {
    format!(
        "{harness}\n\n## Running state (prior iterations)\n{state}\n\n## This is iteration {iter} of {max}. Do ONE increment, then emit the LOOP_STATUS line."
    )
}

/// Pull the assistant result text out of claude's stdout. Prefers the `result`
/// field of the JSON envelope; falls back to the raw stdout when the output is
/// not the expected JSON.
fn extract_result_text(stdout: &str, verbose: bool) -> String {
    match serde_json::from_str::<ClaudeJson>(stdout) {
        Ok(ClaudeJson { result: Some(text) }) => text,
        Ok(ClaudeJson { result: None }) => {
            if verbose {
                eprintln!("warning: claude JSON had no `result` field; using raw stdout");
            }
            stdout.trim().to_string()
        }
        Err(_) => {
            if verbose {
                eprintln!("warning: claude output was not valid JSON; using raw stdout");
            }
            stdout.trim().to_string()
        }
    }
}

/// Append a trimmed summary of an iteration's result to the running state,
/// capping the total length at `cap` characters (keeping the tail when over).
fn append_state(state: &mut String, iter: u32, result_text: &str, cap: usize) {
    let summary = result_text.trim();
    if !state.is_empty() {
        state.push_str("\n\n");
    }
    state.push_str(&format!("[iteration {iter}] {summary}"));

    if state.chars().count() > cap {
        let tail: String = state.chars().skip(state.chars().count() - cap).collect();
        *state = format!("…(truncated)…{tail}");
    }
}

/// Run a verify command via `sh -c`. Returns the exit code (or `None` if the
/// process was terminated by a signal).
fn run_verify(cmd: &str) -> Result<Option<i32>> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .with_context(|| format!("failed to run verify command: {cmd}"))?;
    Ok(status.code())
}

/// Longest slice of `--until` output carried into the next iteration.
const UNTIL_FEEDBACK_CHARS: usize = 2000;

/// Outcome of one `--until` check.
struct CheckRun {
    /// Exit code, or `None` when terminated by a signal.
    code: Option<i32>,
    /// Combined stdout + stderr.
    output: String,
}

/// Run the `--until` command via `sh -c`, capturing combined output.
fn run_until(cmd: &str) -> Result<CheckRun> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .with_context(|| format!("failed to run until command: {cmd}"))?;
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(CheckRun {
        code: out.status.code(),
        output,
    })
}

/// Describe an exit code for notes and feedback.
fn describe_exit(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("exit {c}"),
        None => "terminated by signal".to_string(),
    }
}

/// Format a failing `--until` check as feedback for the next iteration,
/// keeping only the tail of long output (where failures usually are).
fn until_feedback(cmd: &str, code: Option<i32>, output: &str) -> String {
    let trimmed = output.trim();
    let count = trimmed.chars().count();
    let body = if trimmed.is_empty() {
        "(no output)".to_string()
    } else if count > UNTIL_FEEDBACK_CHARS {
        let tail: String = trimmed.chars().skip(count - UNTIL_FEEDBACK_CHARS).collect();
        format!("…(truncated)…{tail}")
    } else {
        trimmed.to_string()
    };
    format!(
        "until check `{cmd}` still failing ({}). Output:\n{body}",
        describe_exit(code)
    )
}

/// Invoke claude once and return its captured stdout.
fn invoke_claude(cfg: &Config, prompt: &str) -> Result<String> {
    let mut command = Command::new(&cfg.claude_bin);
    command
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("json");
    if let Some(model) = &cfg.model {
        command.arg("--model").arg(model);
    }

    if cfg.verbose {
        eprintln!(
            "+ {} -p <prompt {} chars> --output-format json{}",
            cfg.claude_bin,
            prompt.chars().count(),
            cfg.model
                .as_ref()
                .map(|m| format!(" --model {m}"))
                .unwrap_or_default()
        );
    }

    let output = command.output().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            anyhow!(
                "could not find the `{}` binary on PATH. Install Claude Code, or pass --claude-bin <PATH>.",
                cfg.claude_bin
            )
        } else {
            anyhow!("failed to run `{}`: {e}", cfg.claude_bin)
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!(
            "warning: {} exited with {} on this iteration",
            cfg.claude_bin, output.status
        );
        if cfg.verbose && !stderr.trim().is_empty() {
            eprintln!("--- claude stderr ---\n{}", stderr.trim());
        }
    }

    Ok(stdout)
}

/// Drive the full loop to a terminal [`LoopOutcome`].
pub fn run(cfg: &Config) -> Result<LoopOutcome> {
    let harness = render_harness(cfg);
    let mut state = String::from("(none — first iteration).");

    let mut outcome = LoopOutcome::MaxReached;

    for iter in 1..=cfg.max {
        let prompt = compose_prompt(&harness, &state, iter, cfg.max);
        let stdout = invoke_claude(cfg, &prompt)?;
        let result_text = extract_result_text(&stdout, cfg.verbose);

        if cfg.verbose {
            for line in result_text
                .lines()
                .filter(|l| l.to_ascii_uppercase().contains("LOOP_STATUS"))
            {
                eprintln!("raw status: {}", line.trim());
            }
        }

        let parsed = parse_status(&result_text);
        let (mut status, mut note) = match parsed {
            Some(ls) => (ls.status, ls.note),
            None => {
                eprintln!(
                    "warning: no LOOP_STATUS line found on iteration {iter}; treating as CONTINUE"
                );
                (Status::Continue, "(no status line emitted)".to_string())
            }
        };

        // `--until` is the authority on completion: when it exits 0 the
        // iteration counts as DONE regardless of the model's status; while it
        // fails, DONE is downgraded and its output is fed into the next
        // iteration. BLOCKED always wins so a stuck model can still surface
        // its question. Runs before the verify gate, so `--verify` (if also
        // set) must still pass before the loop ends.
        let mut feedback = None;
        if status != Status::Blocked {
            if let Some(cmd) = &cfg.until {
                let check = run_until(cmd)?;
                if cfg.verbose {
                    eprintln!("until `{cmd}`: {}", describe_exit(check.code));
                }
                if check.code == Some(0) {
                    if status != Status::Done {
                        note = format!("until check passed — {note}");
                    }
                    status = Status::Done;
                } else {
                    if status == Status::Done {
                        status = Status::Continue;
                        note = format!("until check failing ({})", describe_exit(check.code));
                    }
                    feedback = Some(until_feedback(cmd, check.code, &check.output));
                }
            }
        }

        // A DONE claim must clear the verify gate, if one was provided.
        if status == Status::Done {
            if let Some(cmd) = &cfg.verify {
                match run_verify(cmd)? {
                    Some(0) => {}
                    Some(code) => {
                        status = Status::Continue;
                        note = format!("verify failed (exit {code})");
                    }
                    None => {
                        status = Status::Continue;
                        note = "verify failed (terminated by signal)".to_string();
                    }
                }
            }
        }

        println!("[iter {iter}/{}] {status} — {note}", cfg.max);

        let carried = match &feedback {
            Some(f) => format!("{}\n\n{f}", result_text.trim()),
            None => result_text,
        };
        append_state(&mut state, iter, &carried, cfg.max_state_chars);

        match status {
            Status::Done => {
                outcome = LoopOutcome::Done;
                break;
            }
            Status::Blocked => {
                outcome = LoopOutcome::Blocked;
                break;
            }
            Status::Continue => {
                if iter == cfg.max {
                    outcome = LoopOutcome::MaxReached;
                }
            }
        }
    }

    let summary = match outcome {
        LoopOutcome::Done if cfg.until.is_some() => "✓ loop complete: DONE (until check passed).",
        LoopOutcome::Done => "✓ loop complete: goal reported DONE.",
        LoopOutcome::Blocked => "■ loop stopped: BLOCKED — a decision or input is needed.",
        LoopOutcome::MaxReached => {
            "✗ loop ended: reached --max without DONE (partial state above)."
        }
    };
    println!("{summary}");

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_mapping() {
        assert_eq!(LoopOutcome::Done.exit_code(), 0);
        assert_eq!(LoopOutcome::Blocked.exit_code(), 2);
        assert_eq!(LoopOutcome::MaxReached.exit_code(), 3);
    }

    #[test]
    fn extract_result_from_json() {
        let stdout = r#"{"result": "hello world", "type": "result"}"#;
        assert_eq!(extract_result_text(stdout, false), "hello world");
    }

    #[test]
    fn extract_result_falls_back_to_raw() {
        let stdout = "not json at all";
        assert_eq!(extract_result_text(stdout, false), "not json at all");
    }

    #[test]
    fn extract_result_missing_field_falls_back() {
        let stdout = r#"{"type": "result"}"#;
        assert_eq!(extract_result_text(stdout, false), stdout.trim());
    }

    #[test]
    fn compose_prompt_includes_parts() {
        let p = compose_prompt("HARNESS", "STATE", 2, 8);
        assert!(p.starts_with("HARNESS"));
        assert!(p.contains("## Running state (prior iterations)\nSTATE"));
        assert!(p.contains("This is iteration 2 of 8."));
    }

    #[test]
    fn append_state_grows_and_marks_iteration() {
        let mut state = String::new();
        append_state(&mut state, 1, "did a thing", 4000);
        assert!(state.contains("[iteration 1] did a thing"));
        append_state(&mut state, 2, "did another", 4000);
        assert!(state.contains("[iteration 2] did another"));
    }

    #[test]
    fn append_state_caps_to_tail() {
        let mut state = String::new();
        let big = "x".repeat(100);
        append_state(&mut state, 1, &big, 20);
        assert!(state.chars().count() <= 20 + "…(truncated)…".chars().count());
        assert!(state.starts_with("…(truncated)…"));
        assert!(state.ends_with('x'));
    }

    #[test]
    fn until_feedback_includes_command_exit_and_output() {
        let f = until_feedback("cargo test", Some(101), "  test foo ... FAILED\n");
        assert!(f.contains("`cargo test`"));
        assert!(f.contains("exit 101"));
        assert!(f.ends_with("test foo ... FAILED"));
    }

    #[test]
    fn until_feedback_keeps_tail_of_long_output() {
        let long = format!("{}END", "a".repeat(UNTIL_FEEDBACK_CHARS * 2));
        let f = until_feedback("make", Some(2), &long);
        assert!(f.contains("…(truncated)…"));
        assert!(f.ends_with("END"));
        assert!(f.chars().count() < UNTIL_FEEDBACK_CHARS + 200);
    }

    #[test]
    fn until_feedback_handles_empty_and_signal() {
        let f = until_feedback("false", None, "   ");
        assert!(f.contains("terminated by signal"));
        assert!(f.contains("(no output)"));
    }

    #[test]
    fn run_until_captures_stdout_and_stderr() {
        let r = run_until("echo out; echo err >&2; exit 4").unwrap();
        assert_eq!(r.code, Some(4));
        assert!(r.output.contains("out"));
        assert!(r.output.contains("err"));
        assert_eq!(run_until("true").unwrap().code, Some(0));
    }
}
