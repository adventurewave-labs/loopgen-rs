//! TOML configuration file support for loopgen.

//! Allows saving and loading loop configurations as TOML files,
//! enabling reusable loop definitions and the wizard workflow.

use serde::{Deserialize, Serialize};
use std::io;

/// Serializable loop configuration.
///
/// This is the on-disk representation; it can be converted to CLI arguments
/// for the core loop engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileConfig {
    /// The outcome to drive toward.
    #[serde(default)]
    pub goal: String,
    /// Hard iteration cap (safety rail).
    #[serde(default = "default_max")]
    pub max: u32,
    /// Shell command; DONE is only accepted if it exits 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
    /// Explicit Definition of Done; otherwise auto-derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dod: Option<String>,
    /// Model name forwarded to `claude -p --model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Cap on running state carried between iterations.
    #[serde(default = "default_state_chars")]
    pub max_state_chars: usize,
    /// Override the claude binary path.
    #[serde(default = "default_claude_bin")]
    pub claude_bin: String,
    /// Echo each invocation and raw status lines.
    #[serde(default)]
    pub verbose: bool,
}

fn default_max() -> u32 {
    8
}
fn default_state_chars() -> usize {
    4000
}
fn default_claude_bin() -> String {
    "claude".to_string()
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            goal: String::new(),
            max: default_max(),
            verify: None,
            dod: None,
            model: None,
            max_state_chars: default_state_chars(),
            claude_bin: default_claude_bin(),
            verbose: false,
        }
    }
}

impl FileConfig {
    /// Save the configuration to a TOML file.
    pub fn save_to_file(&self, path: &str) -> io::Result<()> {
        let toml_str =
            toml::to_string_pretty(self).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Prepend a comment for discoverability.
        let header = "# loopgen configuration\n# Run with: loopgen --config <FILE>\n\n";
        std::fs::write(path, format!("{}{}", header, toml_str))
    }

    /// Load a configuration from a TOML file.
    pub fn load_from_file(path: &str) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Parse a TOML string into a FileConfig.
    pub fn parse(toml_str: &str) -> io::Result<Self> {
        toml::from_str(toml_str)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Convert to CLI argument strings suitable for building a `cli::Config`.
    ///
    /// Returns a vector where the first element is the goal, followed by flag pairs.
    ///
    /// Not currently called from `main` (which round-trips through `file_config_to_cli`
    /// instead) -- kept as tested public API for callers who want a flag-string form.
    #[allow(dead_code)]
    pub fn to_cli_args(&self) -> Vec<String> {
        let mut args = vec![self.goal.clone()];
        args.push("--max".to_string());
        args.push(self.max.to_string());
        if let Some(v) = &self.verify {
            args.push("--verify".to_string());
            args.push(v.clone());
        }
        if let Some(d) = &self.dod {
            args.push("--dod".to_string());
            args.push(d.clone());
        }
        if let Some(m) = &self.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }
        args.push("--max-state-chars".to_string());
        args.push(self.max_state_chars.to_string());
        if self.claude_bin != "claude" {
            args.push("--claude-bin".to_string());
            args.push(self.claude_bin.clone());
        }
        if self.verbose {
            args.push("--verbose".to_string());
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_toml() {
        let cfg = FileConfig {
            goal: "get tests green".to_string(),
            max: 12,
            verify: Some("cargo test".to_string()),
            dod: None,
            model: Some("sonnet".to_string()),
            max_state_chars: 8000,
            claude_bin: "claude".to_string(),
            verbose: true,
        };
        let toml_str = toml::to_string(&cfg).expect("serialize");
        let back: FileConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(cfg, back);
    }

    #[test]
    fn minimal_toml() {
        let toml_str = r#"
            goal = "ship it"
            max = 4
            "#;
        let cfg: FileConfig = toml::from_str(toml_str).expect("parse");
        assert_eq!(cfg.goal, "ship it");
        assert_eq!(cfg.max, 4);
        assert!(cfg.verify.is_none());
        assert!(cfg.model.is_none());
        assert!(!cfg.verbose);
    }

    #[test]
    fn defaults_are_sensible() {
        let cfg = FileConfig::default();
        assert_eq!(cfg.max, 8);
        assert_eq!(cfg.max_state_chars, 4000);
        assert_eq!(cfg.claude_bin, "claude");
        assert!(!cfg.verbose);
    }

    #[test]
    fn to_cli_args_basic() {
        let cfg = FileConfig {
            goal: "do the thing".to_string(),
            max: 6,
            verify: Some("cargo test".to_string()),
            dod: None,
            model: Some("opus".to_string()),
            max_state_chars: 4000,
            claude_bin: "claude".to_string(),
            verbose: false,
        };
        let args = cfg.to_cli_args();
        assert_eq!(args[0], "do the thing");
        assert!(args.contains(&"--max".to_string()));
        assert!(args.contains(&"6".to_string()));
        assert!(args.contains(&"--verify".to_string()));
        assert!(args.contains(&"cargo test".to_string()));
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"opus".to_string()));
        // Default claude_bin should not appear
        assert!(!args.iter().any(|a| a.starts_with("--claude-bin")));
    }

    #[test]
    fn to_cli_args_nondefault_claude_bin() {
        let cfg = FileConfig {
            goal: "test".to_string(),
            claude_bin: "/usr/local/bin/claude".to_string(),
            ..Default::default()
        };
        let args = cfg.to_cli_args();
        assert!(args.contains(&"--claude-bin".to_string()));
        assert!(args.contains(&"/usr/local/bin/claude".to_string()));
    }

    #[test]
    fn to_cli_args_verbose() {
        let cfg = FileConfig {
            goal: "test".to_string(),
            verbose: true,
            ..Default::default()
        };
        let args = cfg.to_cli_args();
        assert!(args.contains(&"--verbose".to_string()));
    }

    #[test]
    fn to_cli_args_no_verify_skips_flag() {
        let cfg = FileConfig {
            goal: "test".to_string(),
            verify: None,
            ..Default::default()
        };
        let args = cfg.to_cli_args();
        assert!(!args.contains(&"--verify".to_string()));
    }
}