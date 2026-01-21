use crate::process::ProcessManager;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
// use std::collections::HashMap; //delete after debugging

// Import Ratatui components
use ratatui::{
    widgets::{Block, Borders, Dataset, GraphType, Chart, Paragraph},
    layout::{Layout, Constraint, Direction, Alignment, Rect},
    text::{Span, Line},
};

// Import Ratatui's color separately to avoid confusion
use ratatui::style::{Style, Modifier, Color as RatatuiColor};

// Import Crossterm components with explicit namespace
// use crossterm::{
//     style::{Color as CrosstermColor, SetForegroundColor, SetBackgroundColor, ResetColor, Attribute, SetAttribute},
//     ExecutableCommand, 
//     QueueableCommand,
// };

use crate::ui::StatisticsTab;  // Add this at the top with other imports
use crate::process::ProcessInfo;

// Add this struct at the top with other structs
pub struct CpuInfo {
    pub usage: f32,
    last_idle: u64,
    last_total: u64,
}

impl CpuInfo {
    fn new() -> Self {
        Self {
            usage: 0.0,
            last_idle: 0,
            last_total: 0,
        }
    }
}

// Modify GraphData struct
pub struct GraphData {
    cpu_history: VecDeque<f32>,
    memory_history: VecDeque<u64>,
    max_points: usize,
    last_update: Instant,
    update_interval: Duration,
    cpu_infos: Vec<CpuInfo>,  // Keep this for per-core display
    per_process_history: std::collections::HashMap<u32, (VecDeque<f32>, VecDeque<u64>)>,
}

impl GraphData {
    pub fn new(max_points: usize, update_interval_ms: u64) -> Self {
        GraphData {
            cpu_history: VecDeque::with_capacity(max_points),
            memory_history: VecDeque::with_capacity(max_points),
            max_points,
            last_update: Instant::now(),
            update_interval: Duration::from_millis(update_interval_ms),
            cpu_infos: (0..get_cpu_count()).map(|_| CpuInfo::new()).collect(),
            per_process_history: std::collections::HashMap::new(),
        }
    }

    fn update_cpu_info(&mut self) {
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            let lines: Vec<&str> = stat.lines().collect();
            
            // Handle individual cores for the CPU bars display
            for (i, cpu_info) in self.cpu_infos.iter_mut().enumerate() {
                if let Some(line) = lines.get(i + 1) {  // Skip first line (aggregate CPU)
                    if line.starts_with("cpu") {
                        let values: Vec<u64> = line.split_whitespace()
                            .skip(1)  // Skip "cpu" prefix
                            .filter_map(|val| val.parse().ok())
                            .collect();

                        if values.len() >= 4 {
                            let idle = values[3];
                            let total: u64 = values.iter().sum();

                            let idle_delta = idle - cpu_info.last_idle;
                            let total_delta = total - cpu_info.last_total;

                            if total_delta > 0 {
                                cpu_info.usage = 100.0 * (1.0 - (idle_delta as f32 / total_delta as f32));
                            }

                            cpu_info.last_idle = idle;
                            cpu_info.last_total = total;
                        }
                    }
                }
            }
        }
    }

    pub fn update(&mut self, process_manager: &ProcessManager) {
        let now = Instant::now();
        if now.duration_since(self.last_update) < self.update_interval {
            return;
        }

        // Update CPU info for the per-core display
        self.update_cpu_info();
        
        // Get total CPU usage from all processes
        let total_cpu: f32 = process_manager.get_processes()
            .iter()
            .map(|p| p.cpu_usage)
            .sum();
        
        // Add to history and maintain max size
        self.cpu_history.push_back(total_cpu);
        while self.cpu_history.len() > self.max_points {
            self.cpu_history.pop_front();
        }
        
        // Use system memory usage from /proc/meminfo
        let (_mem_total, mem_used, _mem_free, _mem_cached, _mem_available) = get_memory_info();
        let total_memory = mem_used / 1024; // Convert to MB
        self.memory_history.push_back(total_memory);
        while self.memory_history.len() > self.max_points {
            self.memory_history.pop_front();
        }
        
        // Update per-process history (leave as is for per-process graphs)
        let current_pids: std::collections::HashSet<u32> = process_manager.get_processes()
            .iter()
            .map(|p| p.pid)
            .collect();
        self.per_process_history.retain(|&pid, _| current_pids.contains(&pid));
        for process in process_manager.get_processes() {
            let entry = self.per_process_history.entry(process.pid).or_insert_with(|| {
                (VecDeque::with_capacity(self.max_points), VecDeque::with_capacity(self.max_points))
            });
            entry.0.push_back(process.cpu_usage);
            entry.1.push_back(process.memory_usage);
            while entry.0.len() > self.max_points {
                entry.0.pop_front();
            }
            while entry.1.len() > self.max_points {
                entry.1.pop_front();
            }
        }
        self.last_update = now;
    }

    pub fn get_cpu_infos(&self) -> &[CpuInfo] {
        &self.cpu_infos
    }

    pub fn get_cpu_history(&self) -> &VecDeque<f32> {
        &self.cpu_history
    }

    pub fn get_memory_history(&self) -> &VecDeque<u64> {
        &self.memory_history
    }

    pub fn get_process_history(&self, pid: u32) -> Option<(&VecDeque<f32>, &VecDeque<u64>)> {
        self.per_process_history.get(&pid).map(|(cpu, mem)| (cpu, mem))
    }
}

pub fn render_graph_dashboard(
    frame: &mut ratatui::Frame,
    graph_data: &GraphData,
    current_tab: &StatisticsTab,
    process_list: &[ProcessInfo],
    area: Rect,
) {
    let size = area;
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(size.height.saturating_sub(3)),
        ])
        .split(size);
    render_tabs(frame, main_chunks[0], current_tab);
    match current_tab {
        StatisticsTab::Graphs => render_graphs_tab(frame, main_chunks[1], graph_data),
        StatisticsTab::Overview => render_overview_tab(frame, main_chunks[1], graph_data, process_list),
        StatisticsTab::CPU => render_cpu_tab(frame, main_chunks[1], graph_data),
        StatisticsTab::Memory => render_memory_tab(frame, main_chunks[1]),
        StatisticsTab::Disk => render_disk_tab(frame, main_chunks[1]),
        StatisticsTab::Processes => {
            render_processes_tab(frame, main_chunks[1], process_list);
        },
        StatisticsTab::Advanced => render_advanced_tab(frame, main_chunks[1], graph_data),
        StatisticsTab::PerProcessGraph | StatisticsTab::ProcessLog | StatisticsTab::Help => {
            // Placeholder
        }
    }
}

pub fn render_tabs(frame: &mut ratatui::Frame, area: Rect, current_tab: &StatisticsTab) {
    // Get the current tab name
    let current_tab_name = match current_tab {
        StatisticsTab::Graphs => "Graphs",
        StatisticsTab::Overview => "Overview",
        StatisticsTab::CPU => "CPU Stats",
        StatisticsTab::Memory => "Memory Stats",
        StatisticsTab::Disk => "Disk Stats",
        StatisticsTab::Processes => "Processes",
        StatisticsTab::Advanced => "Advanced Stats",
        StatisticsTab::PerProcessGraph => "Per-Process Graph",
        StatisticsTab::ProcessLog => "Process Log",
        StatisticsTab::Help => "Help",
    };

    let title = Line::from(vec![
        Span::styled("Current View: ", Style::default().fg(RatatuiColor::Black)),
        Span::styled(current_tab_name, 
            Style::default()
                .fg(RatatuiColor::Black)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
        Span::raw(" "),
        Span::styled("[1] Graphs  [2] Overview  [3] CPU  [4] Memory  [5] Disk  [6] Processes  [7] Advanced ", Style::default().fg(RatatuiColor::Black)),
        Span::styled("[S/Esc] Return", Style::default().fg(RatatuiColor::Black))
    ]);

    let header = Paragraph::new(title)
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(RatatuiColor::Black)));

    frame.render_widget(header, area);
}

pub fn render_graphs_tab(
    frame: &mut ratatui::Frame,
    area: Rect,
    graph_data: &GraphData,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),   // CPU Bars (2 rows of 8 CPUs)
            Constraint::Length(3),   // Memory/Swap Bars with spacing
            Constraint::Percentage(45),  // CPU Graph
            Constraint::Percentage(45),  // Memory Graph
        ])
        .split(area);

    // Render CPU bars (similar to htop)
    render_cpu_bars(frame, chunks[0], graph_data);
    
    // Create a sub-layout for memory bars with spacing
    let mem_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),   // Memory bar
            Constraint::Length(1),   // Spacing
            Constraint::Length(1),   // Swap bar
        ])
        .split(chunks[1]);

    // Render Memory/Swap bars with spacing
    render_memory_bars(frame, mem_chunks[0], mem_chunks[2], graph_data);

    // Render the graphs
    render_cpu_graph(frame, chunks[2], graph_data);
    render_memory_graph(frame, chunks[3], graph_data);
}

fn render_cpu_bars(frame: &mut ratatui::Frame, area: Rect, graph_data: &GraphData) {
    let num_cpus = get_cpu_count();
    let cpus_per_row = 8;
    let num_rows = (num_cpus + cpus_per_row - 1) / cpus_per_row;
    
    let row_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(2); num_rows])
        .split(area);

    for row in 0..num_rows {
        let start_cpu = row * cpus_per_row;
        let end_cpu = ((row + 1) * cpus_per_row).min(num_cpus);
        let num_cpus_this_row = end_cpu - start_cpu;

        let cpu_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage((100 / cpus_per_row) as u16); cpus_per_row])
            .split(row_chunks[row]);

        for (i, chunk) in cpu_chunks.iter().take(num_cpus_this_row).enumerate() {
            let cpu_index = start_cpu + i;
            let cpu_usage = graph_data.get_cpu_infos().get(cpu_index).map_or(0.0, |info| info.usage);

            // Create a vertical bar using Unicode box-drawing characters
            let bar_height = ((cpu_usage / 100.0) * 8.0).round() as usize;
            let bar = "█".repeat(bar_height);
            let empty = "░".repeat(8 - bar_height);
            let vertical_bar = format!("{}{}", bar, empty);

            let label = format!("{:>2} [{:>3}%]", cpu_index, cpu_usage as u16);
            let text = vec![
                Line::from(vec![
                    Span::styled(label, Style::default().fg(RatatuiColor::Black)),
                ]),
                Line::from(vec![
                    Span::styled(vertical_bar, Style::default().fg(get_usage_color(cpu_usage)))
                ])
            ];

            let cpu_widget = Paragraph::new(text)
                .alignment(Alignment::Left);

            frame.render_widget(cpu_widget, *chunk);
        }
    }
}

fn render_memory_bars(
    frame: &mut ratatui::Frame,
    mem_area: Rect,
    swap_area: Rect,
    _graph_data: &GraphData
) {
    // Calculate memory usage
    let (mem_total, mem_used, _mem_free, _mem_cached, _mem_available) = get_memory_info();
    let memory_percentage = if mem_total > 0 {
        (((mem_used as f64 / mem_total as f64) * 100.0).min(100.0)) as u16
    } else {
        0
    };

    // Memory bar with compact format
    let memory_gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(get_usage_color(memory_percentage as f32)))
        .percent(memory_percentage)
        .label(format!("Mem [{:>4}M/{:>4}M]", mem_used / 1024, mem_total / 1024));

    // Swap bar (reading from /proc/swaps)

    let (swap_used, swap_total) = get_swap_info();
    // let swap_percentage = if swap_total > 0 {
    //     ((swap_used as f64 / swap_total as f64) * 100.0) as u16
    // } else {
    //     0
    // };
    
    //fixes the panic that happens when screen is not full
    let swap_percentage = if swap_total > 0 && swap_used <= swap_total {
        let ratio = swap_used as f64 / swap_total as f64;
        let percent = (ratio * 100.0).clamp(0.0, 100.0).round();
        percent as u16
    } else {
        0
    };
    
    

    let swap_gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(get_usage_color(swap_percentage as f32)))
        .percent(swap_percentage)
        .label(format!("Swp [{:>4}M/{:>4}M]", swap_used, swap_total));

    frame.render_widget(memory_gauge, mem_area);
    frame.render_widget(swap_gauge, swap_area);
}

fn get_usage_color(usage: f32) -> RatatuiColor {
    match usage as u16 {
        0..=50 => RatatuiColor::Green,
        51..=75 => RatatuiColor::Yellow,
        76..=90 => RatatuiColor::Red,
        _ => RatatuiColor::LightRed,
    }
}

fn get_swap_info() -> (u64, u64) {
    if let Ok(swaps) = std::fs::read_to_string("/proc/swaps") {
        if let Some(swap_line) = swaps.lines().nth(1) {
            let parts: Vec<&str> = swap_line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let (Ok(total), Ok(used)) = (
                    parts[2].parse::<u64>(),
                    parts[3].parse::<u64>(),
                ) {
                    return (used / 1024, total / 1024);  // Convert KB to MB
                }
            }
        }
    }
    (0, 0)
}

pub fn render_overview_tab(frame: &mut ratatui::Frame, area: Rect, graph_data: &GraphData, process_list: &[ProcessInfo]) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(7),   // System Overview
            ratatui::layout::Constraint::Length(6),   // CPU Summary
            ratatui::layout::Constraint::Length(5),   // Memory Summary
            ratatui::layout::Constraint::Length(6),   // Disk Summary (increased from 4 to 6)
            ratatui::layout::Constraint::Length(4),   // Process States
            ratatui::layout::Constraint::Min(1),      // Spacer
        ])
        .split(area);

    // System Overview
    let (boot_time, last_reboot) = get_boot_time();
    let hostname = hostname::get().unwrap_or_default().to_string_lossy().to_string();
    let os_info = get_os_info();
    let kernel_version = std::fs::read_to_string("/proc/version").unwrap_or_default();
    let uptime = get_system_uptime();
    let sys_overview = vec![
        Line::from(vec![Span::styled("System Overview", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Hostname: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&hostname, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("OS: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&os_info, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Kernel: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&kernel_version, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Boot Time: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&boot_time, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Last Reboot: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&last_reboot, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Uptime: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&uptime, Style::default().fg(RatatuiColor::Black))]),
    ];
    let sys_overview_widget = Paragraph::new(sys_overview).block(Block::default().borders(Borders::ALL)).style(Style::default());
    frame.render_widget(sys_overview_widget, chunks[0]);

    // CPU Summary
    let (cpu_model, _, _) = get_cpu_details();
    let load_avg = get_load_average();
    let total_cpu: f32 = graph_data.get_cpu_history().iter().sum();
    let cpu_summary = vec![
        Line::from(vec![Span::styled("CPU Summary", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Model: ", Style::default().fg(RatatuiColor::Black)), Span::styled(&cpu_model, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Cores: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} (Physical)", get_cpu_count()), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Load Avg: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.2}, {:.2}, {:.2}", load_avg.0, load_avg.1, load_avg.2), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Total CPU Usage: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1}%", total_cpu), get_usage_style(total_cpu as f64))]),
    ];
    let cpu_summary_widget = Paragraph::new(cpu_summary).block(Block::default().borders(Borders::ALL)).style(Style::default());
    frame.render_widget(cpu_summary_widget, chunks[1]);

    // Memory Summary
    let (mem_total, mem_used, mem_free, mem_cached, _mem_available) = get_memory_info();
    let mem_summary = vec![
        Line::from(vec![Span::styled("Memory Summary", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_total / 1024), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Used: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_used / 1024), get_usage_style((mem_used as f64 / mem_total as f64) * 100.0))]),
        Line::from(vec![Span::styled("Free: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_free / 1024), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Cached+Buffers: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_cached / 1024), Style::default().fg(RatatuiColor::Black))]),
    ];
    let mem_summary_widget = Paragraph::new(mem_summary).block(Block::default().borders(Borders::ALL)).style(Style::default());
    frame.render_widget(mem_summary_widget, chunks[2]);

    // Disk Summary
    let (disk_total, disk_used) = get_disk_stats();
    let disk_total_gb = disk_total as f64 / 1024.0 ;
    let disk_used_gb = disk_used as f64 / 1024.0 ;
    let disk_free_gb = (disk_total.saturating_sub(disk_used)) as f64 / 1024.0;
    let disk_summary = vec![
        Line::from(vec![Span::styled("Disk Summary", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total (GB): ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1} GB", disk_total_gb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Used (GB): ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1} GB", disk_used_gb), get_usage_style((disk_used as f64 / disk_total.max(1) as f64) * 100.0))]),
        Line::from(vec![Span::styled("Free (GB): ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1} GB", disk_free_gb), Style::default().fg(RatatuiColor::Black))]),
    ];
    let disk_summary_widget = Paragraph::new(disk_summary).block(Block::default().borders(Borders::ALL)).style(Style::default());
    frame.render_widget(disk_summary_widget, chunks[3]);

    // Process States
    let state_counts = get_process_state_counts_from_status(process_list);
    let process_states = vec![
        Line::from(vec![Span::styled("Process States", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![
            Span::styled("Running: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Running").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Runnable: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Runnable").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Sleeping: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Sleeping").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Uninterruptible: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Uninterruptible").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Stopped: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Stopped").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Zombie: ", Style::default().fg(RatatuiColor::Black)), Span::styled(state_counts.get("Zombie").unwrap_or(&0).to_string(), Style::default().fg(RatatuiColor::Black)),
            Span::raw(" | "),
            Span::styled("Total: ", Style::default().fg(RatatuiColor::Black)), Span::styled(process_list.len().to_string(), Style::default().fg(RatatuiColor::Black)),
        ]),
    ];
    let process_states_widget = Paragraph::new(process_states).block(Block::default().borders(Borders::ALL)).style(Style::default());
    frame.render_widget(process_states_widget, chunks[4]);
}

pub fn render_cpu_tab(frame: &mut ratatui::Frame, area: Rect, graph_data: &GraphData) {
    // Gather CPU details
    let (model, freq, cache) = get_cpu_details();
    let cpu_count = get_cpu_count();
    let temp = get_cpu_temp();
    let per_core_freqs = get_per_core_freq();
    let (ctxt, _processes, procs_running, procs_blocked, interrupts) = get_cpu_stats();
    let load_avg = get_load_average();

    // Per-core usage (from GraphData)
    let per_core_usages: Vec<f32> = graph_data.get_cpu_infos().iter().map(|c| c.usage).collect();

    // Compose lines for the CPU Info tab
    let mut lines = vec![
        Line::from(vec![Span::styled("CPU Information", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Model: ", Style::default().fg(RatatuiColor::Black)), Span::styled(model, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Frequency: ", Style::default().fg(RatatuiColor::Black)), Span::styled(freq, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Cache: ", Style::default().fg(RatatuiColor::Black)), Span::styled(cache, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Cores: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", cpu_count), Style::default().fg(RatatuiColor::Black))]),
    ];
    if let Some(temp) = temp {
        lines.push(Line::from(vec![Span::styled("Temperature: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1} °C", temp), Style::default().fg(RatatuiColor::Black))]));
    }
    // Add total CPU usage line using /proc/stat aggregate
    let total_cpu = get_total_cpu_usage();
    lines.push(Line::from(vec![Span::styled("Total CPU Usage: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.1}%", total_cpu), get_usage_style(total_cpu as f64))]));
    lines.push(Line::from(vec![Span::styled("Context Switches: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", ctxt), Style::default().fg(RatatuiColor::Black))]));
    lines.push(Line::from(vec![Span::styled("Interrupts: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", interrupts), Style::default().fg(RatatuiColor::Black))]));
    lines.push(Line::from(vec![Span::styled("Running Procs: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", procs_running), Style::default().fg(RatatuiColor::Black)), Span::raw(" | "), Span::styled("Blocked: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", procs_blocked), Style::default().fg(RatatuiColor::Black))]));
    lines.push(Line::from(vec![Span::styled("Load Avg: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.2}, {:.2}, {:.2}", load_avg.0, load_avg.1, load_avg.2), Style::default().fg(RatatuiColor::Black))]));
    lines.push(Line::from(vec![Span::styled("", Style::default())]));
    lines.push(Line::from(vec![Span::styled("Per-Core Usage:", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]));
    for (i, usage) in per_core_usages.iter().enumerate() {
        let freq_str = per_core_freqs.get(i).map(|f| format!(" @ {:.0} MHz", f)).unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!("Core {:2}: ", i), Style::default().fg(RatatuiColor::Black)),
            Span::styled(format!("{:5.1}%", usage), get_usage_style(*usage as f64)),
            Span::styled(freq_str, Style::default().fg(RatatuiColor::Black)),
        ]));
    }
    let widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("CPU Info").style(Style::default().fg(RatatuiColor::Black))).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(widget, area);
}

pub fn render_memory_tab(frame: &mut ratatui::Frame, area: Rect) {
    let (mem_total, mem_used, mem_free, mem_cached, _mem_available) = get_memory_info();
    let (swap_used, swap_total) = get_swap_info();
    // Read more details from /proc/meminfo
    let mut available = 0;
    let mut buffers = 0;
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemAvailable:") {
                available = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            } else if line.starts_with("Buffers:") {
                buffers = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            }
        }
    }
    let mem_total_mb = mem_total / 1024;
    let mem_used_mb = mem_used / 1024;
    let mem_free_mb = mem_free / 1024;
    let mem_cached_mb = mem_cached / 1024;
    let mem_available_mb = available / 1024;
    let mem_buffers_mb = buffers / 1024;
    let swap_free = swap_total.saturating_sub(swap_used);
    let mem_usage_percent = if mem_total > 0 { (mem_used as f64 / mem_total as f64) * 100.0 } else { 0.0 };
    let swap_usage_percent = if swap_total > 0 { (swap_used as f64 / swap_total as f64) * 100.0 } else { 0.0 };
    let lines = vec![
        Line::from(vec![Span::styled("Memory Information", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("", Style::default())]),
        Line::from(vec![Span::styled("-- RAM --", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_total_mb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Used: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB ({:.1}%)", mem_used_mb, mem_usage_percent), get_usage_style(mem_usage_percent))]),
        Line::from(vec![Span::styled("Free: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_free_mb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Available: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_available_mb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Cached: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_cached_mb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Buffers: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", mem_buffers_mb), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("", Style::default())]),
        Line::from(vec![Span::styled("-- SWAP --", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", swap_total), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Used: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB ({:.1}%)", swap_used, swap_usage_percent), get_usage_style(swap_usage_percent))]),
        Line::from(vec![Span::styled("Free: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", swap_free), Style::default().fg(RatatuiColor::Black))]),
    ];
    let widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Memory Info").style(Style::default().fg(RatatuiColor::Black)));
    frame.render_widget(widget, area);
}

pub fn render_disk_tab(frame: &mut ratatui::Frame, area: Rect) {
    let (disk_total, disk_used) = get_disk_stats();
    let disk_free = disk_total.saturating_sub(disk_used);
    // Try to get disk read/write speeds and storage type
    let (read_speed, write_speed) = get_disk_rw_speed();
    let storage_type = get_storage_type();
    let read_speed_str = if read_speed > 0.0 { format!("{:.1} MB/s", read_speed) } else { "Unavailable".to_string() };
    let write_speed_str = if write_speed > 0.0 { format!("{:.1} MB/s", write_speed) } else { "Unavailable".to_string() };
    let lines = vec![
        Line::from(vec![Span::styled("Disk Information", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", disk_total), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Used: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", disk_used), get_usage_style((disk_used as f64 / disk_total.max(1) as f64) * 100.0))]),
        Line::from(vec![Span::styled("Free: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{} MB", disk_free), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Read Speed: ", Style::default().fg(RatatuiColor::Black)), Span::styled(read_speed_str, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Write Speed: ", Style::default().fg(RatatuiColor::Black)), Span::styled(write_speed_str, Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Storage Type: ", Style::default().fg(RatatuiColor::Black)), Span::styled(storage_type, Style::default().fg(RatatuiColor::Black))]),
    ];
    let widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Disk Info").style(Style::default().fg(RatatuiColor::Black)));
    frame.render_widget(widget, area);
}

pub fn render_processes_tab(frame: &mut ratatui::Frame, area: Rect, process_list: &[ProcessInfo]) {
    let total_processes = process_list.len();
    let state_counts = get_process_state_counts_from_status(process_list);
    let mut lines = vec![
        Line::from(vec![Span::styled("Processes Overview", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Total Processes: ", Style::default().fg(RatatuiColor::Black)), Span::styled(total_processes.to_string(), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("States: ", Style::default().fg(RatatuiColor::Black)),
            Span::styled(format!("Running: {}  ", state_counts.get("Running").unwrap_or(&0)), Style::default().fg(RatatuiColor::Green)),
            Span::styled(format!("Sleeping: {}  ", state_counts.get("Sleeping").unwrap_or(&0)), Style::default().fg(RatatuiColor::Blue)),
            Span::styled(format!("Stopped: {}  ", state_counts.get("Stopped").unwrap_or(&0)), Style::default().fg(RatatuiColor::Yellow)),
            Span::styled(format!("Zombie: {}", state_counts.get("Zombie").unwrap_or(&0)), Style::default().fg(RatatuiColor::Red)),
        ]),
        Line::from(vec![Span::styled("", Style::default())]),
        Line::from(vec![Span::styled("Top Processes by CPU", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
    ];
    let mut sorted_by_cpu = process_list.iter().enumerate().collect::<Vec<(usize, &ProcessInfo)>>();
    sorted_by_cpu.sort_by(|a, b| b.1.cpu_usage.partial_cmp(&a.1.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
    for &(i, proc) in &sorted_by_cpu.iter().take(5).collect::<Vec<_>>() {
        lines.push(Line::from(vec![Span::styled(
            format!("{}. {} (PID {}) - CPU: {:.2}%", i + 1, proc.name, proc.pid, proc.cpu_usage),
            Style::default().fg(RatatuiColor::Yellow)
        )]));
    }
    lines.push(Line::from(vec![Span::styled("", Style::default())]));
    lines.push(Line::from(vec![Span::styled("Top Processes by Memory", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]));
    let mut sorted_by_mem = process_list.iter().enumerate().collect::<Vec<(usize, &ProcessInfo)>>();
    sorted_by_mem.sort_by(|a, b| b.1.memory_usage.partial_cmp(&a.1.memory_usage).unwrap_or(std::cmp::Ordering::Equal));
    for &(i, proc) in &sorted_by_mem.iter().take(5).collect::<Vec<_>>() {
        lines.push(Line::from(vec![Span::styled(
            format!("{}. {} (PID {}) - MEM: {:.2} MB", i + 1, proc.name, proc.pid, proc.memory_usage as f64 / 1024.0 / 1024.0),
            Style::default().fg(RatatuiColor::Blue)
        )]));
    }
    let widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Processes Info").style(Style::default().fg(RatatuiColor::Black)));
    frame.render_widget(widget, area);
}

pub fn render_advanced_tab(frame: &mut ratatui::Frame, area: Rect, _graph_data: &GraphData) {
    let (pgfault, pswpin, pswpout, iowait) = get_vm_stats();
    let (ctxt, processes, procs_running, procs_blocked, interrupts) = get_cpu_stats();
    // Advanced: CPU temperature and per-core frequency
    let cpu_temp = get_cpu_temp();
    let per_core_freqs = get_per_core_freq();
    let mut lines = vec![
        Line::from(vec![Span::styled("Advanced System Stats", Style::default().fg(RatatuiColor::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("Page Faults: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", pgfault), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Swap In: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", pswpin), Style::default().fg(RatatuiColor::Black)), Span::raw(" | "), Span::styled("Swap Out: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", pswpout), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("IO Wait: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", iowait), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Context Switches: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", ctxt), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Interrupts: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", interrupts), Style::default().fg(RatatuiColor::Black))]),
        Line::from(vec![Span::styled("Processes Since Boot Time: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", processes), Style::default().fg(RatatuiColor::Black)), Span::raw(" | "), Span::styled("Running: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", procs_running), Style::default().fg(RatatuiColor::Black)), Span::raw(" | "), Span::styled("Blocked: ", Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{}", procs_blocked), Style::default().fg(RatatuiColor::Black))]),
    ];
    // Add CPU temperature if available, else show Unavailable
    lines.push(Line::from(vec![Span::styled("CPU Temperature: ", Style::default().fg(RatatuiColor::Black)),
        Span::styled(match cpu_temp { Some(temp) => format!("{:.1} °C", temp), None => "Unavailable".to_string() }, Style::default().fg(RatatuiColor::Red))]));
    // Add per-core frequencies or Unavailable
    if !per_core_freqs.is_empty() {
        lines.push(Line::from(vec![Span::styled("Per-Core Frequency (MHz):", Style::default().fg(RatatuiColor::Cyan).add_modifier(Modifier::BOLD))]));
        for (i, freq) in per_core_freqs.iter().enumerate() {
            lines.push(Line::from(vec![Span::styled(format!("Core {:2}: ", i), Style::default().fg(RatatuiColor::Black)), Span::styled(format!("{:.0} MHz", freq), Style::default().fg(RatatuiColor::Black))]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled("Per-Core Frequency: ", Style::default().fg(RatatuiColor::Cyan)), Span::styled("Unavailable", Style::default().fg(RatatuiColor::Red))]));
    }
    let widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Advanced Info").style(Style::default().fg(RatatuiColor::Black)));
    frame.render_widget(widget, area);
}

fn render_cpu_graph(
    frame: &mut ratatui::Frame,
    area: Rect,
    graph_data: &GraphData,
) {
    let cpu_data: Vec<(f64, f64)> = graph_data
        .get_cpu_history()
        .iter()
        .enumerate()
        .map(|(i, &value)| (i as f64, value as f64))
        .collect();

    // Determine y-axis labels based on height
    let y_labels = if area.height > 15 {
        vec!["0%", "25%", "50%", "75%", "100%"]
    } else if area.height > 10 {
        vec!["0%", "50%", "100%"]
    } else {
        vec!["0%", "100%"]
    };

    let dataset = Dataset::default()
        .name("CPU Usage")
        .marker(ratatui::symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(RatatuiColor::Cyan))
        .data(&cpu_data);

    let chart = Chart::new(vec![dataset])
        .block(Block::default()
            .title("CPU Usage Over Time (%)").style(Style::default().fg(RatatuiColor::Black))
            .borders(Borders::ALL))
        .x_axis(ratatui::widgets::Axis::default()
            .bounds([0.0, graph_data.max_points as f64])
            .labels(vec![]))
        .y_axis(ratatui::widgets::Axis::default()
            .bounds([0.0, 100.0])
            .labels(y_labels
                .into_iter()
                .map(Span::from)
                .collect()));

    frame.render_widget(chart, area);
}

fn render_memory_graph(
    frame: &mut ratatui::Frame,
    area: Rect,
    graph_data: &GraphData,
) {
    let memory_data: Vec<(f64, f64)> = graph_data
        .get_memory_history()
        .iter()
        .enumerate()
        .map(|(i, &value)| (i as f64, value as f64)) // value is already in MB
        .collect();

    let max_memory = memory_data
        .iter()
        .map(|&(_, y)| y)
        .fold(100.0_f64, |a, b| a.max(b));

    let y_labels = if area.height > 15 {
        vec![
            format!("0 MB"),
            format!("{:.0} MB", max_memory / 4.0),
            format!("{:.0} MB", max_memory / 2.0),
            format!("{:.0} MB", max_memory * 3.0 / 4.0),
            format!("{:.0} MB", max_memory),
        ]
    } else if area.height > 10 {
        vec![
            format!("0 MB"),
            format!("{:.0} MB", max_memory / 2.0),
            format!("{:.0} MB", max_memory),
        ]
    } else {
        vec![
            format!("0 MB"),
            format!("{:.0} MB", max_memory),
        ]
    };

    let dataset = Dataset::default()
        .name("Memory Usage")
        .marker(ratatui::symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(RatatuiColor::Green))
        .data(&memory_data);

    let chart = Chart::new(vec![dataset])
        .block(Block::default()
            .title("Memory Usage Over Time (MB)").style(Style::default().fg(RatatuiColor::Black))
            .borders(Borders::ALL))
        .x_axis(ratatui::widgets::Axis::default()
            .bounds([0.0, graph_data.max_points as f64])
            .labels(vec![]))
        .y_axis(ratatui::widgets::Axis::default()
            .bounds([0.0, max_memory])
            .labels(y_labels
                .into_iter()
                .map(Span::from)
                .collect()));

    frame.render_widget(chart, area);
}


// Add these helper functions at the top level
// fn get_process_state_counts(processes: &[f32]) -> std::collections::HashMap<String, usize> {
//     let mut states = std::collections::HashMap::new();
//     for &usage in processes {
//         // Map usage to standard categories
//         let category = match usage {
//             u if u < 25.0 => "Low",
//             u if u < 50.0 => "Medium",
//             u if u < 75.0 => "High",
//             _ => "Very High"
//         };
//         *states.entry(category.to_string()).or_insert(0) += 1;
//     }
//     // Ensure all states exist in the map
//     for state in ["Low", "Medium", "High", "Very High"] {
//         states.entry(state.to_string()).or_insert(0);
//     }
    
//     states
// }
//warning

fn get_memory_info() -> (u64, u64, u64, u64, u64) { // Returns (total, used, free, cached, available) in KB
    let mut total = 0;
    let mut free = 0;
    let mut cached = 0;
    let mut buffers = 0;
    let mut available = 0;
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                total = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            } else if line.starts_with("MemFree:") {
                free = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            } else if line.starts_with("Cached:") {
                cached = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            } else if line.starts_with("Buffers:") {
                buffers = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                available = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
            }
        }
    }
    let used = if available > 0 {
        total - available
    } else {
        total - free - cached - buffers
    };
    (total, used, free, cached + buffers, available)
}

fn get_disk_stats() -> (u64, u64) { // Returns (total, used) in MB
    if let Ok(output) = std::process::Command::new("df")
        .arg("-BM")  // Force output in MB
        .arg("/")
        .output() {
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let lines: Vec<&str> = output_str.lines().collect();
            if lines.len() > 1 {
                let stats: Vec<&str> = lines[1].split_whitespace().collect();
                if stats.len() >= 3 {
                    let total: u64 = stats[1].trim_end_matches('M').parse().unwrap_or(0);
                    let used: u64 = stats[2].trim_end_matches('M').parse().unwrap_or(0);
                    return (total, used);
                }
            }
        }
    }
    (0, 0)
}

// Add these new helper functions
fn get_cpu_temp() -> Option<f64> {
    if let Ok(temp) = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        if let Ok(temp_val) = temp.trim().parse::<u32>() {
            return Some(temp_val as f64 / 1000.0);
        }
    }
    None
}

fn get_per_core_freq() -> Vec<f64> {
    let mut freqs = Vec::new();
    let cpu_count = get_cpu_count();
    
    for i in 0..cpu_count {
        if let Ok(freq) = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i)) {
            if let Ok(freq_val) = freq.trim().parse::<u32>() {
                freqs.push(freq_val as f64 / 1000.0); // Convert to MHz
            }
        }
    }
    freqs
}

fn get_cpu_stats() -> (u64, u64, u64, u64, u64) { // Returns (ctxt, processes, procs_running, procs_blocked, interrupts)
    let mut ctxt = 0;
    let mut processes = 0;
    let mut procs_running = 0;
    let mut procs_blocked = 0;
    let mut interrupts = 0;

    if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
        for line in stat.lines() {
            match line.split_whitespace().next() {
                Some("ctxt") => {
                    ctxt = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("processes") => {
                    processes = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("procs_running") => {
                    procs_running = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("procs_blocked") => {
                    procs_blocked = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("intr") => {
                    interrupts = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    (ctxt, processes, procs_running, procs_blocked, interrupts)
}

fn get_vm_stats() -> (u64, u64, u64, u64) { // Returns (page_faults, swap_in, swap_out, io_wait)
    let mut pgfault = 0;
    let mut pswpin = 0;
    let mut pswpout = 0;
    let mut iowait = 0;

    if let Ok(vmstat) = std::fs::read_to_string("/proc/vmstat") {
        for line in vmstat.lines() {
            match line.split_whitespace().next() {
                Some("pgfault") => {
                    pgfault = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("pswpin") => {
                    pswpin = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                Some("pswpout") => {
                    pswpout = line.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
                }
                _ => {}
            }
        }
    }

    // Get IO wait from /proc/stat
    if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
        if let Some(cpu_line) = stat.lines().next() {
            let values: Vec<u64> = cpu_line.split_whitespace()
                .skip(1)
                .filter_map(|val| val.parse().ok())
                .collect();
            if values.len() >= 6 {
                iowait = values[4]; // iowait is the 5th value
            }
        }
    }

    (pgfault, pswpin, pswpout, iowait)
}

fn get_boot_time() -> (String, String) { // Returns (boot_time, last_reboot)
    let mut boot_time = String::from("Unknown");
    let mut last_reboot = String::from("Unknown");

    if let Ok(uptime) = std::fs::read_to_string("/proc/uptime") {
        if let Some(secs_str) = uptime.split_whitespace().next() {
            if let Ok(secs) = secs_str.parse::<f64>() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as f64;
                let boot_timestamp = now - secs;
                
                // Format boot time using DateTime::from_timestamp
                let datetime = chrono::DateTime::from_timestamp(boot_timestamp as i64, 0)
                    .unwrap_or_default()
                    .naive_local();
                boot_time = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                
                // Try to get last reboot from wtmp (if available)
                if let Ok(output) = std::process::Command::new("last")
                    .arg("-x")
                    .arg("reboot")
                    .arg("-F")
                    .output() {
                    if let Ok(output_str) = String::from_utf8(output.stdout) {
                        if let Some(last_reboot_line) = output_str.lines().next() {
                            last_reboot = last_reboot_line.to_string();
                        }
                    }
                }
            }
        }
    }
    (boot_time, last_reboot)
}

fn get_cpu_count() -> usize {
    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        return cpuinfo.lines().filter(|line| line.starts_with("processor")).count();
    }
    1
}
fn get_os_info() -> String {
    std::fs::read_to_string("/etc/os-release")
        .map(|content| {
            let mut name = String::new();
            for line in content.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    name = line.split('=').nth(1).unwrap_or("").trim_matches('"').to_string();
                }
            }
            if !name.is_empty() { name } else { "Unknown".to_string() }
        })
        .unwrap_or_else(|_| "Unknown".to_string())
}
fn get_system_uptime() -> String {
    if let Ok(uptime) = std::fs::read_to_string("/proc/uptime") {
        if let Some(secs_str) = uptime.split_whitespace().next() {
            if let Ok(secs) = secs_str.parse::<f64>() {
                let days = (secs / 86400.0) as u64;
                let hours = ((secs % 86400.0) / 3600.0) as u64;
                let minutes = ((secs % 3600.0) / 60.0) as u64;
                return format!("{}d {}h {}m", days, hours, minutes);
            }
        }
    }
    "Unknown".to_string()
}
fn get_cpu_details() -> (String, String, String) {
    let mut model = String::new();
    let mut freq = String::new();
    let mut cache = String::new();
    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("model name") {
                model = line.split(':').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("cpu MHz") {
                freq = format!("{:.2} MHz", line.split(':').nth(1).unwrap_or("0").trim().parse::<f64>().unwrap_or(0.0));
            } else if line.starts_with("cache size") {
                cache = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
        }
    }
    (model, freq, cache)
}
fn get_load_average() -> (f64, f64, f64) {
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        let values: Vec<f64> = loadavg.split_whitespace().take(3).filter_map(|s| s.parse().ok()).collect();
        if values.len() == 3 {
            return (values[0], values[1], values[2]);
        }
    }
    (0.0, 0.0, 0.0)
}
fn get_usage_style(usage: f64) -> ratatui::style::Style {
    use ratatui::style::Color as RatatuiColor;
    match usage {
        u if u > 90.0 => ratatui::style::Style::default().fg(RatatuiColor::Red),
        u if u > 70.0 => ratatui::style::Style::default().fg(RatatuiColor::Yellow),
        _ => ratatui::style::Style::default().fg(RatatuiColor::Green),
    }
}

// Helper: Simulate or get disk read/write speeds (MB/s)
fn get_disk_rw_speed() -> (f64, f64) {
    #[cfg(target_os = "linux")]
    {
        // use std::sync::Mutex; delete after debugging
        use std::time::Instant;
        static mut LAST_READ: Option<(u64, u64, Instant)> = None;
        let mut read_bytes = 0u64;
        let mut write_bytes = 0u64;
        if let Ok(stats) = std::fs::read_to_string("/proc/diskstats") {
            for line in stats.lines() {
                if line.contains(" sda ") || line.contains(" vda ") || line.contains(" nvme0n1 ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() > 9 {
                        let sectors_read: u64 = parts[5].parse().unwrap_or(0);
                        let sectors_written: u64 = parts[9].parse().unwrap_or(0);
                        // Assume 512 bytes per sector
                        read_bytes = sectors_read * 512;
                        write_bytes = sectors_written * 512;
                    }
                }
            }
        }
        let now = Instant::now();
        unsafe {
            if let Some((last_read, last_write, last_time)) = LAST_READ {
                let dt = now.duration_since(last_time).as_secs_f64().max(0.1);
                let read_speed = (read_bytes.saturating_sub(last_read)) as f64 / 1_048_576.0 / dt;
                let write_speed = (write_bytes.saturating_sub(last_write)) as f64 / 1_048_576.0 / dt;
                LAST_READ = Some((read_bytes, write_bytes, now));
                (read_speed, write_speed)
            } else {
                LAST_READ = Some((read_bytes, write_bytes, now));
                (0.0, 0.0)
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Simulate values for non-Linux
        (0.0, 0.0)
    }
}

// Helper: Get storage type (filesystem)
fn get_storage_type() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() > 2 && parts[1] == "/" {
                    return parts[2].to_string(); // Filesystem type
                }
            }
        }
        "Unknown".to_string()
    }
    #[cfg(not(target_os = "linux"))]
    {
        "WSL/Unknown".to_string()
    }
}

// Helper to count process states from status
fn get_process_state_counts_from_status(processes: &[ProcessInfo]) -> std::collections::HashMap<String, usize> {
    let mut states = std::collections::HashMap::new();
    for proc in processes {
        let status = proc.status.trim().to_lowercase();
        let category = if status.contains("run") {
            "Running"
        } else if status.contains("sleep") {
            "Sleeping"
        } else if status.contains("stop") {
            "Stopped"
        } else if status.contains("zomb") {
            "Zombie"
        } else {
            "Other"
        };
        *states.entry(category.to_string()).or_insert(0) += 1;
    }
    for state in ["Running", "Sleeping", "Stopped", "Zombie", "Other"] {
        states.entry(state.to_string()).or_insert(0);
    }
    states
}

// Add this function to get total CPU usage like htop/top
fn get_total_cpu_usage() -> f32 {
    use std::sync::OnceLock;
    static LAST_TOTAL: OnceLock<std::sync::Mutex<(u64, u64)>> = OnceLock::new();
    let last_idle;
    let last_total;
    {
        let lock = LAST_TOTAL.get_or_init(|| std::sync::Mutex::new((0, 0)));
        let (li, lt) = *lock.lock().unwrap();
        last_idle = li;
        last_total = lt;
    }
    if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
        if let Some(line) = stat.lines().next() {
            let values: Vec<u64> = line.split_whitespace()
                .skip(1)
                .filter_map(|val| val.parse().ok())
                .collect();
            if values.len() >= 4 {
                let idle = values[3];
                let total: u64 = values.iter().sum();
                let idle_delta = idle - last_idle;
                let total_delta = total - last_total;
                // Update static for next call
                let lock = LAST_TOTAL.get_or_init(|| std::sync::Mutex::new((0, 0)));
                *lock.lock().unwrap() = (idle, total);
                if total_delta > 0 {
                    return 100.0 * (1.0 - (idle_delta as f32 / total_delta as f32));
                }
            }
        }
    }
    0.0
}
