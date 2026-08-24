//! Interactive wizard that builds a `FileConfig` by asking questions.

use crate::config_file::FileConfig;
use crate::ui;
use std::io;

pub fn run() -> io::Result<FileConfig> {
    ui::banner("loopgen \u{00b7} wizard");
    println!();
    ui::info(
        "This wizard creates a loopgen configuration.
\
         Answer the questions below, then choose to save, run, or export.",
    );
    println!();

    let goal = ui::ask_text("Goal \u{2014} what should Claude achieve?", None)?;
    println!();

    let max = ui::ask_u64("Max iterations (safety cap)", 8)? as u32;
    println!();

    let verify = ui::ask_optional(
        "Verify command (shell cmd; DONE is only accepted if this exits 0)",
    )?;
    println!();

    let dod = if verify.is_some() {
        None
    } else {
        ui::ask_optional("Definition of Done (auto-derived if blank)")?
    };
    println!();

    let model = ui::ask_optional("Model (default = Claude Code's configured model)")?;
    println!();

    let verbose = ui::ask_bool("Verbose output (echo each invocation)?", false)?;
    println!();

    Ok(FileConfig {
        goal,
        max,
        verify,
        dod,
        model,
        max_state_chars: 4000,
        claude_bin: "claude".to_string(),
        verbose,
    })
}

/// Ask post-creation actions and execute them.
///
/// Returns `Ok(true)` if the loop should be run, `Ok(false)` if not.
pub fn post_create(cfg: &FileConfig) -> io::Result<bool> {
    // Show summary
    println!("{}", ui::bold("Configuration summary:"));
    let goal_display = if cfg.goal.len() > 60 {
        let truncated = &cfg.goal[..60];
        format!("{}...", truncated)
    } else {
        cfg.goal.clone()
    };
    println!("  goal:          {}", ui::dim(&goal_display));
    println!("  max:           {}", cfg.max);
    println!(
        "  verify:        {}",
        ui::dim(match &cfg.verify {
            Some(v) => v.as_str(),
            None => "(none)",
        })
    );
    println!(
        "  model:         {}",
        ui::dim(match &cfg.model {
            Some(m) => m.as_str(),
            None => "default",
        })
    );
    println!("  verbose:       {}", if cfg.verbose { "yes" } else { "no" });
    println!();

    let save = ui::ask_bool("Save configuration to loop.toml?", true)?;
    if save {
        let path = ui::ask_text("File path", Some("loop.toml"))?;
        match cfg.save_to_file(&path) {
            Ok(()) => ui::success(&format!("saved to {}", path)),
            Err(e) => ui::error(&format!("failed to save: {}", e)),
        }
    }
    println!();

    let export = ui::ask_bool("Export as a standalone bash script?", false)?;
    if export {
        let path = ui::ask_text("Output file", Some("loop.sh"))?;
        let script = crate::bash_export::render(cfg);
        match std::fs::write(&path, &script) {
            Ok(()) => {
                ui::success(&format!("exported to {}", path));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&path)?.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&path, perms);
                }
            }
            Err(e) => ui::error(&format!("failed to export: {}", e)),
        }
    }
    println!();

    let run = ui::ask_bool("Run the loop now?", true)?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = FileConfig::default();
        assert!(cfg.goal.is_empty());
        assert_eq!(cfg.max, 8);
        assert!(cfg.verify.is_none());
        assert_eq!(cfg.claude_bin, "claude");
    }
}
