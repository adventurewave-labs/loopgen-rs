//! Terminal UI helpers — colors and interactive prompts, zero external dependencies.

use std::io::{self, BufRead, IsTerminal, Write};

/// Returns true if colored output should be used.
pub fn color_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    io::stdout().is_terminal()
}

fn c(s: &str, code: &str) -> String {
    if color_enabled() {
        format!("\x1b[{}m{}\x1b[0m", code, s)
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    c(s, "1")
}
pub fn dim(s: &str) -> String {
    c(s, "2")
}
pub fn cyan(s: &str) -> String {
    c(s, "36")
}
pub fn green(s: &str) -> String {
    c(s, "32")
}
pub fn yellow(s: &str) -> String {
    c(s, "33")
}
pub fn red(s: &str) -> String {
    c(s, "31")
}
/// Not currently called outside its own test/`step` -- kept for a consistent
/// full color set (bold/dim/cyan/green/yellow/red/magenta).
#[allow(dead_code)]
pub fn magenta(s: &str) -> String {
    c(s, "35")
}

pub fn banner(title: &str) {
    let width = title.chars().count();
    let line = "─".repeat(width + 2);
    println!("{}", cyan(&format!("┌{}┐", line)));
    println!(
        "{} {} {}",
        cyan("│"),
        bold(title),
        cyan("│")
    );
    println!("{}", cyan(&format!("└{}┘", line)));
}

pub fn info(msg: &str) {
    println!("{} {}", cyan("ℹ"), msg);
}
pub fn warn(msg: &str) {
    println!("{} {}", yellow("⚠"), msg);
}
pub fn success(msg: &str) {
    println!("{} {}", green("✓"), msg);
}
pub fn error(msg: &str) {
    eprintln!("{} {}", red("✗"), msg);
}
/// Not currently called from the wizard flow -- kept for a future
/// step-numbering UI, tested below.
#[allow(dead_code)]
pub fn step(msg: &str) {
    println!("{} {}", magenta("▸"), bold(msg));
}

fn read_line_opt() -> io::Result<Option<String>> {
    let mut buf = String::new();
    let n = io::stdin().lock().read_line(&mut buf)?;
    if n == 0 {
        Ok(None)
    } else {
        while buf.ends_with('\n') || buf.ends_with('\r') {
            buf.pop();
        }
        Ok(Some(buf))
    }
}

fn flush() {
    let _ = io::stdout().flush();
}

pub fn ask_text(prompt: &str, default: Option<&str>) -> io::Result<String> {
    loop {
        match default {
            None => print!("{} ", cyan(&format!("{}:", prompt))),
            Some("") => print!(
                "{} {} ",
                cyan(&format!("{}:", prompt)),
                dim("(optional)")
            ),
            Some(d) => print!(
                "{} {} ",
                cyan(&format!("{}:", prompt)),
                dim(&format!("[{}]", d))
            ),
        }
        flush();
        match read_line_opt()? {
            None => {
                println!();
                match default {
                    Some(d) => return Ok(d.to_string()),
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "input closed while a required value was expected",
                        ))
                    }
                }
            }
            Some(line) => {
                let line = line.trim();
                if line.is_empty() {
                    match default {
                        Some(d) => return Ok(d.to_string()),
                        None => {
                            warn("This field is required.");
                            continue;
                        }
                    }
                }
                return Ok(line.to_string());
            }
        }
    }
}

pub fn ask_optional(prompt: &str) -> io::Result<Option<String>> {
    print!("{} {} ", cyan(&format!("{}:", prompt)), dim("(press Enter to skip)"));
    flush();
    match read_line_opt()? {
        None => Ok(None),
        Some(line) => {
            let line = line.trim().to_string();
            Ok(if line.is_empty() { None } else { Some(line) })
        }
    }
}

pub fn ask_bool(prompt: &str, default: bool) -> io::Result<bool> {
    let hint = if default {
        "[Y/n]"
    } else {
        "[y/N]"
    };
    loop {
        print!("{} {} ", cyan(&format!("{}:", prompt)), dim(hint));
        flush();
        match read_line_opt()? {
            None => {
                println!();
                return Ok(default);
            }
            Some(line) => {
                let line = line.trim().to_lowercase();
                if line.is_empty() {
                    return Ok(default);
                }
                match line.as_str() {
                    "y" | "yes" => return Ok(true),
                    "n" | "no" => return Ok(false),
                    _ => {
                        warn("Please answer y or n.");
                    }
                }
            }
        }
    }
}

pub fn ask_u64(prompt: &str, default: u64) -> io::Result<u64> {
    loop {
        print!(
            "{} {} ",
            cyan(&format!("{}:", prompt)),
            dim(&format!("[{}]", default))
        );
        flush();
        match read_line_opt()? {
            None => {
                println!();
                return Ok(default);
            }
            Some(line) => {
                let line = line.trim();
                if line.is_empty() {
                    return Ok(default);
                }
                match line.parse::<u64>() {
                    Ok(n) => return Ok(n),
                    Err(_) => warn("Please enter a whole number."),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_helpers_return_string() {
        // These should not panic regardless of terminal state
        let _ = bold("test");
        let _ = dim("test");
        let _ = cyan("test");
        let _ = green("test");
        let _ = yellow("test");
        let _ = red("test");
        let _ = magenta("test");
    }

    #[test]
    fn banner_formats() {
        // Just ensure it doesn't panic
        banner("test");
    }

    #[test]
    fn info_warn_success_error_format() {
        info("info");
        warn("warn");
        success("ok");
        error("err");
        step("step");
    }
}