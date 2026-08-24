# loopgen

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

**Agentic loop runner for Claude Code.** `loopgen` turns a one-line goal into an
iterative loop that drives headless Claude Code (`claude -p`) until a termination
contract trips. It is the "all I do is write loops for Claude" pattern, packaged
as a small, dependable CLI: a hard iteration cap, an optional verify gate, and a
`BLOCKED` escape hatch so a run never spins forever or terminates silently.

```text
loopgen "get the test suite green" --verify "cargo test" --max 6
```

## Demo

![loopgen rendering an agentic loop harness with --dry-run](demo.gif)

*Recorded from the actual binary with [asciinema](https://asciinema.org) + [agg](https://github.com/asciinema/agg).*

## Install

```sh
cargo install --path .
```

This builds the optimized binary and installs `loopgen` into your Cargo bin
directory (`~/.cargo/bin` by default). You need the [`claude`](https://docs.claude.com/en/docs/claude-code)
CLI available on your `PATH` (or pass `--claude-bin <PATH>`) for real runs.

Or install from [crates.io](https://crates.io/crates/loopgen):

```sh
cargo install loopgen
```

### TOML configuration

Save a loop definition as a file for reuse:

```toml
# loop.toml
goal = "get the test suite green"
max = 12
verify = "cargo test"
```

Then run with `loopgen --config loop.toml`. CLI flags override file values when both are set.

### Bash export

Export any loop as a standalone, portable bash script:

```sh
loopgen "fix the parser" --verify "cargo test" --export-bash > fix-loop.sh
chmod +x fix-loop.sh
./fix-loop.sh
```

The exported script uses only `claude`, `grep`, and `python3` — no Rust required at runtime.

## Usage

```text
loopgen [OPTIONS] <GOAL>
loopgen --wizard
loopgen --config loop.toml
```

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `<GOAL>` | positional | — (required) | The outcome to drive toward |
| `--max <N>` | `u32` | `8` | Hard iteration cap (safety rail) |
| `--verify <CMD>` | string | none | Shell command; `DONE` is only accepted if it exits 0 |
| `--dod <TEXT>` | string | none | Explicit Definition of Done; otherwise auto-derived |
| `--model <NAME>` | string | none | Forwarded to `claude -p --model` |
| `--dry-run` | flag | false | Render the harness, print it, exit 0 (no claude calls) |
| `--max-state-chars <N>` | `usize` | `4000` | Cap on running state carried between iterations |
| `--claude-bin <PATH>` | string | `claude` | Override the claude binary path |
| `-v, --verbose` | flag | false | Echo each invocation and raw status lines |
| `--wizard` | flag | false | Interactive configuration wizard |
| `--config <FILE>` | string | none | Load configuration from a TOML file |
| `--save <FILE>` | string | none | Save effective config to TOML and exit |
| `--export-bash` | flag | false | Export as a standalone bash script and exit |

## Examples

### Preview the harness without calling Claude

`--dry-run` renders the exact prompt `loopgen` would send and exits 0, so you can
review or tweak the goal before spending tokens:

```sh
loopgen "demo goal" --dry-run
```

```text
# AGENTIC LOOP — demo-goal

## Goal
demo goal

## Role
You are the loop CONTROLLER. Execute this as an ITERATIVE loop, not a single pass.

## Cycle (repeat each iteration)
1. PLAN   — state the smallest next increment toward the goal.
2. ACT    — do it. For non-trivial work spawn a worker; otherwise act directly.
3. VERIFY — check progress against the Definition of Done.
4. REPORT — emit exactly one line, this format:
          LOOP_STATUS: <DONE|CONTINUE|BLOCKED> | iter <n>/8 | <one-line note>
5. CARRY  — update a running STATE summary: what is done, what remains, key decisions.
...
```

### Drive a goal with a verify gate

When you pass `--verify`, a `DONE` claim from the model is only accepted if the
command exits 0; otherwise that iteration is downgraded to `CONTINUE` and the
loop keeps going:

```sh
loopgen "get tests green" --verify "cargo test"
```

Each iteration prints a concise status line, and the run finishes with a summary:

```text
[iter 1/8] CONTINUE — wrote a failing-case fix, tests still red
[iter 2/8] DONE — all tests pass
✓ loop complete: goal reported DONE.
```

### Pick a model

```sh
loopgen "refactor the parser for clarity" --model claude-opus-4-8 --max 4
```

## How the loop works

Each iteration `loopgen`:

1. Composes a prompt = the rendered **harness** + the **running state** from
   prior iterations + an instruction to do exactly one increment.
2. Invokes `claude -p <prompt> --output-format json [--model …]` and reads the
   `result` field from the JSON envelope (falling back to raw stdout if the
   output is not JSON).
3. Extracts the contract line with a case-insensitive match (last one wins):

   ```text
   LOOP_STATUS: <DONE|CONTINUE|BLOCKED> | iter <n>/<max> | <one-line note>
   ```

4. If the status is `DONE` and `--verify` is set, runs the verify command via
   `sh -c`; a non-zero exit downgrades the result to `CONTINUE`.
5. Appends a trimmed summary of the result to the running state (capped at
   `--max-state-chars`, keeping the tail when over) and continues.

The loop **continues** while the status is `CONTINUE` and `iter < --max`. It
**stops** on `DONE`, on `BLOCKED`, or when it reaches `--max`. A missing
`LOOP_STATUS` line is treated as `CONTINUE` (with a warning) so a quiet model
does not silently end the run.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Goal reported `DONE` (and verified, if `--verify` was set) |
| `2` | Loop stopped because the model reported `BLOCKED` |
| `3` | Reached `--max` without `DONE` |
| `1` | Internal/setup error (e.g. the `claude` binary was not found) |

## Development

```sh
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under the [MIT License](LICENSE). © 2026 Marcus Patman.

## Why loops, not prompts

Boris Cherny — the creator of Claude Code — has talked about how his own workflow moved away from one-shot prompting. Rather than hand-crafting a single perfect prompt and hoping for a one-shot result, he increasingly *writes loops*: hand the agent a goal and let it iterate — act, check, correct — until the goal is actually met. The prompt stops being the deliverable; the loop is.

`loopgen` is that idea as a tool. You give it the outcome you want; it compiles the goal into a structured loop harness and drives Claude Code (`claude -p`) around a PLAN → ACT → VERIFY → REPORT cycle until a termination contract trips: `DONE` (optionally gated on a real verify command), `BLOCKED` (it needs a decision from you), or a hard `--max` iteration cap so a run can never spin forever. Stop writing prompts. Start writing loops.
