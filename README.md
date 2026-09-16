# ProcSentinel

**A Rust process-inspection and management application for Linux, with terminal and graphical interfaces.**

ProcSentinel combines process listing and filtering with signal delivery, priority changes, resource graphs, grouping by Linux cgroups and namespaces, and optional scheduling and checkpoint/restore integrations. It was developed by a four-person team for **CSCE 3401 — Operating Systems** at the American University in Cairo. The source is the evidence for its implemented features; this repository does **not** include a reproducible performance benchmark or evidence of production hardening.

## What to inspect

| Area | Source | What it demonstrates |
| --- | --- | --- |
| Process collection and control | [`src/process.rs`](src/process.rs) | Linux `/proc` information, `sysinfo`, signals, nice values and process launching |
| Terminal interface | [`src/ui.rs`](src/ui.rs) | `ratatui`/`crossterm` process-management interface |
| Graphical interface | [`src/gui.rs`](src/gui.rs) | `egui`/`eframe` interface |
| Filtering | [`src/filter_parser.rs`](src/filter_parser.rs) | Hand-written expression parser and field evaluation |
| Grouping | [`src/container_view.rs`](src/container_view.rs), [`src/namespace_view.rs`](src/namespace_view.rs) | cgroup-based container identification and namespace discovery |
| Automation | [`src/scheduler.rs`](src/scheduler.rs), [`src/alert.rs`](src/alert.rs) | Scheduled actions and threshold monitoring |
| Remote read-only monitoring | [`src/agent.rs`](src/agent.rs), [`src/coordinator.rs`](src/coordinator.rs) | HTTP process-list endpoint and coordinator client |
| Optional checkpointing | [`src/criu_manager.rs`](src/criu_manager.rs) | CRIU command integration, subject to host and process constraints |

The default mode starts the terminal interface. `--gui` starts the graphical interface. `--agent` starts the read-only monitoring endpoint instead. CLI arguments are defined in [`src/main.rs`](src/main.rs).

## Build and run

**Requirements:** Linux, a Rust toolchain supporting the 2024 edition (Rust 1.85 or newer), Cargo and any native graphics libraries required by the `eframe` backend on your distribution. CRIU is optional and not needed for ordinary process monitoring. The repository includes `Cargo.lock` for a reproducible dependency selection, not a guarantee of identical behavior on every Linux host.

```bash
git clone https://github.com/omarsaqr12/procsentinel.git
cd procsentinel
cargo build --locked
cargo run --locked                 # terminal UI; run in an interactive terminal
cargo run --locked -- --gui        # graphical UI; needs a desktop environment
cargo run --locked -- --help       # authoritative CLI options
```

Run initially as your normal user, **not as root**. Signals and priority changes still require the operating system's normal permissions. Destructive actions can terminate real processes; try the interface on disposable user-owned processes first. The app reads Linux-specific `/proc`, cgroup and namespace data; macOS and WSL behavior has not been validated here.

### Agent mode and its security boundary

```bash
cargo run --locked -- --agent --port 3000
# on the same machine:
curl http://127.0.0.1:3000/api/health
curl http://127.0.0.1:3000/api/processes
```

The agent binds to **127.0.0.1 only**. Its HTTP API has **no built-in authentication or TLS** and returns process names, users and resource usage. Do not expose it directly to a LAN or the internet. For an authorized remote host, run the agent there and use SSH local port forwarding, for example `ssh -L 3001:127.0.0.1:3000 user@remote-host`; point the local coordinator at `127.0.0.1:3001`. The SSH connection supplies transport security and access control; this does not make the endpoint safe to publish publicly.

## Filters and features

The expression parser supports `AND`, `OR`, `NOT`, parentheses, field comparisons and regex matching. Example expressions to try in the interface:

```text
cpu > 50 AND name ~= "^fi"
(memory > 200 OR user == "root") AND status == "Running"
```

The `memory` comparison uses MiB in the implementation. The parser is bespoke and does not implement a complete query language; validate complex or untrusted filters before relying on them. Container and namespace identification uses Linux `/proc` data and may be incomplete when permissions or cgroup naming differ. Task scheduling, alerting, automatic restart and CRIU are advanced capabilities, **not** independently validated operational safeguards.

### Important limitations

- This repository has no tracked benchmark harness, benchmark environment description or measurements supporting quantitative CPU, memory or latency claims. No throughput or process-count promise is made.
- Process IDs can be recycled between listing and an action. Confirm the selected process and its identity before signaling, changing priority or terminating a tree; this implementation does not provide an atomic PID-identity guarantee.
- CRIU restore depends on kernel support, privileges, namespaces, available resources and the target process. It has not been demonstrated as a general-purpose restart mechanism.
- The agent is read-only but unauthenticated; its default loopback binding is a deliberate safeguard, not a replacement for access control if the design is extended.
- The tree contains no standalone integration-test suite. The loopback binding has a focused unit test, but real process-management, GUI and CRIU behavior still require Linux-host validation.

## Review and verification

```bash
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
```

These are **instructions**, not assertions that those commands passed in this review. For behavioral verification, also run the UI on Linux, send signals only to a disposable process, and confirm that agent mode cannot be reached via a non-loopback address. See [`docs/REVIEW_NOTES.md`](docs/REVIEW_NOTES.md) for the source-audit findings and remaining test requirements.

## Team and attribution

| Contributor | Contributions recorded in the original project documentation |
| --- | --- |
| Adham Ali | Process listing, sorting, safety confirmations, multi-select and process control |
| Ebram Thabet | Per-process graphs, dashboard, filtering, profiles and alerts |
| Omar Saqr | Process signals, priority management, grouping, containers and scheduling |
| Aabed Elghadban | CRIU integration, coordinator, GUI and visualizations |

Submitted to Dr. Mohamed El Halaby for CSCE 3401. These descriptions are the team's documented attribution; they are not an independently verified file-by-file authorship audit.

**License:** [MIT](LICENSE). This change does not alter the license.
