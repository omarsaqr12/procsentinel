# Engineering review notes — 2026-09-16

**Baseline:** `15dddb6236c8a665644fcbc056a7e3681906041b` on `main`. This is a source review with limited remote CI verification, **not** full independent validation.

## Scope and method

GitHub's recursive tracked tree listed 22 baseline files. The README, package manifest and entry point were inspected, along with the agent, coordinator, filter parser, scheduler, grouping and selected process/CRIU modules. The large terminal/graphical UI and other modules were not exhaustively reviewed. `Cargo.lock` was inventoried, not independently dependency-audited entry by entry. The license was not changed. No demo video was independently viewed.

The connected GitHub API enabled a review branch, commits and GitHub Actions CI. Direct cloning in the analysis environment was blocked by host-resolution problems, and no local Rust/Cargo toolchain, Linux UI session or CRIU test host was available. **GitHub Actions performed compilation and unit tests; no live agent network, GUI, process-control or CRIU test was performed.**

## Findings and handling

| Priority | Evidence and impact | Status |
| --- | --- | --- |
| High | Original `src/agent.rs` bound `0.0.0.0:<port>` and served `/api/processes` without authentication, disclosing process names and users to reachable network clients. | **Changed:** binds to `127.0.0.1`; focused address-selection unit test passes in CI. Actual socket reachability remains untested. |
| High | `src/criu_manager.rs` constructs paths using caller-provided checkpoint IDs; deletion invokes `remove_dir_all` on the joined path without validating absolute paths or parent components. | **Unresolved:** potential deletion outside checkpoint directory; see [CRIU_SECURITY.md](CRIU_SECURITY.md). Do not delete arbitrary checkpoint IDs. |
| High | `src/process.rs` acts on numeric PIDs without an atomic identity check; PIDs may be recycled. PID 0 has special process-group behavior for POSIX `kill(2)`. | **Unresolved:** design and test safe signal handling before using on uncertain PIDs. |
| High | `src/scheduler.rs::check_due_tasks` falls back to every-minute execution for invalid cron expressions; scheduled actions include kill, restart and renice. | **Unresolved:** reject invalid expressions and add boundary regression tests. |
| Medium | Original README asserted performance for 500/1,500 processes without tracked benchmark fixtures or measurements. | **Changed documentation:** unsupported numbers removed. |
| Medium | Original README described broad platform/UI parity unsupported by inspected source or execution. | **Changed documentation:** Linux target and interface limits clarified. |
| Medium | `src/filter_parser.rs` uses a bespoke recursive expression parser. Unicode/malformed expression and precedence behavior merits focused tests. | **Unresolved hypothesis:** reproduce before claiming a defect. |

## Verification ledger

| Check | Result | Evidence / next action |
| --- | --- | --- |
| Repository identity and baseline | PASS | GitHub ref and recursive tracked tree inspected. |
| Agent change and regression test committed on review branch | PASS | The branch includes both changes. |
| `cargo check --locked` | PASS | GitHub Actions [run 35084852701](https://github.com/omarsaqr12/procsentinel/actions/runs/35084852701), independent compile job. |
| `cargo test --locked` | PASS | Same CI run, independent unit-test job, including loopback-address test. No claim of process/GUI/CRIU integration coverage. |
| `cargo fmt --all -- --check` | FAIL | Same CI run, independent formatting job. Formatting remains unresolved; do not present the PR as all-green. |
| Live agent bind and non-loopback refusal | NOT RUN | Requires a controlled Linux host with suitable network interfaces. |
| Linux TUI and GUI smoke tests | NOT RUN | Requires interactive terminal and graphical session. |
| PID/cron/CRIU regression tests | NOT RUN | Open code-level risks, no safe test fixture or repair completed. |
| Benchmark reproduction | NOT RUN | No benchmark harness or preserved measurement data. |

### Safe follow-up validation

1. Resolve formatting failures and rerun all affected CI jobs. Before merge, require `cargo fmt --all -- --check`, `cargo check --locked` and `cargo test --locked` to pass on the final commit.
2. Start `cargo run --locked -- --agent --port 3000`, verify `curl http://127.0.0.1:3000/api/health` returns 200, and check `ss -ltn` lists `127.0.0.1:3000`, **not** `0.0.0.0:3000` or `[::]:3000`.
3. Validate the agent locally and over an authenticated SSH tunnel on a controlled second host; do not open the port to the internet.
4. Test any process-control fix on disposable user-owned child processes, including missing/recycled PID, PID 0 and permission denial; never point destructive tests at system processes.
5. Guard checkpoint path containment before attempting deletion; test only inside a disposable temporary directory. Validate cron syntax and boundaries using an injected clock.

No default-branch changes, deployment, license changes or merges are part of this review.
