//! `loopgen` — an agentic loop runner for Claude Code.
//!
//! Turns a one-line goal into an iterative loop that drives headless Claude
//! Code (`claude -p`) until the `LOOP_STATUS` termination contract trips.
//!
//! Supports four input modes:
//! 1. Direct goal: `loopgen "goal" --verify "cmd"`
//! 2. Wizard:     `loopgen --wizard`  (interactive)
//! 3. Config:     `loopgen --config loop.toml`
//! 4. Named loop: `loopgen --run fix-tests`  (from the named-loop store)
//!
//! plus store management: `--save-as <NAME>`, `--list`, `--show`, `--remove`.

mod bash_export;
mod cli;
mod config_file;
mod engine;
mod harness;
mod status;
mod store;
mod ui;
mod wizard;

use std::process::ExitCode;

use clap::Parser;

use cli::{validate_input_mode, Config, InputMode};
use config_file::FileConfig;
use store::LoopStore;

/// Build a `cli::Config` from a `FileConfig` (for --config and --wizard paths).
fn file_config_to_cli(fc: &FileConfig) -> Config {
    Config {
        goal: Some(fc.goal.clone()),
        max: fc.max,
        verify: fc.verify.clone(),
        until: fc.until.clone(),
        dod: fc.dod.clone(),
        model: fc.model.clone(),
        dry_run: false,
        max_state_chars: fc.max_state_chars,
        claude_bin: fc.claude_bin.clone(),
        verbose: fc.verbose,
        wizard: false,
        config: None,
        run: None,
        save: None,
        save_as: None,
        export_bash: false,
        list: false,
        show: None,
        remove: None,
    }
}

/// Convert a `cli::Config` (with a goal) to a `FileConfig` for saving/exporting.
fn cli_to_file_config(cfg: &Config) -> FileConfig {
    FileConfig {
        goal: cfg.goal.clone().unwrap_or_default(),
        max: cfg.max,
        verify: cfg.verify.clone(),
        until: cfg.until.clone(),
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
        // ── Store management (--list / --show / --remove) ────────────
        InputMode::Manage => manage_store(&cfg, &LoopStore::from_env()),

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
            let path = cfg.config.as_deref().unwrap_or_default();
            match FileConfig::load_from_file(path) {
                Ok(file_cfg) => run_from_file(&cfg, &file_cfg),
                Err(e) => {
                    eprintln!("error loading config: {e}");
                    ExitCode::from(1)
                }
            }
        }

        // ── Named loop mode ──────────────────────────────────────────
        InputMode::Named => {
            let name = cfg.run.as_deref().unwrap_or_default();
            match LoopStore::from_env().load(name) {
                Ok(file_cfg) => run_from_file(&cfg, &file_cfg),
                Err(e) => {
                    eprintln!("error: {e:#}");
                    ExitCode::from(1)
                }
            }
        }

        // ── Direct goal mode ─────────────────────────────────────────
        InputMode::Goal => finish(&cfg),
    }
}

/// Merge CLI overrides on top of a loaded file config, then finish.
fn run_from_file(cli: &Config, file_cfg: &FileConfig) -> ExitCode {
    let mut run_cfg = file_config_to_cli(file_cfg);
    // Allow --max, --verify, --until, --model, --verbose etc. to override file values
    if cli.max != 8 {
        run_cfg.max = cli.max;
    }
    if cli.verify.is_some() {
        run_cfg.verify.clone_from(&cli.verify);
    }
    if cli.until.is_some() {
        run_cfg.until.clone_from(&cli.until);
    }
    if cli.dod.is_some() {
        run_cfg.dod.clone_from(&cli.dod);
    }
    if cli.model.is_some() {
        run_cfg.model.clone_from(&cli.model);
    }
    if cli.dry_run {
        run_cfg.dry_run = true;
    }
    if cli.verbose {
        run_cfg.verbose = true;
    }
    run_cfg.export_bash = cli.export_bash;
    run_cfg.save.clone_from(&cli.save);
    run_cfg.save_as.clone_from(&cli.save_as);
    finish(&run_cfg)
}

/// Apply the terminal actions (export, save, save-as) or run the loop,
/// using the effective configuration.
fn finish(cfg: &Config) -> ExitCode {
    if cfg.export_bash {
        let script = bash_export::render(&cli_to_file_config(cfg));
        println!("{script}");
        return ExitCode::SUCCESS;
    }

    if cfg.save.is_some() || cfg.save_as.is_some() {
        let file_cfg = cli_to_file_config(cfg);
        if let Some(save_path) = &cfg.save {
            if let Err(e) = file_cfg.save_to_file(save_path) {
                eprintln!("error saving config: {e}");
                return ExitCode::from(1);
            }
            ui::success(&format!("saved to {save_path}"));
        }
        if let Some(name) = &cfg.save_as {
            match LoopStore::from_env().save(name, &file_cfg) {
                Ok(path) => ui::success(&format!(
                    "saved loop '{name}' to {} (run it with: loopgen --run {name})",
                    path.display()
                )),
                Err(e) => {
                    eprintln!("error saving loop: {e:#}");
                    return ExitCode::from(1);
                }
            }
        }
        return ExitCode::SUCCESS;
    }

    run_loop(cfg)
}

/// Handle `--list`, `--show <NAME>`, and `--remove <NAME>`.
fn manage_store(cfg: &Config, store: &LoopStore) -> ExitCode {
    let result = if cfg.list {
        list_loops(store)
    } else if let Some(name) = &cfg.show {
        store.read_raw(name).map(|(path, raw)| {
            println!("# {}", path.display());
            print!("{raw}");
        })
    } else if let Some(name) = &cfg.remove {
        store
            .remove(name)
            .map(|path| ui::success(&format!("removed loop '{name}' ({})", path.display())))
    } else {
        Ok(())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Print each stored loop's name alongside the first line of its goal.
fn list_loops(store: &LoopStore) -> anyhow::Result<()> {
    let names = store.list()?;
    if names.is_empty() {
        println!(
            "no saved loops in {} — create one with --save-as <NAME>",
            store.loops_dir().display()
        );
        return Ok(());
    }
    let width = names.iter().map(|n| n.len()).max().unwrap_or(0);
    for name in names {
        let detail = match store.load(&name) {
            Ok(fc) => fc.goal.lines().next().unwrap_or_default().to_string(),
            Err(e) => format!("(unreadable: {e})"),
        };
        println!("{name:<width$}  {detail}");
    }
    Ok(())
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
