# ProcSentinel

**A Linux process-inspection and management application written in Rust, with terminal and graphical interfaces.** Developed by a four-person team for CSCE 3401 (Operating Systems) at the American University in Cairo. This is an educational system-programming project, **not a production-hardened process manager**.

ProcSentinel reads process and resource information, supports filtering, signals and priority management, and includes optional cgroup/namespace views, scheduling, alerts, remote read-only monitoring and a CRIU integration. The repository does not contain a reproducible performance benchmark; earlier quantitative performance claims have been removed rather than presented without measurements.

## Architecture and code map

| Component | Implementation | Purpose |
| --- | --- | --- |
| Collection and control | [`src/process.rs`](src/process.rs) | Linux `/proc`, `sysinfo`, process signals, nice values and launching |
| Terminal interface | [`src/ui.rs`](src/ui.rs) | `ratatui` and `crossterm` |
| Graphical interface | [`src/gui.rs`](src/gui.rs) | `eframe` / `egui` |
| Filtering | [`src/filter_parser.rs`](src/filter_parser.rs) | Hand-written expression parser and field evaluation |
| Linux grouping | [`src/container_view.rs`](src/container_view.rs), [`src/namespace_view.rs`](src/namespace_view.rs) | cgroups and namespaces |
| Automation | [`src/scheduler.rs`](src/scheduler.rs), [`src/alert.rs`](src/alert.rs) | Scheduled actions and threshold alerts |
| Read-only agent | [`src/agent.rs`](src/agent.rs), [`src/coordinator.rs`](src/coordinator.rs) | HTTP process-list endpoint and coordinator |
| Optional checkpointing | [`src/criu_manager.rs`](src/criu_manager.rs) | CRIU command integration; important unresolved path-safety defect below |

The entry point and supported CLI options are in [`src/main.rs`](src/main.rs). Default launch starts the terminal interface; `--gui` starts the graphical interface and `--agent` starts the HTTP monitoring endpoint.

## Build and inspect

Requirements: Linux, Rust 1.85 or later (2024 edition), Cargo, and Linux graphical dependencies for the GUI. CRIU is optional. Use an unprivileged account and disposable user-owned processes for initial exploration; do **not** run this prototype as root for routine use.

```bash
git clone https://github.com/omarsaqr12/procsentinel.git
cd procsentinel
cargo build --locked
cargo run --locked                  # terminal UI; interactive terminal required
cargo run --locked -- --gui         # graphical desktop required
cargo run --locked -- --help        # inspect exact CLI switches
```

The filters support field comparisons and Boolean expressions. Example: `cpu > 50 AND name ~= "^fi"`; implementation details are in [`src/filter_parser.rs`](src/filter_parser.rs). The project is Linux-specific; macOS and WSL behavior has not been validated.

### Agent access

```bash
cargo run --locked -- --agent --port 3000
curl http://127.0.0.1:3000/api/health
curl http://127.0.0.1:3000/api/processes
```

This branch binds the agent to **127.0.0.1 only**. The API has **no built-in authentication or TLS** and exposes process names and users. Never expose the port directly to a LAN or the internet. To reach an authorized remote machine, run the agent there and use SSH forwarding such as `ssh -L 3001:127.0.0.1:3000 user@remote-host`, then connect a local coordinator to `127.0.0.1:3001`. SSH secures the tunnel; it does not add authentication to the HTTP service itself. A unit test covers the selected bind address, but actual socket reachability remains untested.

## Limitations and safety

- **Do not delete arbitrary CRIU checkpoints:** [`src/criu_manager.rs`](src/criu_manager.rs) joins caller-provided IDs into filesystem paths and recursively removes the result without adequately validating path containment. An absolute or parent-traversing ID could delete outside the intended directory. This unresolved defect is documented in [`docs/CRIU_SECURITY.md`](docs/CRIU_SECURITY.md).
- Process IDs can be recycled after listing and before a signal; no atomic identity guarantee is implemented. PID 0 has special process-group semantics. Exercise process actions only on known, disposable child processes until guards are implemented.
- Invalid cron expressions can fall back to every-minute execution in the scheduler; do not entrust it with unattended destructive actions.
- The CRIU integration depends on host kernel, permissions and target-process constraints; neither general restore reliability nor GUI/TUI feature parity has been independently verified.
- No tracked benchmark harness or measurements support quantitative performance promises. No full integration-test suite is present.

## Verification status for this review branch

On GitHub Actions [run 35084852701](https://github.com/omarsaqr12/procsentinel/actions/runs/35084852701), `cargo check --locked` **passed** and `cargo test --locked` **passed**, including the new loopback-address unit test. `cargo fmt --all -- --check` **failed**; the PR is not all-green and should not be merged until formatting is resolved and affected checks rerun. CI does not establish safe live process actions, GUI operation, agent network isolation or CRIU behavior. The review branch has since received documentation-only edits, so the final commit also needs its own full CI check.

Local verification commands:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
```

See [`docs/REVIEW_NOTES.md`](docs/REVIEW_NOTES.md) for the source findings, exact verification ledger and remaining acceptance tests.

## Team and attribution

The original project documentation attributes process listing, safety confirmations and multi-select to **Adham Ali**; resource graphs, filtering, profiles and alerts to **Ebram Thabet**; signals, priorities, grouping, containers and scheduling to **Omar Saqr**; and CRIU, coordinator, GUI and visualizations to **Aabed Elghadban**. These are documented team contributions, not an independently verified file-by-file authorship map. Submitted to Dr. Mohamed El Halaby for CSCE 3401.

**License:** [MIT](LICENSE), unchanged in this review.
