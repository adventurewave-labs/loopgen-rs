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

## loopgen vs. plain loops and Ralph

Every agentic loop technique is some version of "call `claude -p` repeatedly until something is true." The differences are in what "something is true" means and how much state survives between calls.

**A plain `while` loop** —

```sh
while :; do claude -p "get the tests green"; done
```

— has no termination contract at all. It runs until you `Ctrl+C` it or your terminal dies. Nothing carries between iterations except whatever Claude itself wrote to disk; each call starts blind to what the last one concluded, and a `DONE`-sounding response is just prose in a transcript, not a signal anything reads.

**The [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)** is a real step up: `while :; do cat PROMPT.md | claude; done`, with an `IMPLEMENTATION_PLAN.md` persisted on disk as shared state, a documented file convention (`PROMPT.md`, `AGENTS.md`, `specs/`), and (typically) `--dangerously-skip-permissions` so it can run unattended. Each iteration re-reads the plan, picks the most important remaining task, implements it, commits, and exits with a fresh context window. It's a genuinely good pattern — minimal, fully inspectable, no binary dependency beyond `claude` itself. But the stop condition is still implicit: nothing parses a machine-readable "done" signal, so termination is still "a human watches and hits Ctrl+C" or an external max-iteration wrapper, and "is this actually done" is left entirely to the model's own judgment — there's no equivalent of a verify gate that can override an optimistic `DONE`.

**`loopgen`** keeps Ralph's core insight (fresh context per iteration, disk/prompt-carried state, let the model do the work) and adds the parts a plain loop or Ralph leave to chance:

- a `LOOP_STATUS: <DONE|CONTINUE|BLOCKED>` line the harness actually parses, not just prose the model happens to write
- distinct exit codes (`0`/`2`/`3`/`1`) so a wrapper script, CI job, or another agent can act on the result programmatically instead of scraping a transcript
- an optional `--verify` gate: a `DONE` claim is only accepted if a real command exits 0, otherwise the loop is forced back to `CONTINUE`
- a hard `--max` cap as a safety rail that doesn't depend on anyone watching
- state carried explicitly in the prompt (capped, tail-truncated) rather than re-derived by the model from a plan file each time

None of this makes `loopgen` strictly better than Ralph — "just markdown files and a bash one-liner" is a legitimate, more inspectable design point if you'd rather read `IMPLEMENTATION_PLAN.md` than trust a binary's parsing. `loopgen` is for when you want the termination contract and the verify gate enforced by the harness itself, not by convention.

**cloop**, the org's other Claude Code loop runner, takes the opposite trade-off: no parsed status line and no verify gate, but a wizard-first setup and named, saved loop files under `~/.config/cloop/` you can list, show, edit, and re-run by name. Pick `loopgen` when you want the harness to enforce the contract and gate `DONE` on a real command; pick [`cloop`](https://github.com/adventurewave-labs/cloop) when you want a zero-config wizard and fast reuse of loops you've already dialed in.
