# Engineering review notes — 2026-09-16

**Baseline:** `15dddb6236c8a665644fcbc056a7e3681906041b` on `main`. This document records source-review findings and limitations, **not** a claim of full independent validation.

## Scope and method

GitHub's recursive tracked tree lists 22 files: five top-level files and 17 Rust modules. The README, package manifest and entry point were read, along with the complete agent, coordinator, filter parser and scheduler modules and selected parts of the process and CRIU modules. Other modules (particularly the large `ui.rs`, `gui.rs`, `graph.rs` and the remainder of `process.rs`) were not reviewed exhaustively. `Cargo.lock` was inventoried but not dependency-audited entry by entry. The existing license is unchanged. No external demo video was independently viewed.

The connected GitHub API allowed a review branch and code edits, but this execution environment could not resolve `github.com` to clone the project and has no Rust/Cargo toolchain. **No Cargo build, unit test, GUI, real-process, remote-host or CRIU test was run.** A source-level inspection is not a test pass.

## Findings and handling

| Priority | Evidence and impact | Status |
| --- | --- | --- |
| High | Original `src/agent.rs` bound `0.0.0.0:<port>` and served `/api/processes` without authentication. Process names and users could be disclosed to anyone with network access to the port. | **Code change:** bind to `127.0.0.1`; add a loopback-address unit test. Real socket test still required. |
| High | `src/process.rs` sends POSIX signals using numeric PIDs. The code shown has no atomic process-identity check between a snapshot and action. PID recycling can target a different process. PID 0 is a process-group special case for `kill(2)` and should be rejected by a future guarded-signaling patch. | **Unresolved:** do not use actions on uncertain PIDs; requires a separately tested design change. |
| High | `src/scheduler.rs::check_due_tasks` falls back to every-minute execution for invalid cron expressions; scheduled actions include process kill, restart and renice. | **Unresolved:** reject invalid expressions before enabling a task, with regression tests. Do not rely on the current parser for destructive scheduled actions. |
| Medium | The original README provided CPU, memory and latency tables for 500/1,500 processes, but no tracked benchmark script, fixtures or measurement records are present. | **Documentation change:** remove unsupported measurements; do not imply reproducible performance. |
| Medium | The original README described every TUI/GUI feature as shared and described 2024-edition Rust as broadly cross-platform. UI feature equivalence and non-Linux operation were not verified. | **Documentation change:** narrow support claims and identify Linux as target. |
| Medium | `src/filter_parser.rs` is a hand-written recursive parser and uses byte slicing while examining Unicode input; malformed/unusual expressions need panic-resistance and operator-precedence tests. | **Unresolved:** hypothesis requiring a reproducer and a parser-focused test suite. |

## Verification ledger

| Check | Result | Evidence / next action |
| --- | --- | --- |
| Repository identity and baseline commit | PASS | GitHub reference and recursive tracked tree inspected. |
| Agent source change committed on separate branch | PASS | Review branch contains loopback bind change and a focused unit test. |
| `cargo fmt --all -- --check` | BLOCKED | No checkout / Cargo in execution environment. |
| `cargo check --locked` | BLOCKED | No checkout / Cargo in execution environment. |
| `cargo test --locked` | BLOCKED | No checkout / Cargo in execution environment. |
| Live agent bind / network exposure test | NOT RUN | Run on Linux and verify non-loopback connection refusal. |
| Linux TUI and GUI smoke tests | NOT RUN | Requires Linux runtime and GUI session. |
| PID-control / scheduler regression tests | NOT RUN | Guard and tests not implemented in this small patch. |
| CRIU integration and benchmark reproduction | NOT RUN | No suitable test host, safe target fixture or benchmark harness. |

### Suggested Linux host test plan

1. Run `cargo fmt --all -- --check && cargo check --locked && cargo test --locked` after cloning this branch.
2. Start `cargo run --locked -- --agent --port 3000` and check `curl http://127.0.0.1:3000/api/health` returns HTTP 200; inspect `ss -ltn` for `127.0.0.1:3000` and **not** `0.0.0.0:3000` or `[::]:3000`.
3. Validate the agent process-list response locally and the SSH-tunnel flow with a controlled second Linux host. Do not expose the port on the public internet.
4. Use a disposable child process for signals/priority operations; include PID 0, vanished PID, PID reuse and permission-denied cases in future regression coverage. Never point destructive tests at system processes.
5. Validate cron syntax rejection and schedule boundaries with an injected clock before enabling scheduled destructive actions.

No main-branch changes, deployment, license changes, or publication are authorized by this review patch.
