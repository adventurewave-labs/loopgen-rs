//! `loopgen` — an agentic loop runner for Claude Code.
//!
//! Turns a one-line goal into an iterative loop that drives headless Claude
//! Code (`claude -p`) until the `LOOP_STATUS` termination contract trips.
//!
//! Supports three input modes:
//! 1. Direct goal: `loopgen "goal" --verify "cmd"`
//! 2. Wizard:     `loopgen --wizard`  (interactive)
//! 3. Config:     `loopgen --config loop.toml`

mod bash_export;
mod cli;
mod config_file;
mod engine;
mod harness;
mod status;
mod ui;
mod wizard;

use std::process::ExitCode;

use clap::Parser;

use cli::{validate_input_mode, Config, InputMode};
use config_file::FileConfig;

/// Build a `cli::Config` from a `FileConfig` (for --config and --wizard paths).
fn file_config_to_cli(fc: &FileConfig) -> Config {
    Config {
        goal: Some(fc.goal.clone()),
        max: fc.max,
        verify: fc.verify.clone(),
        dod: fc.dod.clone(),
        model: fc.model.clone(),
        dry_run: false,
        max_state_chars: fc.max_state_chars,
        claude_bin: fc.claude_bin.clone(),
        verbose: fc.verbose,
        wizard: false,
        config: None,
        save: None,
        export_bash: false,
    }
}

/// Convert a `cli::Config` (with a goal) to a `FileConfig` for saving/exporting.
fn cli_to_file_config(cfg: &Config) -> FileConfig {
    FileConfig {
        goal: cfg.goal.clone().unwrap_or_default(),
        max: cfg.max,
        verify: cfg.verify.clone(),
        dod: cfg.dod.clone(),
        model: cfg.model.clone(),
        max_state_chars: cfg.max_state_chars,
        claude_bin: cfg.claude_bin.clone(),
        verbose: cfg.verbose,
    }
}

fn main() -> ExitCode {
    let cfg = Config::parse();

    // Validate input mode
    let mode = match validate_input_mode(&cfg) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(1);
        }
    };

    match mode {
        // ── Wizard mode ──────────────────────────────────────────────
        InputMode::Wizard => {
            let file_cfg = match wizard::run() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("wizard error: {e}");
                    return ExitCode::from(1);
                }
            };
            let should_run = match wizard::post_create(&file_cfg) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("wizard error: {e}");
                    return ExitCode::from(1);
                }
            };
            if should_run {
                let run_cfg = file_config_to_cli(&file_cfg);
                run_loop(&run_cfg)
            } else {
                ExitCode::SUCCESS
            }
        }

        // ── Config file mode ─────────────────────────────────────────
        InputMode::ConfigFile => {
            let path = cfg.config.as_deref().unwrap();
            let file_cfg = match FileConfig::load_from_file(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error loading config: {e}");
                    return ExitCode::from(1);
                }
            };

            // Merge CLI overrides on top of file config
            let mut run_cfg = file_config_to_cli(&file_cfg);
            // Allow --max, --verify, --model, --verbose etc. to override file values
            if cfg.max != 8 {
                run_cfg.max = cfg.max;
            }
            if cfg.verify.is_some() {
                run_cfg.verify = cfg.verify;
            }
            if cfg.dod.is_some() {
                run_cfg.dod = cfg.dod;
            }
            if cfg.model.is_some() {
                run_cfg.model = cfg.model;
            }
            if cfg.dry_run {
                run_cfg.dry_run = true;
            }
            if cfg.verbose {
                run_cfg.verbose = true;
            }

            if cfg.export_bash {
                let script = bash_export::render(&file_cfg);
                println!("{script}");
                return ExitCode::SUCCESS;
            }

            if let Some(save_path) = &cfg.save {
                if let Err(e) = file_cfg.save_to_file(save_path) {
                    eprintln!("error saving config: {e}");
                    return ExitCode::from(1);
                }
                ui::success(&format!("saved to {save_path}"));
                return ExitCode::SUCCESS;
            }

            run_loop(&run_cfg)
        }

        // ── Direct goal mode ─────────────────────────────────────────
        InputMode::Goal => {
            if cfg.export_bash {
                let file_cfg = cli_to_file_config(&cfg);
                let script = bash_export::render(&file_cfg);
                println!("{script}");
                return ExitCode::SUCCESS;
            }

            if let Some(save_path) = &cfg.save {
                let file_cfg = cli_to_file_config(&cfg);
                if let Err(e) = file_cfg.save_to_file(save_path) {
                    eprintln!("error saving config: {e}");
                    return ExitCode::from(1);
                }
                ui::success(&format!("saved to {save_path}"));
                return ExitCode::SUCCESS;
            }

            run_loop(&cfg)
        }
    }
}

/// Execute the loop engine with the given CLI config.
fn run_loop(cfg: &Config) -> ExitCode {
    if cfg.dry_run {
        println!("{}", harness::render_harness(cfg));
        return ExitCode::SUCCESS;
    }

    match engine::run(cfg) {
        Ok(outcome) => ExitCode::from(outcome.exit_code() as u8),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}