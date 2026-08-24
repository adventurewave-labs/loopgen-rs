//! Renders the agentic-loop harness prompt from a [`Config`].

use crate::cli::Config;

/// Derive a short slug from the goal: lowercase, non-alphanumerics collapsed to
/// `-`, first ~6 words. Falls back to `loop` when nothing usable remains.
pub fn slugify(goal: &str) -> String {
    let words: Vec<String> = goal
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .map(|w| w.to_ascii_lowercase())
        .collect();

    if words.is_empty() {
        "loop".to_string()
    } else {
        words.join("-")
    }
}

/// Resolve the Definition of Done: explicit `--dod` wins, otherwise auto-derive
/// based on whether a `--verify` command is present.
pub fn definition_of_done(cfg: &Config) -> String {
    if let Some(dod) = &cfg.dod {
        return dod.clone();
    }
    match &cfg.verify {
        Some(cmd) => format!(
            "The stated goal is achieved and the verify command (`{cmd}`) exits 0."
        ),
        None => "The stated goal is fully achieved, with concrete evidence cited for each success criterion."
            .to_string(),
    }
}

/// Render the full harness prompt. When `--verify` is absent, the two
/// `Run:` / `It MUST exit 0` lines are omitted entirely.
pub fn render_harness(cfg: &Config) -> String {
    let goal = cfg.goal.as_deref().unwrap_or("");
    let slug = slugify(goal);
    let dod = definition_of_done(cfg);
    let max = cfg.max;

    let verify_block = match &cfg.verify {
        Some(cmd) => format!(
            "\n          Run: {cmd}\n          It MUST exit 0 before you may report DONE."
        ),
        None => String::new(),
    };

    format!(
        "# AGENTIC LOOP \u{2014} {slug}

## Goal
{goal}

## Role
You are the loop CONTROLLER. Execute this as an ITERATIVE loop, not a single pass.

## Cycle (repeat each iteration)
1. PLAN   \u{2014} state the smallest next increment toward the goal.
2. ACT    \u{2014} do it. For non-trivial work spawn a worker; otherwise act directly.
3. VERIFY \u{2014} check progress against the Definition of Done.{verify_block}
4. REPORT \u{2014} emit exactly one line, this format:
          LOOP_STATUS: <DONE|CONTINUE|BLOCKED> | iter <n>/{max} | <one-line note>
5. CARRY  \u{2014} update a running STATE summary: what is done, what remains, key decisions.

## Definition of Done
{dod}

## Termination
- Continue while status == CONTINUE and iter < {max}.
- Stop on DONE, BLOCKED, or iter == {max} (then report partial state \u{2014} do not silently continue).

## Constraints (hard)
- Max iterations: {max}.
- Do NOT modify or delete files without explicit confirmation.
- Do NOT send any message (email/Slack/GitHub/etc.) without per-action confirmation.
- Infra changes (Terraform/K8s/CI): always dry-run / plan before apply.
- Never log or print secrets.
- If BLOCKED (needs a decision, missing credential, or ambiguity): stop and surface the specific question \u{2014} do not guess.

## Begin
Start at iteration 1.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(goal: &str) -> Config {
        Config {
            goal: Some(goal.to_string()),
            max: 8,
            verify: None,
            dod: None,
            model: None,
            dry_run: false,
            max_state_chars: 4000,
            claude_bin: "claude".to_string(),
            verbose: false,
            wizard: false,
            config: None,
            save: None,
            export_bash: false,
        }
    }

    #[test]
    fn slug_basic() {
        assert_eq!(slugify("Get the tests green"), "get-the-tests-green");
    }

    #[test]
    fn slug_limits_to_six_words() {
        assert_eq!(
            slugify("one two three four five six seven eight"),
            "one-two-three-four-five-six"
        );
    }

    #[test]
    fn slug_collapses_non_alphanumerics() {
        assert_eq!(slugify("Fix:  the!!! bug__now"), "fix-the-bug-now");
    }

    #[test]
    fn slug_fallback() {
        assert_eq!(slugify("!!! ???"), "loop");
        assert_eq!(slugify(""), "loop");
    }

    #[test]
    fn verify_lines_present_with_verify() {
        let mut cfg = base_config("ship it");
        cfg.verify = Some("cargo test".to_string());
        let out = render_harness(&cfg);
        assert!(out.contains("Run: cargo test"));
        assert!(out.contains("It MUST exit 0 before you may report DONE."));
    }

    #[test]
    fn verify_lines_absent_without_verify() {
        let cfg = base_config("ship it");
        let out = render_harness(&cfg);
        assert!(!out.contains("Run:"));
        assert!(!out.contains("It MUST exit 0"));
    }

    #[test]
    fn dod_auto_without_verify() {
        let cfg = base_config("ship it");
        assert_eq!(
            definition_of_done(&cfg),
            "The stated goal is fully achieved, with concrete evidence cited for each success criterion."
        );
    }

    #[test]
    fn dod_auto_with_verify() {
        let mut cfg = base_config("ship it");
        cfg.verify = Some("cargo test".to_string());
        assert_eq!(
            definition_of_done(&cfg),
            "The stated goal is achieved and the verify command (`cargo test`) exits 0."
        );
    }

    #[test]
    fn dod_explicit_overrides() {
        let mut cfg = base_config("ship it");
        cfg.verify = Some("cargo test".to_string());
        cfg.dod = Some("custom done".to_string());
        assert_eq!(definition_of_done(&cfg), "custom done");
        assert!(render_harness(&cfg).contains("custom done"));
    }

    #[test]
    fn harness_contains_core_sections() {
        let cfg = base_config("demo goal");
        let out = render_harness(&cfg);
        assert!(out.contains("# AGENTIC LOOP"));
        assert!(out.contains("## Goal\ndemo goal"));
        assert!(out.contains("LOOP_STATUS: <DONE|CONTINUE|BLOCKED>"));
        assert!(out.contains("Max iterations: 8."));
        assert!(out.trim_end().ends_with("Start at iteration 1."));
    }
}
