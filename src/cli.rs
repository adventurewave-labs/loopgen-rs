//! Command-line argument definitions for `loopgen`.

use clap::Parser;

/// Agentic loop runner for Claude Code.
///
/// Turns a one-line goal into an iterative loop that drives headless Claude
/// Code (`claude -p`) until a termination contract (`LOOP_STATUS`) trips.
#[derive(Parser, Debug, Clone)]
#[command(name = "loopgen", version, about, long_about = None)]
pub struct Config {
    /// The outcome to drive toward.
    pub goal: Option<String>,

    /// Hard iteration cap (safety rail).
    #[arg(long, default_value_t = 8)]
    pub max: u32,

    /// Shell command; `DONE` is only accepted if it exits 0.
    #[arg(long)]
    pub verify: Option<String>,

    /// Explicit Definition of Done; otherwise auto-derived.
    #[arg(long)]
    pub dod: Option<String>,

    /// Model name forwarded to `claude -p --model`.
    #[arg(long)]
    pub model: Option<String>,

    /// Render the harness, print it, and exit 0 (no claude calls).
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Cap on running state carried between iterations.
    #[arg(long, default_value_t = 4000)]
    pub max_state_chars: usize,

    /// Override the claude binary path.
    #[arg(long, default_value = "claude")]
    pub claude_bin: String,

    /// Echo each invocation and raw status lines.
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Run the interactive configuration wizard.
    #[arg(long, conflicts_with = "goal")]
    pub wizard: bool,

    /// Load configuration from a TOML file.
    #[arg(long, conflicts_with = "goal")]
    pub config: Option<String>,

    /// Save the effective configuration to a TOML file and exit.
    #[arg(long)]
    pub save: Option<String>,

    /// Export the loop as a standalone bash script and exit.
    #[arg(long)]
    pub export_bash: bool,
}

/// Validate that exactly one input mode is active (goal, wizard, or config).
pub fn validate_input_mode(cfg: &Config) -> Result<InputMode, String> {
    let has_goal = cfg.goal.is_some();
    let has_wizard = cfg.wizard;
    let has_config = cfg.config.is_some();

    let count = has_goal as u8 + has_wizard as u8 + has_config as u8;
    if count == 0 {
        return Err(
            "error: provide a <GOAL>, --wizard, or --config <FILE>. \
             Run \`loopgen --help\` for usage."
                .to_string(),
        );
    }
    if count > 1 {
        return Err(
            "error: <GOAL>, --wizard, and --config are mutually exclusive.".to_string(),
        );
    }
    Ok(if has_goal {
        InputMode::Goal
    } else if has_wizard {
        InputMode::Wizard
    } else {
        InputMode::ConfigFile
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Goal,
    Wizard,
    ConfigFile,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> Config {
        Config {
            goal: Some("test goal".to_string()),
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
    fn validate_goal_mode() {
        let cfg = base_config();
        assert_eq!(validate_input_mode(&cfg).unwrap(), InputMode::Goal);
    }

    #[test]
    fn validate_wizard_mode() {
        let mut cfg = base_config();
        cfg.goal = None;
        cfg.wizard = true;
        assert_eq!(validate_input_mode(&cfg).unwrap(), InputMode::Wizard);
    }

    #[test]
    fn validate_config_mode() {
        let mut cfg = base_config();
        cfg.goal = None;
        cfg.config = Some("loop.toml".to_string());
        assert_eq!(validate_input_mode(&cfg).unwrap(), InputMode::ConfigFile);
    }

    #[test]
    fn reject_no_input() {
        let mut cfg = base_config();
        cfg.goal = None;
        assert!(validate_input_mode(&cfg).is_err());
    }

    #[test]
    fn reject_goal_and_wizard() {
        let mut cfg = base_config();
        cfg.wizard = true;
        assert!(validate_input_mode(&cfg).is_err());
    }

    #[test]
    fn reject_goal_and_config() {
        let mut cfg = base_config();
        cfg.config = Some("f.toml".to_string());
        assert!(validate_input_mode(&cfg).is_err());
    }
}