# ProcSentinel - Linux Process Manager

<div align="center">

![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black)
![License](https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge)

**A high-performance Linux process manager built in Rust with both TUI and GUI interfaces**

[Features](#-features) • [Installation](#-installation) • [Usage](#-usage) • [Demo](#-demo) • [Architecture](#-architecture) • [Team](#-team)

</div>

---

## 📖 Overview

**ProcSentinel** is a comprehensive Linux process management tool designed for real-time monitoring and control of system processes. Built using Rust for memory safety and performance, it offers dual interfaces: a powerful Terminal User Interface (TUI) for terminal enthusiasts and a modern Graphical User Interface (GUI) for visual monitoring.

The tool bridges the gap between traditional command-line utilities like `htop`/`top` and modern monitoring solutions, providing advanced features such as container detection, namespace grouping, process checkpointing, and automation capabilities.

## ✨ Features

### Core Process Management
- **Real-time Process Monitoring** - View all running processes with PID, name, CPU/memory usage, status, user, nice value, start time, and more
- **Process Control** - Kill, stop, terminate, continue processes with safety confirmations
- **Priority Management** - Adjust nice values (-20 to 19) with permission handling
- **Process Tree Management** - Kill process trees recursively with dependency warnings
- **Multi-select Operations** - Batch operations on multiple processes simultaneously
- **Start New Processes** - Launch processes with custom working directories and environment variables

### Advanced Filtering & Sorting
- **Simple Filters** - Filter by user, name, PID, PPID
- **Advanced Boolean Queries** - Complex expressions with AND, OR, NOT operators
- **Regular Expression Support** - Regex-based name matching
- **Field Comparisons** - Numeric comparisons (==, !=, >, <, >=, <=)
- **Multi-field Sorting** - Sort by any process attribute in ascending/descending order

### Visualization & Monitoring
- **System Dashboard** - CPU, memory, swap, disk usage with real-time graphs
- **Per-process Graphs** - Individual CPU and memory usage charts
- **Historical Trends** - Time-series data for trend analysis
- **Color-coded Metrics** - Visual indicators for resource thresholds

### Resource Grouping
- **cgroup Grouping** - Aggregate processes by control groups
- **Container Detection** - Automatic Docker/Podman/Kubernetes container identification
- **Namespace Views** - Group by PID, network, mount, UTS, IPC, user namespaces
- **Drill-down Navigation** - Navigate from groups to individual processes

### Automation & Alerts
- **Custom Alerts** - CPU, memory, and process death threshold notifications
- **Task Scheduling** - Interval, cron-like, and one-shot task execution
- **Focus Profiles** - Workflow-based process prioritization (build, editing, presentation)
- **Scripting Rules** - Rhai scripting language for custom automation

### Advanced Features
- **CRIU Integration** - Checkpoint and restore processes (when CRIU is available)
- **Multi-host Monitoring** - Remote process data fetching (read-only)
- **Process Exit Logging** - Track terminated processes with uptime data

## 🚀 Installation

### Prerequisites
- **Operating System**: Linux (primary), macOS, or WSL (with limited features)
- **Rust**: 1.70+ with Cargo
- **Optional**: CRIU for checkpoint/restore functionality

### Build from Source

```bash
# Clone the repository
git clone https://github.com/YOUR_USERNAME/procsentinel.git
cd procsentinel

# Build the project
cargo build --release

# The binary will be at ./target/release/Linux_process_manager
```

### Quick Start

```bash
# Run in TUI mode (default)
cargo run --release

# Run in GUI mode
cargo run --release -- --gui

# Run in agent mode (for multi-host monitoring)
cargo run --release -- --agent --port 8080
```

## 📖 Usage

### TUI Mode Navigation

#### Main Process List
| Key | Action |
|-----|--------|
| `↑/↓` | Navigate processes |
| `1` | Filter/Sort menu |
| `2` | Change priority (nice value) |
| `3` | Kill/Stop/Terminate/Continue |
| `4` | Per-process graphs |
| `5` | Process log |
| `6` | Help |
| `s` | Statistics dashboard |
| `g` | Grouped view (cgroups/containers) |
| `j` | Job scheduler |
| `n` | Start new process |
| `p` | Profile management |
| `A` | Alert management |
| `c` | Checkpoint management |
| `m` | Toggle multi-select mode |
| `Space/Enter` | Select/deselect process |
| `q` | Quit |

#### General Navigation
| Key | Action |
|-----|--------|
| `Esc` | Go back/exit current view |
| `Tab` | Switch between input fields |
| `Enter` | Confirm/execute action |
| `Backspace` | Delete character |

### Advanced Filtering Examples

```
# Filter processes using more than 50% CPU with names starting with "fi"
cpu > 50 AND name ~= "^fi"

# Filter by memory usage
memory > 200 AND user == "root"

# Complex boolean expressions
(cpu > 30 OR memory > 500) AND status == "running"
```

### Configuration

ProcSentinel stores configuration in `~/.lpm/`:
- `profiles.toml` - Focus mode profiles
- `alerts.toml` - Alert configurations
- `scheduled_tasks.toml` - Scheduled tasks
- `checkpoints/` - CRIU checkpoint data

## 🎬 Demo

A demonstration video showcasing ProcSentinel's features is available on Google Drive:

📹 **[Watch Demo Video](https://drive.google.com/drive/folders/1TAH8xwEtSSa2Dmvk5bdUQxbOKBQ9WNZQ)**

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                               │
│                    (Entry Point)                             │
└─────────────────────┬───────────────────┬───────────────────┘
                      │                   │
          ┌───────────▼───────┐ ┌─────────▼─────────┐
          │      ui.rs        │ │      gui.rs       │
          │  (TUI - ratatui)  │ │   (GUI - egui)    │
          └───────────┬───────┘ └─────────┬─────────┘
                      │                   │
                      └─────────┬─────────┘
                                │
┌───────────────────────────────▼───────────────────────────────┐
│                     Shared Backend                             │
├─────────────┬─────────────┬─────────────┬────────────────────┤
│ process.rs  │ graph.rs    │ profile.rs  │ scheduler.rs       │
│ (Core PM)   │ (Metrics)   │ (Profiles)  │ (Task Scheduler)   │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│ alert.rs    │ filter_     │ container_  │ namespace_view.rs  │
│ (Alerts)    │ parser.rs   │ view.rs     │ (NS Grouping)      │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│ criu_       │ coordinator │ agent.rs    │ scripting_rules.rs │
│ manager.rs  │ .rs (Multi) │ (HTTP API)  │ (Rhai Scripts)     │
└─────────────┴─────────────┴─────────────┴────────────────────┘
                                │
┌───────────────────────────────▼───────────────────────────────┐
│                     System Layer                               │
│         sysinfo • procfs • libc • /proc filesystem            │
└───────────────────────────────────────────────────────────────┘
```

### Module Overview

| Module | Description |
|--------|-------------|
| `process.rs` | Core process management, signals, priority control |
| `ui.rs` | Terminal UI using ratatui/crossterm |
| `gui.rs` | Graphical UI using egui/eframe |
| `filter_parser.rs` | Advanced filter expression parser |
| `graph.rs` | System-wide metrics and graphs |
| `per_process_graph.rs` | Individual process metrics |
| `container_view.rs` | Docker/Podman container detection |
| `namespace_view.rs` | Linux namespace grouping |
| `process_group.rs` | Process grouping by various criteria |
| `profile.rs` | Focus mode profile management |
| `alert.rs` | Threshold-based alerting |
| `scheduler.rs` | Task scheduling (cron/interval) |
| `scripting_rules.rs` | Rhai scripting for automation |
| `criu_manager.rs` | CRIU checkpoint/restore integration |
| `coordinator.rs` | Multi-host coordination |
| `agent.rs` | HTTP agent for remote monitoring |
| `process_log.rs` | Process exit logging |

## 📊 Performance

| Metric | 500 Processes | 1,500 Processes |
|--------|---------------|-----------------|
| CPU Overhead | 2-5% | 3-7% |
| Memory Usage | 110-130 MB | 180-220 MB |
| Refresh Latency | 1.2s | 2.5s |
| Filter Latency | 0.4-0.9s | 0.7-1.0s |

## 🔧 Dependencies

- **sysinfo** - System and process information
- **tokio** - Async runtime
- **ratatui** - Terminal UI framework
- **crossterm** - Terminal handling
- **egui/eframe** - GUI framework
- **procfs** - Linux /proc filesystem access
- **chrono** - Date/time handling
- **rhai** - Scripting language
- **axum/reqwest** - HTTP server/client for multi-host
- **serde/toml** - Configuration serialization
- **regex** - Regular expression support
- **clap** - Command-line argument parsing

## ⚠️ Known Limitations

- Cron expression parsing supports basic patterns only
- Remote monitoring is read-only (no remote control)
- I/O alerts are defined but not fully implemented
- CRIU integration requires root privileges
- Process command arguments are not stored (full restart not possible)

## 🛣️ Future Improvements

- Full remote process control with TLS encryption
- Enhanced cron expression support
- Persistent historical data (SQLite/PostgreSQL)
- Role-based access control (RBAC)
- Plugin system for extensibility
- Enhanced alerting channels (email, Slack, webhooks)

## 👥 Team

This project was developed as part of **CSCE 3401 - Operating Systems** at The American University in Cairo.

| Name | Contributions |
|------|---------------|
| **Adham Ali** | Process listing, sorting, safety confirmations, multi-select, process control |
| **Ebram Thabet** | Per-process graphs, dashboard, filtering system, profiles, alerts |
| **Omar Saqr** | Process signals, priority management, grouping, containers, scheduling |
| **Aabed Elghadban** | CRIU integration, coordinator, GUI implementation, visualizations |

**Submitted to**: Dr. Mohamed El Halaby

## 📄 License

This project is available for educational purposes. See the LICENSE file for details.

---

<div align="center">
Made with ❤️ and Rust
</div>
