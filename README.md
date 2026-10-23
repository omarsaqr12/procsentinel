# ProcSentinel

**A Rust/Linux process manager with terminal and graphical interfaces.**

ProcSentinel combines process and resource inspection with filtering, signals,
priority management, cgroup/namespace views, scheduling, alerts, a read-only
monitoring agent and optional CRIU checkpoint integration. It was built by a
four-person team for Operating Systems at The American University in Cairo.

[Build and try it](#build-and-try-it) · [Architecture](#architecture-and-code-map) ·
[Current verification and remaining work](docs/CURRENT_STATUS.md) ·
[Team contributions](#team-and-attribution)

## Build and try it

Requires Linux, Rust 1.85+ and Cargo. The graphical interface also needs a desktop
and the platform libraries required by egui/eframe. CRIU is optional.

```bash
git clone https://github.com/omarsaqr12/procsentinel.git
cd procsentinel
cargo build --locked
cargo run --locked -- --help
cargo run --locked                  # terminal UI
cargo run --locked -- --gui         # graphical desktop required
```

Start by inspecting processes with an unprivileged account. For filter examples,
see [the parser](src/filter_parser.rs); an expression such as
`cpu > 50 AND name ~= "^fi"` combines a threshold with a regular expression.

### Read-only agent

```bash
cargo run --locked -- --agent --port 3000
curl http://127.0.0.1:3000/api/health
curl http://127.0.0.1:3000/api/processes
```

The agent binds to **127.0.0.1**, exposes process names/users, and has no built-in
authentication or TLS. Use an authenticated SSH tunnel for authorized remote
access, for example `ssh -L 3001:127.0.0.1:3000 user@remote-host`. Keep the HTTP
port local rather than exposing it directly to a network.

A [live loopback smoke check](scripts/ci_agent_smoke.sh) exercises both endpoints,
checks the listener when `ss` is available, and cleans up its own agent process.

## Architecture and code map

| Component | Implementation | What to inspect |
| --- | --- | --- |
| Collection and control | [process.rs](src/process.rs) | Linux /proc, sysinfo, signals, priorities and launching |
| Terminal interface | [ui.rs](src/ui.rs) | ratatui/crossterm interaction |
| Graphical interface | [gui.rs](src/gui.rs) | egui/eframe interface |
| Filtering | [filter_parser.rs](src/filter_parser.rs) | Expression parsing and field evaluation |
| Linux grouping | [container_view.rs](src/container_view.rs), [namespace_view.rs](src/namespace_view.rs) | cgroups and namespaces |
| Automation | [scheduler.rs](src/scheduler.rs), [alert.rs](src/alert.rs) | Scheduled actions and thresholds |
| Monitoring | [agent.rs](src/agent.rs), [coordinator.rs](src/coordinator.rs) | Read-only HTTP agent and coordinator |
| Checkpointing | [criu_manager.rs](src/criu_manager.rs) | ID/path guards, metadata and restored-PID validation |

[main.rs](src/main.rs) selects the terminal, GUI or agent entry point.

## Verification

[![Rust checks](https://github.com/omarsaqr12/procsentinel/actions/workflows/rust-checks.yml/badge.svg?branch=main)](https://github.com/omarsaqr12/procsentinel/actions/workflows/rust-checks.yml)

```bash
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
cargo build --locked
bash scripts/ci_agent_smoke.sh
```

The workflow covers formatting, compilation, unit tests and a live agent smoke
check. The September 18 [format/agent repair](https://github.com/omarsaqr12/procsentinel/pull/5)
and [checkpoint repair](https://github.com/omarsaqr12/procsentinel/pull/4) were
merged after the September 16 review. The [current status](docs/CURRENT_STATUS.md)
separates those changes from remaining limitations; the older review documents
are retained as historical records.

These checks do not establish complete GUI/TUI parity, a real CRIU
checkpoint/restore cycle, or safe handling of every process-control race.

## Scope and remaining work

This is an educational systems project. Important boundaries remain:

- Checkpoint IDs now have a strict allowlist and directory/symlink checks, and
  restored PIDs must be nonzero. Concurrent directory renames still need stronger
  directory-handle confinement; see [current status](docs/CURRENT_STATUS.md).
- Process actions still use numeric PIDs without an atomic identity guarantee.
  PID reuse and special PID values need additional guards. Exercise control only
  on known, disposable user-owned child processes.
- Invalid cron syntax can fall back to every-minute scheduling. Avoid unattended
  destructive schedules until validation is implemented.
- CRIU requires suitable kernel support and permissions; normal exploration
  should use an unprivileged account.
- No reproducible performance benchmark is included, and platform behavior
  beyond the documented Linux setup is not established.

## Team and attribution

The original project documentation attributes process listing, safety confirmations
and multi-select to **Adham Ali**; resource graphs, filtering, profiles and alerts
to **Ebram Thabet**; signals, priorities, grouping, containers and scheduling to
**Omar Saqr**; and CRIU, coordinator, GUI and visualizations to **Aabed Elghadban**.
These are documented team contributions rather than an independently established
file-by-file authorship map. Submitted to Dr. Mohamed El Halaby for CSCE 3401.

[MIT license](LICENSE).
