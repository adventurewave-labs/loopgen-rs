//! Named-loop store: save, list, show, load, and remove loop configurations
//! by name, so a loop you have dialed in can be re-run with `--run <NAME>`.
//!
//! Loops are plain TOML files (the same format as `--config`) stored at
//! `<store>/loops/<NAME>.toml`, where `<store>` is resolved from
//! `$LOOPGEN_DIR`, then `$XDG_CONFIG_HOME/loopgen`, then
//! `$HOME/.config/loopgen`.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config_file::FileConfig;

/// Longest accepted loop name.
const MAX_NAME_LEN: usize = 64;

/// A directory of named loop configurations.
#[derive(Debug, Clone)]
pub struct LoopStore {
    loops_dir: PathBuf,
}

/// Resolve the store root from an environment lookup function.
///
/// Split out from [`LoopStore::from_env`] so the precedence can be tested
/// without mutating the process environment.
pub fn store_root(env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(dir) = non_empty("LOOPGEN_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = non_empty("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("loopgen");
    }
    if let Some(home) = non_empty("HOME") {
        return PathBuf::from(home).join(".config").join("loopgen");
    }
    PathBuf::from(".loopgen")
}

/// Reject names that are empty, too long, or could escape the store
/// directory. Allowed: ASCII letters, digits, `-`, and `_`.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("loop name must not be empty");
    }
    if name.len() > MAX_NAME_LEN {
        bail!("loop name must be at most {MAX_NAME_LEN} characters: {name:?}");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("invalid loop name {name:?}: use only letters, digits, '-' and '_'");
    }
    Ok(())
}

impl LoopStore {
    /// A store rooted at `root` (loops live in `root/loops`).
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            loops_dir: root.as_ref().join("loops"),
        }
    }

    /// The store at the location resolved from the process environment.
    pub fn from_env() -> Self {
        Self::new(store_root(|k| std::env::var_os(k)))
    }

    /// Directory holding the `<NAME>.toml` files.
    pub fn loops_dir(&self) -> &Path {
        &self.loops_dir
    }

    /// Path of the file backing loop `name` (validated, may not exist yet).
    pub fn path_for(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.loops_dir.join(format!("{name}.toml")))
    }

    /// Save `cfg` as loop `name`, overwriting any existing loop of that name.
    pub fn save(&self, name: &str, cfg: &FileConfig) -> Result<PathBuf> {
        let path = self.path_for(name)?;
        fs::create_dir_all(&self.loops_dir)
            .with_context(|| format!("failed to create {}", self.loops_dir.display()))?;
        let path_str = path.to_string_lossy();
        cfg.save_to_file(&path_str)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    /// Load loop `name`.
    pub fn load(&self, name: &str) -> Result<FileConfig> {
        let path = self.path_for(name)?;
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                bail!(
                    "no loop named '{name}' in {} (see `loopgen --list`)",
                    self.loops_dir.display()
                )
            }
            Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
        };
        FileConfig::parse(&content).with_context(|| format!("invalid loop file {}", path.display()))
    }

    /// Read the raw TOML of loop `name` (for `--show`).
    pub fn read_raw(&self, name: &str) -> Result<(PathBuf, String)> {
        // Parse first so a missing or malformed loop reports a clear error.
        self.load(name)?;
        let path = self.path_for(name)?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Ok((path, raw))
    }

    /// Names of all stored loops, sorted. A missing store directory is empty.
    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.loops_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("failed to read {}", self.loops_dir.display()))
            }
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .filter(|n| validate_name(n).is_ok())
            .collect();
        names.sort();
        Ok(names)
    }

    /// Delete loop `name`, returning the removed path.
    pub fn remove(&self, name: &str) -> Result<PathBuf> {
        let path = self.path_for(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(path),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                bail!("no loop named '{name}' in {}", self.loops_dir.display())
            }
            Err(e) => Err(e).with_context(|| format!("failed to delete {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(goal: &str) -> FileConfig {
        FileConfig {
            goal: goal.to_string(),
            max: 5,
            until: Some("cargo test".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        let cfg = sample("get tests green");
        let path = store.save("fix-tests", &cfg).unwrap();
        assert_eq!(path, tmp.path().join("loops").join("fix-tests.toml"));
        assert!(path.is_file());
        assert_eq!(store.load("fix-tests").unwrap(), cfg);
    }

    #[test]
    fn save_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        store.save("a", &sample("first")).unwrap();
        store.save("a", &sample("second")).unwrap();
        assert_eq!(store.load("a").unwrap().goal, "second");
    }

    #[test]
    fn list_is_sorted_and_ignores_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        store.save("zeta", &sample("z")).unwrap();
        store.save("alpha", &sample("a")).unwrap();
        fs::write(store.loops_dir().join("notes.txt"), "x").unwrap();
        fs::write(store.loops_dir().join("bad name.toml"), "goal = \"x\"").unwrap();
        fs::create_dir(store.loops_dir().join("dir.toml")).unwrap();
        assert_eq!(store.list().unwrap(), vec!["alpha", "zeta"]);
    }

    #[test]
    fn list_of_missing_store_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path().join("does-not-exist"));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn load_missing_names_the_loop() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        let err = store.load("ghost").unwrap_err().to_string();
        assert!(err.contains("no loop named 'ghost'"), "{err}");
    }

    #[test]
    fn load_malformed_reports_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        fs::create_dir_all(store.loops_dir()).unwrap();
        fs::write(store.loops_dir().join("broken.toml"), "max = \"nope\"").unwrap();
        let err = format!("{:#}", store.load("broken").unwrap_err());
        assert!(err.contains("invalid loop file"), "{err}");
    }

    #[test]
    fn remove_deletes_and_errors_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        let path = store.save("gone", &sample("g")).unwrap();
        assert_eq!(store.remove("gone").unwrap(), path);
        assert!(!path.exists());
        assert!(store.remove("gone").is_err());
    }

    #[test]
    fn read_raw_returns_file_text() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        store.save("show-me", &sample("visible goal")).unwrap();
        let (path, raw) = store.read_raw("show-me").unwrap();
        assert!(path.ends_with("show-me.toml"));
        assert!(raw.contains("goal = \"visible goal\""));
        assert!(raw.contains("until = \"cargo test\""));
    }

    #[test]
    fn names_are_validated() {
        for ok in ["fix-tests", "a", "Loop_2", &"x".repeat(MAX_NAME_LEN)] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "../escape",
            "a/b",
            "a b",
            "dot.name",
            &"x".repeat(MAX_NAME_LEN + 1),
        ] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
        let tmp = tempfile::tempdir().unwrap();
        let store = LoopStore::new(tmp.path());
        assert!(store.save("../evil", &sample("x")).is_err());
        assert!(!tmp.path().join("evil.toml").exists());
    }

    #[test]
    fn store_root_precedence() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(
            store_root(env(&[
                ("LOOPGEN_DIR", "/custom"),
                ("XDG_CONFIG_HOME", "/xdg"),
                ("HOME", "/home/u"),
            ])),
            PathBuf::from("/custom")
        );
        assert_eq!(
            store_root(env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/u")])),
            PathBuf::from("/xdg/loopgen")
        );
        assert_eq!(
            store_root(env(&[("LOOPGEN_DIR", ""), ("HOME", "/home/u")])),
            PathBuf::from("/home/u/.config/loopgen")
        );
        assert_eq!(store_root(env(&[])), PathBuf::from(".loopgen"));
    }
}
