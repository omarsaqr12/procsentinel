use crate::process;
use crate::scripting_rules::RuleEngine;
use crate::graph;
use std::io::stdout;
use std::thread::sleep;
use std::time::Duration;
use process::ProcessManager;
use std::error::Error;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{ disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};

use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Table, Row, Cell,
        Dataset, GraphType, Chart, BorderType,
    },
    layout::{Layout, Constraint, Direction, Alignment},
    style::{Style, Modifier, Color},
    text::{Line, Span},
    Frame,
};

use crate::process_log::{ProcessExitLogEntry, render_process_log_tab};
use chrono::Local;
use std::collections::{HashSet, VecDeque};

// ViewMode enum to track current view
#[derive(PartialEq)]
enum ViewMode {
    ProcessList,
    Statistics,  // Renamed from GraphView
    FilterSort,
    Sort,
    Filter,
    FilterInput,
    KillStop,
    ChangeNice,
    PerProcessGraph, // Added for new feature
    ProcessLog,      // Added for new feature
    Help,            // Added for new feature
    RuleInput,
    GroupedView,     // Added for container/cgroup grouping
    ContainerDetail, // Detailed container view
    NamespaceDetail, // Detailed namespace view
    Scheduler,       // Job scheduler view
    StartProcess,    // Start new process view
    AdvancedFilter,  // Advanced filter input
    ProfileManagement, // Profile management view
    ProfileEditor,     // Profile editing view
    AlertManagement, // Alert management view
    AlertEditor,     // Alert editing view
    CheckpointManagement, // CRIU checkpoint management view
    MultiHost, // Multi-host view
    HostManagement, // Host management view
    TaskEditor, // Task editor view for creating/editing scheduled tasks
}

// Input state for various operations
struct InputState {
    pid_input: String,
    nice_input: String,
    filter_input: String,
    rule_input: String,
    message: Option<(String, bool)>, // (message, is_error)
    message_timeout: Option<std::time::Instant>,
    // Start process input
    program_path: String,
    working_dir: String,
    arguments: String,
    env_vars: Vec<(String, String)>, // (key, value)
    current_start_input_field: usize, // 0=program, 1=working_dir, 2=arguments, 3=env_vars
    // Advanced filter input
    advanced_filter_input: String,
    // Task editor input
    task_name: String,
    task_schedule_type: String, // "cron", "interval", or "once"
    task_schedule_value: String, // Cron expression, interval seconds, or timestamp
    task_action_type: String, // "restart", "cleanup", or "rule"
    task_action_value: String, // Process pattern, cleanup params, or rule expression
    current_task_field: usize, // 0=name, 1=schedule_type, 2=schedule_value, 3=action_type, 4=action_value
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            pid_input: String::new(),
            nice_input: String::new(),
            filter_input: String::new(),
            rule_input: String::new(),
            message: None,
            message_timeout: None,
            program_path: String::new(),
            working_dir: String::new(),
            arguments: String::new(),
            env_vars: Vec::new(),
            current_start_input_field: 0,
            advanced_filter_input: String::new(),
            task_name: String::new(),
            task_schedule_type: String::new(),
            task_schedule_value: String::new(),
            task_action_type: String::new(),
            task_action_value: String::new(),
            current_task_field: 0,
        }
    }
}

// NiceInputState enum to track the state of nice value input
#[derive(PartialEq)]
enum NiceInputState {
    SelectingPid,
    EnteringNice,
}
// KillStopInputState enum to track the state of kill/stop/continue input
#[derive(PartialEq, Clone)]
enum KillStopInputState {
    SelectingPid,
    EnteringAction,
    ConfirmingAction {
        pid: u32,
        process_name: String,
        action_type: String, // "kill", "stop", "terminate", "continue"
    },
    DependencyWarning {
        pid: u32,
        process_name: String,
        action_type: String,
        child_count: usize,
        children: Vec<(u32, String)>, // (pid, name)
    },
    ConfirmingBatchAction {
        pids: Vec<u32>,
        process_names: Vec<String>,
        action_type: String,
    },
}

// StatisticsTab enum to track the current statistics tab
#[derive(PartialEq)]
#[allow(dead_code)]
pub enum StatisticsTab {
    Graphs,
    Overview,
    CPU,
    Memory,
    PerProcessGraph, // New tab for per-process graphing
    ProcessLog,      // New tab for process logging
    Disk,
    Processes,
    Advanced,
    Help,            // New tab for help
}

// LogGroupMode enum to track process log grouping
#[derive(PartialEq, Clone, Copy)]
enum LogGroupMode {
    None,
    Name,
    PPID,
    User,
}

// App state
struct App {
    process_manager: ProcessManager,
    graph_data: graph::GraphData,
    view_mode: ViewMode,
    scroll_offset: usize,
    display_limit: usize,
    input_state: InputState,
    sort_ascending: bool,
    sort_mode: Option<String>,
    filter_mode: Option<String>,
    stats_scroll_offset: usize,  // New field for statistics scrolling
    nice_input_state: NiceInputState,  // Track which input we're currently handling
    current_stats_tab: StatisticsTab,  // New field for tracking current statistics tab
    change_nice_scroll_offset: usize,
    selected_process_index: usize,
    per_process_graph_scroll_offset: usize,  // Add this
    selected_process_for_graph: Option<u32>,  // Add this
    kill_stop_input_state: KillStopInputState,
    process_exit_log: VecDeque<ProcessExitLogEntry>, // Add this
    prev_pids: std::collections::HashMap<u32, String>, // For tracking exited processes with names
    process_first_seen: std::collections::HashMap<u32, std::time::Instant>, // Track when we first saw each process
    log_filter_input: String, // For process log search/filter
    log_filter_active: bool,  // True if in filter input mode
    log_scroll_offset: usize, // For scrolling the process log
    log_group_mode: LogGroupMode, // For grouping process log
    pub rule_engine: RuleEngine, //for scripting
    // Grouped view state
    grouped_view_type: crate::process_group::GroupType, // Current grouping type
    selected_group_index: usize, // Selected group in grouped view
    expanded_groups: HashSet<String>, // Set of expanded group IDs
    grouped_view_scroll_offset: usize, // Scroll offset for grouped view
    current_namespace_type: Option<String>, // Current namespace type if grouping by namespace
    frozen_group_order: Vec<String>, // Frozen group order to prevent jumping when expanded
    group_view_frozen: bool, // Whether group order is frozen
    selected_container_id: Option<String>, // Selected container for detail view
    selected_namespace: Option<(String, u64)>, // Selected namespace (type, id) for detail view
    detail_view_scroll_offset: usize, // Scroll offset for detail view
    // Scheduler state
    scheduler: crate::scheduler::Scheduler,
    selected_task_index: usize, // Selected task in scheduler view
    scheduler_scroll_offset: usize, // Scroll offset for scheduler view
    scheduler_last_check: std::time::Instant, // Last time we checked for due tasks
    // Profile management
    profile_manager: crate::profile::ProfileManager,
    selected_profile_index: usize,
    profile_scroll_offset: usize,
    profile_edit_mode: bool, // True when editing a profile
    profile_edit_name: String,
    profile_edit_prioritize: String,
    profile_edit_hide: String,
    profile_edit_nice: String,
    profile_edit_current_field: usize, // 0=prioritize, 1=hide, 2=nice
    // Multi-select state
    multi_select_mode: bool,
    selected_processes: HashSet<u32>,
    // Alert management
    alert_manager: crate::alert::AlertManager,
    selected_alert_index: usize,
    alert_scroll_offset: usize,
    alert_edit_mode: bool,
    alert_edit_name: String,
    alert_edit_threshold: String,
    alert_edit_duration: String,
    alert_edit_current_field: usize, // 0=Name, 1=Threshold, 2=Duration
    // CRIU checkpoint management
    criu_manager: crate::criu_manager::CriuManager,
    selected_checkpoint_index: usize,
    checkpoint_scroll_offset: usize,
    // Multi-host coordination
    coordinator: crate::coordinator::Coordinator,
    multi_host_mode: bool,
    selected_host_index: usize,
    host_scroll_offset: usize,
    host_input: String,
    last_process_refresh: std::time::Instant,
}

impl App {
    fn new() -> Self {
        Self {
            process_manager: ProcessManager::new(),
            graph_data: graph::GraphData::new(60, 500),
            rule_engine: RuleEngine::new(),
            view_mode: ViewMode::ProcessList,
            scroll_offset: 0,
            display_limit: 20,
            input_state: InputState::default(),
            sort_ascending: true,
            sort_mode: Some("pid".to_string()),
            filter_mode: None,
            stats_scroll_offset: 0,  // Initialize stats scroll offset
            nice_input_state: NiceInputState::SelectingPid,
            current_stats_tab: StatisticsTab::Graphs,  // Default to Graphs tab
            change_nice_scroll_offset: 0,
            selected_process_index: 0,
            per_process_graph_scroll_offset: 0,  // Add this
            selected_process_for_graph: None,    // Add this
            kill_stop_input_state: KillStopInputState::SelectingPid,
            process_exit_log: VecDeque::with_capacity(100), // Keep last 100 exits
            prev_pids: std::collections::HashMap::new(),
            process_first_seen: std::collections::HashMap::new(), // Track when processes were first seen
            log_filter_input: String::new(),
            log_filter_active: false,
            log_scroll_offset: 0,
            log_group_mode: LogGroupMode::None,
            grouped_view_type: crate::process_group::GroupType::Cgroup,
            selected_group_index: 0,
            expanded_groups: HashSet::new(),
            grouped_view_scroll_offset: 0,
            current_namespace_type: None,
            frozen_group_order: Vec::new(),
            group_view_frozen: false,
            selected_container_id: None,
            selected_namespace: None,
            detail_view_scroll_offset: 0,
            scheduler: {
                let mut sched = crate::scheduler::Scheduler::new();
                // Load tasks from config
                let tasks = crate::scheduler::load_tasks();
                for task in tasks {
                    sched.add_task(task);
                }
                sched
            },
            selected_task_index: 0,
            scheduler_scroll_offset: 0,
            scheduler_last_check: std::time::Instant::now(),
            multi_select_mode: false,
            selected_processes: HashSet::new(),
            profile_manager: crate::profile::ProfileManager::new(),
            selected_profile_index: 0,
            profile_scroll_offset: 0,
            profile_edit_mode: false,
            profile_edit_name: String::new(),
            profile_edit_prioritize: String::new(),
            profile_edit_hide: String::new(),
            profile_edit_nice: String::new(),
            profile_edit_current_field: 0,
            alert_manager: crate::alert::AlertManager::new(),
            selected_alert_index: 0,
            alert_scroll_offset: 0,
            alert_edit_mode: false,
            alert_edit_name: String::new(),
            alert_edit_threshold: String::new(),
            alert_edit_duration: String::new(),
            alert_edit_current_field: 0,
            criu_manager: crate::criu_manager::CriuManager::new(),
            selected_checkpoint_index: 0,
            checkpoint_scroll_offset: 0,
            coordinator: crate::coordinator::Coordinator::new(),
            multi_host_mode: false,
            selected_host_index: 0,
            host_scroll_offset: 0,
            host_input: String::new(),
            last_process_refresh: std::time::Instant::now(),
        }
    }

    fn refresh(&mut self) {
        // Throttle process updates to once per second
        if self.last_process_refresh.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_process_refresh = std::time::Instant::now();

        let prev_map: std::collections::HashMap<u32, process::ProcessInfo> = self.process_manager.get_processes().iter().map(|p| (p.pid, p.clone())).collect();
        let prev_pids = self.prev_pids.clone();
        self.process_manager.refresh();
        
        // Apply profile-based prioritization if active
        if let Some(_profile_name) = self.profile_manager.get_active_profile() {
            let profile_mgr = &self.profile_manager;
            // Prioritize
            self.process_manager.apply_prioritization(|name| {
                profile_mgr.is_process_prioritized(name)
            });
            // Apply nice values (persistent enforcement)
            self.process_manager.apply_nice_adjustments(|name| {
                profile_mgr.get_nice_adjustment(name)
            });
        }
        
        self.graph_data.update(&self.process_manager);
        let current: Vec<_> = self.process_manager.get_processes().iter().map(|p| p.pid).collect();
        let current_set: HashSet<u32> = current.iter().copied().collect();
        
        // Track newly seen processes
        for pid in &current_set {
            if !prev_pids.contains_key(pid) {
                self.process_first_seen.insert(*pid, std::time::Instant::now());
            }
        }
        
        // Find exited PIDs
        for (pid, _name) in &prev_pids {
            if !current_set.contains(pid) {
                if let Some(proc) = prev_map.get(pid) {
                    let exit_time = Local::now();
                // Calculate uptime based on when we first saw the process
                let uptime_secs = if let Some(first_seen) = self.process_first_seen.get(pid) {
                    first_seen.elapsed().as_secs()
                } else {
                    // Fallback: try to use start_timestamp if we didn't track first seen
                    // This handles processes that were already running when app started
                    if let Ok(uptime_str) = std::fs::read_to_string("/proc/uptime") {
                        if let Some(system_uptime_str) = uptime_str.split_whitespace().next() {
                            if let Ok(system_uptime) = system_uptime_str.parse::<f64>() {
                                let process_uptime = system_uptime - proc.start_timestamp as f64;
                                process_uptime.max(0.0) as u64
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                };
                let entry = ProcessExitLogEntry {
                    pid: proc.pid,
                    name: proc.name.clone(),
                    user: proc.user.clone(),
                    start_time: proc.start_time_str.clone(),
                    exit_time,
                    uptime_secs,
                };
                if self.process_exit_log.len() >= 100 {
                    self.process_exit_log.pop_front();
                }
                self.process_exit_log.push_back(entry);
                // Clean up tracking
                self.process_first_seen.remove(pid);
            }
        }
    }
        // Update prev_pids with current process names
        self.prev_pids = self.process_manager.get_processes()
            .iter()
            .map(|p| (p.pid, p.name.clone()))
            .collect();
        
        // Check alerts
        self.alert_manager.check_alerts(self.process_manager.get_processes(), &prev_pids);
        
        // Check for due scheduler tasks every 5 seconds
        if self.scheduler_last_check.elapsed().as_secs() >= 5 {
            let due_tasks = self.scheduler.check_due_tasks();
            // Clone task info before execution to avoid borrowing issues
            let tasks_to_execute: Vec<(String, crate::scheduler::ScheduleAction)> = due_tasks.iter()
                .filter_map(|&idx| {
                    self.scheduler.get_tasks().get(idx)
                        .map(|t| (t.name.clone(), t.action.clone()))
                })
                .collect();
            
            for (task_name, action) in tasks_to_execute {
                let result = match &action {
                    crate::scheduler::ScheduleAction::RestartProcess { pattern } => {
                        match self.process_manager.restart_process_by_pattern(pattern) {
                            Ok(pids) => {
                                if pids.is_empty() {
                                    format!("No processes found matching '{}' to restart", pattern)
                                } else {
                                    format!("Restarted {} process(es) matching '{}'", pids.len(), pattern)
                                }
                            },
                            Err(e) => format!("Error restarting processes matching '{}': {}", pattern, e),
                        }
                    }
                    crate::scheduler::ScheduleAction::StartProcess { program, args } => {
                        let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                        match self.process_manager.start_process(program, &args_str, None, &[]) {
                            Ok(pid) => format!("Started process '{}' (PID: {})", program, pid),
                            Err(e) => format!("Error starting '{}': {}", program, e),
                        }
                    }
                    crate::scheduler::ScheduleAction::CleanupIdle { cpu_threshold, memory_threshold, action, .. } => {
                        // Note: duration_seconds is not currently checked - would require historical tracking
                        match self.process_manager.cleanup_idle_processes(*cpu_threshold, *memory_threshold, action) {
                            Ok(pids) => format!("Cleaned up {} idle processes", pids.len()),
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                    crate::scheduler::ScheduleAction::ApplyRule { rule } => {
                        self.rule_engine.set_rule(rule.clone());
                        self.process_manager.apply_rules(&mut self.rule_engine);
                        "Rule applied".to_string()
                    }
                    crate::scheduler::ScheduleAction::KillProcess { pid } => {
                        match self.process_manager.kill_process(*pid) {
                            Ok(_) => format!("Killed process PID {}", pid),
                            Err(e) => format!("Error killing PID {}: {}", pid, e),
                        }
                    }
                    crate::scheduler::ScheduleAction::StopProcess { pid } => {
                        match self.process_manager.stop_process(*pid) {
                            Ok(_) => format!("Stopped process PID {}", pid),
                            Err(e) => format!("Error stopping PID {}: {}", pid, e),
                        }
                    }
                    crate::scheduler::ScheduleAction::ContinueProcess { pid } => {
                        match self.process_manager.continue_process(*pid) {
                            Ok(_) => format!("Continued process PID {}", pid),
                            Err(e) => format!("Error continuing PID {}: {}", pid, e),
                        }
                    }
                    crate::scheduler::ScheduleAction::ReniceProcess { pid, nice } => {
                        match self.process_manager.set_niceness(*pid, *nice) {
                            Ok(_) => format!("Reniced PID {} to {}", pid, nice),
                            Err(e) => format!("Error renicing PID {}: {}", pid, e),
                        }
                    }
                };
                self.scheduler.add_log_entry(task_name, result);
            }
            self.scheduler_last_check = std::time::Instant::now();
        }
    }
}





fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let items = vec![
        "Processes",
        "Statistics",
        "Profiles",
        "Alerts",
        "Checkpoints",
        "Multi-Host",
        "Scheduler",
        "Rules",
        "Help",
    ];

    let current_index = match app.view_mode {
        ViewMode::ProcessList | ViewMode::FilterSort | ViewMode::Sort | ViewMode::Filter | ViewMode::FilterInput | ViewMode::KillStop | ViewMode::ChangeNice | ViewMode::StartProcess | ViewMode::AdvancedFilter | ViewMode::PerProcessGraph | ViewMode::ProcessLog | ViewMode::GroupedView | ViewMode::ContainerDetail | ViewMode::NamespaceDetail => 0,
        ViewMode::Statistics => 1,
        ViewMode::ProfileManagement | ViewMode::ProfileEditor => 2,
        ViewMode::AlertManagement | ViewMode::AlertEditor => 3,
        ViewMode::CheckpointManagement => 4,
        ViewMode::MultiHost | ViewMode::HostManagement => 5,
        ViewMode::Scheduler | ViewMode::TaskEditor => 6,
        ViewMode::RuleInput => 7,
        ViewMode::Help => 8,
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let style = if i == current_index {
                Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(item).style(style)
        })
        .collect();

    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title("Menu").style(Style::default().fg(Color::White).bg(Color::Rgb(20, 20, 20))))
        .highlight_style(Style::default().fg(Color::White).bg(Color::Black).add_modifier(Modifier::BOLD));

    f.render_widget(list, area);
}

//ui_renderer
pub fn ui_renderer() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        app.refresh();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(20), // Sidebar width
                    Constraint::Min(0),     // Main content
                ])
                .split(f.size());

            draw_sidebar(f, &app, chunks[0]);
            let main_area = chunks[1];
            
            // Render background
            let background = Block::default().style(Style::default().bg(Color::White));
            f.render_widget(background, main_area);

            match app.view_mode {
                ViewMode::ProcessList => draw_process_list(f, &mut app, main_area),
                ViewMode::Statistics => graph::render_graph_dashboard(
                    f,
                    &app.graph_data,
                    &app.current_stats_tab,
                    app.process_manager.get_processes(),
                    main_area,
                ),
                ViewMode::FilterSort => draw_filter_sort_menu(f, &app, main_area),
                ViewMode::Sort => draw_sort_menu(f, &app, main_area),
                ViewMode::Filter => draw_filter_menu(f, main_area),
                ViewMode::FilterInput => draw_filter_input_menu(f, &app, main_area),
                ViewMode::AdvancedFilter => draw_advanced_filter_input(f, &mut app, main_area),
                ViewMode::KillStop => draw_kill_stop_menu(f, &mut app, main_area),
                ViewMode::ChangeNice => draw_change_nice_menu(f, &mut app, main_area),
                ViewMode::PerProcessGraph => render_per_process_graph_tab(f, main_area, &app),
                ViewMode::RuleInput => draw_rule_input(f, &app, main_area), //for scripting
                ViewMode::GroupedView => draw_grouped_view(f, &mut app, main_area),
                ViewMode::ContainerDetail => draw_container_detail_view(f, &mut app, main_area),
                ViewMode::NamespaceDetail => draw_namespace_detail_view(f, &mut app, main_area),
                ViewMode::Scheduler => draw_scheduler_view(f, &mut app, main_area),
                ViewMode::StartProcess => draw_start_process_menu(f, &mut app, main_area),
                ViewMode::ProfileManagement => draw_profile_management(f, &mut app, main_area),
                ViewMode::ProfileEditor => draw_profile_editor(f, &mut app, main_area),
                ViewMode::AlertManagement => draw_alert_management(f, &mut app, main_area),
                ViewMode::AlertEditor => draw_alert_editor(f, &mut app, main_area),
                ViewMode::CheckpointManagement => draw_checkpoint_management(f, &mut app, main_area),
                ViewMode::MultiHost => draw_multi_host_view(f, &mut app, main_area),
                ViewMode::HostManagement => draw_host_management(f, &mut app, main_area),
                ViewMode::TaskEditor => draw_task_editor(f, &mut app, main_area),
                ViewMode::ProcessLog => {
                    let size = main_area;
                    // Filter log if needed
                    let log: Vec<_> = if app.log_filter_input.is_empty() {
                        app.process_exit_log.make_contiguous().to_vec()
                    } else {
                        let query = app.log_filter_input.to_lowercase();
                        app.process_exit_log
                            .iter()
                            .filter(|entry| {
                                entry.name.to_lowercase().contains(&query)
                                    || entry.user.as_ref().map(|u| u.to_lowercase().contains(&query)).unwrap_or(false)
                                    || entry.pid.to_string().contains(&query)
                            })
                            .cloned()
                            .collect()
                    };
                    // Draw filter input at top (make it 3 lines tall)
                    let group_status = match app.log_group_mode {
                        LogGroupMode::None => "Ungrouped (press 'g' to group)",
                        LogGroupMode::Name => "Grouped by Name (press 'g' to group by PPID, 'u' to ungroup)",
                        LogGroupMode::PPID => "Grouped by PPID (press 'g' to group by User, 'u' to ungroup)",
                        LogGroupMode::User => "Grouped by User (press 'g' to ungroup, 'u' to ungroup)",
                    };
                    let filter_line = if app.log_filter_active {
                        format!("/{}", app.log_filter_input)
                    } else if !app.log_filter_input.is_empty() {
                        format!("Filter: {} | {}", app.log_filter_input, group_status)
                    } else {
                        format!("{}\nPress / to search/filter, ↑/↓/PgUp/PgDn to scroll, g: group, u: ungroup, Esc/q: back", group_status)
                    };
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(5), // Increase height to accommodate two lines
                            Constraint::Min(0),
                        ])
                        .split(size);
                    let filter_para = Paragraph::new(filter_line)
                        .block(Block::default().borders(Borders::ALL).title("Search/Filter/Group").style(Style::default().fg(Color::Black)));
                    f.render_widget(filter_para, chunks[0]);
                    // Calculate visible log window
                    let log_height = chunks[1].height as usize;
                    let (visible, is_grouped) = match app.log_group_mode {
                        LogGroupMode::None => {
                            let total = log.len();
                            let max_scroll = total.saturating_sub(log_height);
                            let offset = app.log_scroll_offset.min(max_scroll);
                            (&log[offset..(offset + log_height).min(total)], false)
                        }
                        LogGroupMode::Name | LogGroupMode::PPID | LogGroupMode::User => {
                            use std::collections::BTreeMap;
                            let mut grouped: BTreeMap<String, Vec<&ProcessExitLogEntry>> = BTreeMap::new();
                            for entry in &log {
                                let key = match app.log_group_mode {
                                    LogGroupMode::Name => entry.name.clone(),
                                    LogGroupMode::PPID => entry.user.clone().unwrap_or_else(|| "Unknown".to_string()), // Use user for now, will fix below
                                    LogGroupMode::User => entry.user.clone().unwrap_or_else(|| "Unknown".to_string()),
                                    LogGroupMode::None => unreachable!(),
                                };
                                grouped.entry(key).or_default().push(entry);
                            }
                            // If grouping by PPID, fix key
                            if app.log_group_mode == LogGroupMode::PPID {
                                grouped.clear();
                                for entry in &log {
                                    let key = format!("{}", entry.pid); // Actually, we want PPID, but ProcessExitLogEntry doesn't have it. For now, use PID.
                                    grouped.entry(key).or_default().push(entry);
                                }
                            }
                            // Build summary rows
                            let mut summary: Vec<(String, usize, u64, u64, u64, String)> = Vec::new();
                            for (key, entries) in grouped.iter() {
                                let count = entries.len();
                                let min_uptime = entries.iter().map(|e| e.uptime_secs).min().unwrap_or(0);
                                let max_uptime = entries.iter().map(|e| e.uptime_secs).max().unwrap_or(0);
                                let avg_uptime = if count > 0 { entries.iter().map(|e| e.uptime_secs).sum::<u64>() / count as u64 } else { 0 };
                                let most_recent = entries.iter().map(|e| e.exit_time).max().map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
                                summary.push((key.clone(), count, min_uptime, max_uptime, avg_uptime, most_recent));
                            }
                            // Sort by count descending
                            summary.sort_by(|a, b| b.1.cmp(&a.1));
                            let total = summary.len();
                            let max_scroll = total.saturating_sub(log_height);
                            let offset = app.log_scroll_offset.min(max_scroll);
                            let visible = &summary[offset..(offset + log_height).min(total)];
                            // Render summary table
                            let header = Row::new(vec![
                                Cell::from(match app.log_group_mode {
                                    LogGroupMode::Name => "Name",
                                    LogGroupMode::PPID => "PPID",
                                    LogGroupMode::User => "User",
                                    LogGroupMode::None => unreachable!(),
                                }).style(Style::default().fg(Color::Yellow)),
                                Cell::from("Count").style(Style::default().fg(Color::Green)),
                                Cell::from("Min Uptime").style(Style::default().fg(Color::Cyan)),
                                Cell::from("Max Uptime").style(Style::default().fg(Color::Cyan)),
                                Cell::from("Avg Uptime").style(Style::default().fg(Color::Cyan)),
                                Cell::from("Most Recent Exit").style(Style::default().fg(Color::Blue)),
                            ]);
                            let rows: Vec<Row> = visible.iter().map(|(key, count, min, max, avg, recent)| {
                                Row::new(vec![
                                    Cell::from(key.clone()),
                                    Cell::from(count.to_string()),
                                    Cell::from(format!("{}s", min)),
                                    Cell::from(format!("{}s", max)),
                                    Cell::from(format!("{}s", avg)),
                                    Cell::from(recent.clone()),
                                ])
                            }).collect();
                            let table = Table::new(rows)
                                .header(header)
                                .block(Block::default().borders(Borders::ALL).title("Process Log (Grouped)").style(Style::default().fg(Color::Black)))
                                .widths(&[
                                    Constraint::Length(20),
                                    Constraint::Length(8),
                                    Constraint::Length(12),
                                    Constraint::Length(12),
                                    Constraint::Length(12),
                                    Constraint::Length(20),
                                ]);
                            f.render_widget(table, chunks[1]);
                            (&[][..], true)
                        }
                    };
                    if !is_grouped {
                        render_process_log_tab(f, chunks[1], visible);
                    }
                },
                ViewMode::Help => {
                    let size = main_area;
                    let help_text = vec![
                        Line::from(vec![Span::styled("Linux Process Manager - Help", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
                        Line::from(""),
                        Line::from(vec![Span::styled("Navigation:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
                        Line::from("  [S] - Statistics/Graphs (CPU, Memory, I/O monitoring)"),
                        Line::from("  [1] - Filter/Sort processes"),
                        Line::from("  [2] - Change process priority (nice value)"),
                        Line::from("  [3] - Kill/Stop/Terminate processes"),
                        Line::from("  [4] - Per-Process Graphs"),
                        Line::from("  [5] - Process Exit Log"),
                        Line::from("  [G] - Grouped View (containers/cgroups)"),
                        Line::from("  [J] - Job Scheduler"),
                        Line::from("  [N] - Start New Process"),
                        Line::from("  [P] - Profile Management"),
                        Line::from("  [A] - Alert Management"),
                        Line::from("  [C] - Checkpoint Management (CRIU)"),
                        Line::from("  [H] - Host Management (Multi-Host)"),
                        Line::from(""),
                        Line::from(vec![Span::styled("Controls:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
                        Line::from("  ↑/↓ - Navigate up/down"),
                        Line::from("  Enter - Select/Confirm"),
                        Line::from("  Esc - Go back"),
                        Line::from("  Q - Quit application"),
                        Line::from(""),
                        Line::from(vec![Span::styled("Press Esc or Q to return", Style::default().fg(Color::Cyan))]),
                    ];
                    let para = Paragraph::new(help_text)
                        .block(Block::default().borders(Borders::ALL).title("Help - Press Esc to go back").style(Style::default().fg(Color::Black)));
                    f.render_widget(para, size);
                },
            }
        })?;

        if handle_events(&mut app)? {
            break;
        }

        sleep(Duration::from_millis(100));
    }

    // Cleanup and restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    Ok(())
}

const PROCESS_TABLE_HEIGHT: usize = 12;

fn draw_process_list(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),     // Header
            Constraint::Min(size.height.saturating_sub(8)), // Process list (reduced to make room for multi-line menu)
            Constraint::Length(5),   // Menu (increased to 5 lines to show all options)
        ])
        .split(size);

    // Update display limit based on available height
    // Height - 2 (borders) - 1 (header) = Height - 3
    if chunks[1].height > 3 {
        app.display_limit = (chunks[1].height - 3) as usize;
    }

    // Get sort indicator for each column
    let get_sort_indicator = |column: &str| -> &str {
        if let Some(mode) = &app.sort_mode {
            if mode == column {
                if app.sort_ascending {
                    " ↑"
                } else {
                    " ↓"
                }
            } else {
                ""
            }
        } else {
            ""
        }
    };

    // Header
    let headers = if app.multi_select_mode {
        let mut h = vec![
            "✓".to_string(),
            format!("PID{}", get_sort_indicator("pid")),
        ];
        if app.multi_host_mode {
            h.push("HOST".to_string());
        }
        h.extend(vec![
            format!("NAME{}", get_sort_indicator("name")),
            format!("USER{}", get_sort_indicator("user")),
            format!("CPU%{}", get_sort_indicator("cpu")),
            format!("MEM(MB){}", get_sort_indicator("mem")),
            format!("START{}", get_sort_indicator("start")),
            format!("NICE{}", get_sort_indicator("nice")),
            "STATUS".to_string(),
            format!("PPID{}", get_sort_indicator("ppid")),
        ]);
        h
    } else {
        let mut h = vec![
            format!("PID{}", get_sort_indicator("pid")),
        ];
        if app.multi_host_mode {
            h.push("HOST".to_string());
        }
        h.extend(vec![
            format!("NAME{}", get_sort_indicator("name")),
            format!("USER{}", get_sort_indicator("user")),
            format!("CPU%{}", get_sort_indicator("cpu")),
            format!("MEM(MB){}", get_sort_indicator("mem")),
            format!("START{}", get_sort_indicator("start")),
            format!("NICE{}", get_sort_indicator("nice")),
            "STATUS".to_string(),
            format!("PPID{}", get_sort_indicator("ppid")),
        ]);
        h
    };

    let header_cells = headers
        .iter()
        .map(|h| Cell::from(h.as_str()).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
    
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Black))
        .height(1);

    // Process rows
    // Apply profile filtering if active
    let processes = if app.rule_engine.active_rule.is_some() {
        app.process_manager.apply_rules(&mut app.rule_engine);
        app.process_manager.get_filtered_processes()
    } else {
        app.process_manager.get_processes()
    };
    
    // Filter by active profile (hide processes)
    let processes: Vec<&process::ProcessInfo> = if app.profile_manager.get_active_profile().is_some() {
        processes.iter()
            .filter(|p| !app.profile_manager.should_hide_process(&p.name))
            .collect()
    } else {
        processes.iter().collect()
    };
    
    
    let rows: Vec<Row> = processes
        .iter()
        .skip(app.scroll_offset)
        .take(app.display_limit)
        .enumerate()
        .map(|(i, process)| {
            let base_style = if i % 2 == 0 {
                Style::default().fg(Color::Black)
            } else {
                Style::default().fg(Color::Black)
            };
            
            // Check if process has active alerts
            let has_alert = app.alert_manager.get_active_alerts().iter()
                .any(|a| a.process_pid == Some(process.pid));
            
            // Highlight if has alert
            let style = if has_alert {
                base_style.fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            let memory_mb = process.memory_usage / (1024 * 1024);
            let cpu_style = match process.cpu_usage {
                c if c > 50.0 => Style::default().fg(Color::Red),
                c if c > 25.0 => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::Green),
            };
            
            let is_selected = app.selected_processes.contains(&process.pid);
            let is_current = (app.scroll_offset + i) == app.selected_process_index;
            
            let mut cells = if app.multi_select_mode {
                vec![
                    Cell::from(if is_selected { "✓" } else { " " })
                        .style(if is_selected { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default() }),
                    Cell::from(process.pid.to_string())
                        .style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Black) }),
                ]
            } else {
                vec![
                    Cell::from(process.pid.to_string())
                        .style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Black) }),
                ]
            };
            
            // Add HOST column if multi-host mode is enabled
            if app.multi_host_mode {
                let host_name = process.host.as_ref().map(|h| h.as_str()).unwrap_or("local");
                cells.push(Cell::from(host_name).style(Style::default().fg(Color::Cyan)));
            }
            
            cells.extend(vec![
                Cell::from(process.name.clone()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Black) }),
                Cell::from(process.user.clone().unwrap_or_default()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Magenta) }),
                Cell::from(format!("{:.2}%", process.cpu_usage)).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { cpu_style }),
                Cell::from(format!("{}MB", memory_mb)).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { style }),
                Cell::from(process.start_time_str.clone()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Black) }),
                Cell::from(process.nice.to_string()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Black) }),
                Cell::from(process.status.trim()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { get_status_style(&process.status) }),
                Cell::from(process.parent_pid.unwrap_or(0).to_string()).style(if is_current { Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD) } else { style }),
            ]);

            Row::new(cells)
        })
        .collect();

    let widths: Vec<Constraint> = if app.multi_select_mode {
        let mut w = vec![
            Constraint::Length(2),  // Selection indicator
            Constraint::Length(8),  // PID
        ];
        if app.multi_host_mode {
            w.push(Constraint::Length(15)); // HOST
        }
        w.extend(vec![
            Constraint::Length(20), // NAME
            Constraint::Length(12), // USER
            Constraint::Length(8),  // CPU%
            Constraint::Length(10), // MEM(MB)
            Constraint::Length(10), // START
            Constraint::Length(6),  // NICE
            Constraint::Length(10), // STATUS
            Constraint::Length(8),  // PPID
        ]);
        w
    } else {
        let mut w = vec![
            Constraint::Length(8),  // PID
        ];
        if app.multi_host_mode {
            w.push(Constraint::Length(15)); // HOST
        }
        w.extend(vec![
            Constraint::Length(20), // NAME
            Constraint::Length(12), // USER
            Constraint::Length(8),  // CPU%
            Constraint::Length(10), // MEM(MB)
            Constraint::Length(10), // START
            Constraint::Length(6),  // NICE
            Constraint::Length(10), // STATUS
            Constraint::Length(8),  // PPID
        ]);
        w
    };

    let table = Table::new(rows)
        .header(header)
        .block(Block::default().borders(Borders::ALL))
        .widths(&widths);

    f.render_widget(table, chunks[1]);

    // Menu
    let multi_select_status = if app.multi_select_mode {
        format!(" [MULTI-SELECT: {} selected]", app.selected_processes.len())
    } else {
        String::new()
    };
    let active_alerts_count = app.alert_manager.get_active_alerts().len();
    let alert_indicator = if active_alerts_count > 0 {
        format!(" [ALERTS: {}]", active_alerts_count)
    } else {
        String::new()
    };
    let active_profile_indicator = app.profile_manager.get_active_profile()
        .map(|s| format!(" [PROFILE: {}]", s))
        .unwrap_or_default();
    
    // Split menu into multiple lines to ensure all options are visible
    let menu_text = vec![
        // Line 1: Navigation and status indicators
        Line::from(vec![
            Span::styled("[↑/↓] Scroll  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[M] Multi-Select  ", Style::default().fg(if app.multi_select_mode { Color::Green } else { Color::Yellow })),
            if app.multi_select_mode {
                Span::styled(multi_select_status, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            },
            if !active_profile_indicator.is_empty() {
                Span::styled(active_profile_indicator, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            },
            if !alert_indicator.is_empty() {
                Span::styled(alert_indicator, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            },
        ]),
        // Line 2: Main actions
        Line::from(vec![
            Span::styled("[1] Filter/Sort  ", Style::default().fg(Color::Yellow)),
            Span::raw("| "),
            Span::styled("[2] Change Nice  ", Style::default().fg(Color::Green)),
            Span::raw("| "),
            Span::styled("[3] Kill/Stop  ", Style::default().fg(Color::Red)),
            Span::raw("| "),
            Span::styled("[4] Per-Process Graph  ", Style::default().fg(Color::Magenta)),
            Span::raw("| "),
            Span::styled("[5] Process Log  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[6] Help  ", Style::default().fg(Color::Yellow)),
        ]),
        // Line 3: Advanced features
        Line::from(vec![
            Span::styled("[S] Statistics  ", Style::default().fg(Color::Blue)),
            Span::raw("| "),
            Span::styled("[G] Grouped View  ", Style::default().fg(Color::Green)),
            Span::raw("| "),
            Span::styled("[J] Scheduler  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[N] New Process  ", Style::default().fg(Color::Green)),
            Span::raw("| "),
            Span::styled("[P] Profiles  ", Style::default().fg(Color::Magenta)),
            Span::raw("| "),
            Span::styled("[A] Alerts  ", Style::default().fg(Color::Red)),
            Span::raw("| "),
            Span::styled("[C] Checkpoints  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[H] Hosts  ", Style::default().fg(Color::Blue)),
            Span::raw("| "),
            Span::styled("[q] Quit", Style::default().fg(Color::Black)),
        ]),
    ];

    let menu = Paragraph::new(menu_text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);

    f.render_widget(menu, chunks[2]);
}

fn draw_filter_sort_menu(f: &mut Frame, app: &App, area: Rect) {
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Process list
            Constraint::Length(3), // Footer/Status
        ])
        .split(area);

    // Header
    let header_text = vec![
        Span::styled("Linux Process Manager", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled(format!("Total Processes: {}", app.process_manager.get_processes().len()), Style::default().fg(Color::Black)),
    ];
    
    let header = Paragraph::new(Line::from(header_text))
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Rounded).style(Style::default().fg(Color::Black)))
        .alignment(Alignment::Center);
    f.render_widget(header, chunks[0]);

    // Menu items
    let items = vec![
        ListItem::new(Span::styled("[1] Sort", Style::default().fg(Color::Yellow))),
        ListItem::new(Span::styled("[2] Filter", Style::default().fg(Color::Green))),
        ListItem::new(Span::styled("[3] Advanced Filter", Style::default().fg(Color::Cyan))),
        ListItem::new(Span::styled("[X] Script Filtering", Style::default().fg(Color::Magenta))),
        ListItem::new(Span::styled("[←] Back", Style::default().fg(Color::Blue))),
    ];

    let menu = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default())
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(menu, chunks[1]);
}

fn draw_sort_menu(f: &mut Frame, app: &App, area: Rect) {
    let size = area;
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Menu items
            Constraint::Length(3),  // Status
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Sort Menu")
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Menu items
    let items = vec![
        ListItem::new(Span::styled("[1] Sort by PID", Style::default().fg(Color::Yellow))),
        ListItem::new(Span::styled("[2] Sort by Memory", Style::default().fg(Color::Green))),
        ListItem::new(Span::styled("[3] Sort by PPID", Style::default().fg(Color::Blue))),
        ListItem::new(Span::styled("[4] Sort by Start Time", Style::default().fg(Color::Magenta))),
        ListItem::new(Span::styled("[5] Sort by Nice Value", Style::default().fg(Color::Cyan))),
        ListItem::new(Span::styled("[6] Sort by CPU Usage", Style::default().fg(Color::Red))),
        ListItem::new(Span::styled("[a] Toggle Ascending/Descending", Style::default().fg(Color::Black))),
        ListItem::new(Span::styled("[←] Back", Style::default().fg(Color::Blue))),
    ];

    let menu = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default())
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(menu, chunks[1]);

    // Status
    let order_text = format!("Current Order: {}", if app.sort_ascending { "Ascending ↑" } else { "Descending ↓" });
    let status = Paragraph::new(order_text)
        .style(Style::default())
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(status, chunks[2]);
}

fn draw_filter_menu(f: &mut Frame, area: Rect) {
    let size = area;
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Menu items
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Select Filter Type")
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Menu items
    let items = vec![
        ListItem::new(Span::styled("[1] Filter by User", Style::default().fg(Color::Magenta))),
        ListItem::new(Span::styled("[2] Filter by Name", Style::default().fg(Color::Green))),
        ListItem::new(Span::styled("[3] Filter by PID", Style::default().fg(Color::Yellow))),
        ListItem::new(Span::styled("[4] Filter by PPID", Style::default().fg(Color::Cyan))),
        ListItem::new(Span::styled("[Esc] Clear Filter", Style::default().fg(Color::Red))),
        ListItem::new(Span::styled("[←] Back", Style::default().fg(Color::Blue))),
    ];

    let menu = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default())
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(menu, chunks[1]);
}

fn draw_filter_input_menu(f: &mut Frame, app: &App, area: Rect) {
    let size = area;
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Instructions
            Constraint::Length(3),  // Input
        ])
        .split(size);

    // Title
    let filter_type = match app.filter_mode.as_deref() {
        Some("user") => "User",
        Some("name") => "Process Name",
        Some("pid") => "PID",
        Some("ppid") => "Parent PID",
        _ => "Unknown",
    };
    let title = Paragraph::new(format!("Enter {} Filter", filter_type))
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Instructions
    let mut instructions = vec![
        ListItem::new(Span::styled(
            format!("Enter value to filter by {}", filter_type.to_lowercase()),
            Style::default().fg(Color::Black)
        )),
        ListItem::new(Span::styled("[Enter] Apply Filter", Style::default().fg(Color::Green))),
        ListItem::new(Span::styled("[←] Back", Style::default().fg(Color::Blue))),
    ];

    if app.filter_mode.as_deref().map_or(false, |m| m == "pid" || m == "ppid") {
        instructions.insert(1, ListItem::new(Span::styled(
            "(Numbers only)",
            Style::default().fg(Color::Yellow)
        )));
    }

    let instructions_widget = List::new(instructions)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default());

    f.render_widget(instructions_widget, chunks[1]);

    // Input field
    let input_text = format!("Filter value: {}", app.input_state.filter_input);
    let input = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(input, chunks[2]);
}

fn draw_kill_stop_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    // Add a visually prominent title box at the top
    let title_chunk = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Make the title box taller
            Constraint::Min(1),
        ])
        .split(size);
    let title = Paragraph::new("Process Control Menu")
        .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, title_chunk[0]);
    let size = title_chunk[1];
    // Add a blank line below the title for spacing
    let spacing_chunk = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(size);
    let size = spacing_chunk[1];

    let process_table_width = (size.width as f32 * 0.55) as u16;
    let right_panel_width = size.width - process_table_width;
    let process_table_height = size.height - 2;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(process_table_width),
            Constraint::Length(right_panel_width),
        ])
        .split(size);

    // --- LEFT: Process Table with highlight ---
    // let processes = app.process_manager.get_processes();

    let processes = if app.rule_engine.active_rule.is_some() {
        app.process_manager.apply_rules(&mut app.rule_engine);
        app.process_manager.get_filtered_processes()
    } else {
        app.process_manager.get_processes()
    };
    

    let headers = ["PID", "NAME", "STATUS", "CPU%", "MEM(MB)", "USER"];
    let header_cells = headers
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Blue))
        .height(1);

    let visible_processes = processes
        .iter()
        .skip(app.scroll_offset)
        .take(process_table_height as usize - 2)
        .enumerate()
        .map(|(i, process)| {
            let idx = app.scroll_offset + i;
            let highlight = idx == app.selected_process_index;
            let style = if highlight {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if i % 2 == 0 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Blue)
            };
            let memory_mb = process.memory_usage / (1024 * 1024);
            Row::new(vec![
                Cell::from(process.pid.to_string()).style(style),
                Cell::from(process.name.clone()).style(Style::default().fg(Color::Green)),
                Cell::from(process.status.trim()).style(get_status_style(&process.status)),
                Cell::from(format!("{:.1}%", process.cpu_usage)).style(style),
                Cell::from(format!("{}", memory_mb)).style(style),
                Cell::from(process.user.clone().unwrap_or_default()).style(Style::default().fg(Color::Magenta)),
            ])
        })
        .collect::<Vec<_>>();

    let process_table = Table::new(visible_processes)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Processes (↑↓ to move, Enter to select)").style(Style::default().fg(Color::Black)))
        .widths(&[
            Constraint::Length(8),   // PID
            Constraint::Length(20),  // NAME
            Constraint::Length(10),  // STATUS
            Constraint::Length(8),   // CPU%
            Constraint::Length(10),  // MEM(MB)
            Constraint::Length(12),  // USER
        ]);
    f.render_widget(process_table, chunks[0]);

    // --- RIGHT: Details, Input, Instructions, Status ---
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Process details
            Constraint::Length(5), // Input box
            Constraint::Min(3),    // Instructions & status
        ])
        .split(chunks[1]);

    // Process details
    let selected = app.selected_process_index.min(processes.len().saturating_sub(1));
    let proc = processes.get(selected);
    let details = if let Some(proc) = proc {
        vec![
            Line::from(vec![Span::styled("Selected Process:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::raw(format!("PID: {}", proc.pid))]),
            Line::from(vec![Span::raw(format!("Name: {}", proc.name))]),
            Line::from(vec![Span::raw(format!("User: {}", proc.user.clone().unwrap_or_default()))]),
            Line::from(vec![Span::raw(format!("Status: {}", proc.status))]),
        ]
    } else {
        vec![Line::from("No process selected.")]
    };
    let details_box = Paragraph::new(details)
        .block(Block::default().borders(Borders::ALL).title("Details").style(Style::default().fg(Color::Black)));
    f.render_widget(details_box, right_chunks[0]);

    // Input box for action
    let input_text = match &app.kill_stop_input_state {
        KillStopInputState::EnteringAction => {
            "Enter action: [k] Kill, [s] Stop, [c] Continue, [t] Terminate, [Esc] Cancel".to_string()
        }
        KillStopInputState::ConfirmingAction { .. } => {
            "Confirming action...".to_string()
        }
        _ => {
            "Press Enter to select action".to_string()
        }
    };
    let input_box = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL).title("Action Input").style(Style::default().fg(Color::Black)));
    f.render_widget(input_box, right_chunks[1]);

    // Instructions and status
    let mut info = vec![
        Line::from(vec![Span::styled(
            "Instructions:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)
        )]),
        Line::from(vec![Span::raw("- Use ↑/↓ to move selection in the process list.")]),
        Line::from(vec![Span::raw("- Press Enter to select a process and input an action.")]),
        Line::from(vec![Span::raw("- Type k/s/c/t for Kill/Stop/Continue/Terminate, then Esc to cancel or return." )]),
        Line::from(vec![Span::raw("- Press Esc to cancel and return.")]),
    ];
    if let Some((msg, is_error)) = &app.input_state.message {
        info.push(Line::from(vec![Span::styled(
            msg,
            if *is_error { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }
        )]));
    }
    let info_box = Paragraph::new(info)
        .block(Block::default().borders(Borders::ALL).title("Help & Status").style(Style::default().fg(Color::Black)));
    f.render_widget(info_box, right_chunks[2]);
    
    // Draw confirmation dialog if in confirmation state
    if let KillStopInputState::ConfirmingAction { pid, process_name, action_type } = &app.kill_stop_input_state {
        draw_confirmation_dialog(f, *pid, process_name, action_type, area);
    }
    
    // Draw dependency warning dialog if in dependency warning state
    if let KillStopInputState::DependencyWarning { pid, process_name, action_type, child_count, children } = &app.kill_stop_input_state {
        draw_dependency_warning_dialog(f, *pid, process_name, action_type, *child_count, children, area);
    }
    
    // Draw batch confirmation dialog if in batch confirmation state
    if let KillStopInputState::ConfirmingBatchAction { pids, process_names, action_type } = &app.kill_stop_input_state {
        draw_batch_confirmation_dialog(f, pids, process_names, action_type, area);
    }
}

// Draw confirmation dialog for process control actions
fn draw_confirmation_dialog(f: &mut Frame, pid: u32, process_name: &str, action_type: &str, area: Rect) {
    use ratatui::layout::Rect;
    
    let size = area;
    
    // Create a centered dialog box
    let dialog_width = 60;
    let dialog_height = 10;
    let x = (size.width.saturating_sub(dialog_width)) / 2;
    let y = (size.height.saturating_sub(dialog_height)) / 2;
    
    let dialog_area = Rect {
        x,
        y,
        width: dialog_width,
        height: dialog_height,
    };
    
    // Draw semi-transparent overlay (by drawing a block)
    f.render_widget(ratatui::widgets::Clear, dialog_area);
    let overlay = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .border_type(ratatui::widgets::BorderType::Thick)
        .style(Style::default().bg(Color::Black));
    f.render_widget(overlay, dialog_area);
    
    // Prepare dialog content
    let action_name = match action_type {
        "kill" => "Kill process",
        "stop" => "Stop process",
        "terminate" => "Terminate process",
        "continue" => "Continue process",
        _ => "Perform action on process",
    };
    
    let warning = match action_type {
        "kill" => "⚠️  WARNING: This will forcefully terminate the process!",
        "stop" => "⚠️  This will suspend the process.",
        "terminate" => "⚠️  This will send a termination signal to the process.",
        "continue" => "This will resume the suspended process.",
        _ => "",
    };
    
    let warning_color = match action_type {
        "kill" => Color::Red,
        "stop" => Color::Yellow,
        "terminate" => Color::Yellow,
        "continue" => Color::Green,
        _ => Color::Black,
    };
    
    let dialog_content = vec![
        Line::from(vec![Span::styled(
            format!("Confirm: {}", action_name),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(format!("Process: {} (PID: {})", process_name, pid))]),
        Line::from(""),
        Line::from(vec![Span::styled(
            warning,
            Style::default().fg(warning_color).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press [y] or [Enter] to confirm, [n] or [Esc] to cancel",
            Style::default().fg(Color::Cyan)
        )]),
    ];
    
    let dialog_paragraph = Paragraph::new(dialog_content)
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    
    // Inner area for content (accounting for borders)
    let inner_area = Rect {
        x: dialog_area.x + 1,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(2),
        height: dialog_area.height.saturating_sub(2),
    };
    
    f.render_widget(dialog_paragraph, inner_area);
}

// Draw dependency warning dialog for processes with children
fn draw_dependency_warning_dialog(f: &mut Frame, pid: u32, process_name: &str, action_type: &str, child_count: usize, children: &[(u32, String)], area: Rect) {
    use ratatui::layout::Rect;
    
    let size = area;
    
    // Create a larger dialog box for dependency warning
    let dialog_width = 70;
    // Increase height to ensure options are visible: base height + children + extra space for options
    let dialog_height = (15 + child_count.min(5)) as u16; // Show up to 5 children + room for options
    let x = (size.width.saturating_sub(dialog_width)) / 2;
    let y = (size.height.saturating_sub(dialog_height)) / 2;
    
    let dialog_area = Rect {
        x,
        y,
        width: dialog_width,
        height: dialog_height,
    };
    
    // Draw warning overlay
    f.render_widget(ratatui::widgets::Clear, dialog_area);
    let overlay = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .border_type(ratatui::widgets::BorderType::Thick)
        .style(Style::default().bg(Color::Black));
    f.render_widget(overlay, dialog_area);
    
    let action_name = match action_type {
        "kill" => "Kill process",
        "terminate" => "Terminate process",
        _ => "Perform action on process",
    };
    
    let mut dialog_content = vec![
        Line::from(vec![Span::styled(
            format!("⚠️  DEPENDENCY WARNING: {}", action_name),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(format!("Process: {} (PID: {})", process_name, pid))]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("This process has {} child process(es)!", child_count),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
    ];
    
    // Show first few children
    if !children.is_empty() {
        dialog_content.push(Line::from(vec![Span::styled(
            "Child processes:",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        )]));
        for (child_pid, child_name) in children.iter().take(5) {
            dialog_content.push(Line::from(vec![Span::raw(
                format!("  - {} (PID: {})", child_name, child_pid)
            )]));
        }
        if children.len() > 5 {
            dialog_content.push(Line::from(vec![Span::raw(
                format!("  ... and {} more", children.len() - 5)
            )]));
        }
        dialog_content.push(Line::from(""));
    }
    
    dialog_content.push(Line::from(vec![Span::styled(
        "⚠️  Killing parent may orphan or affect children!",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    )]));
    dialog_content.push(Line::from(""));
    dialog_content.push(Line::from(vec![Span::styled(
        "[1] Kill parent only  |  [2] Kill parent + all children  |  [n/Esc] Cancel",
        Style::default().fg(Color::Cyan)
    )]));
    
    let dialog_paragraph = Paragraph::new(dialog_content)
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: true });
    
    // Inner area for content
    let inner_area = Rect {
        x: dialog_area.x + 1,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(2),
        height: dialog_area.height.saturating_sub(2),
    };
    
    f.render_widget(dialog_paragraph, inner_area);
}

// Draw batch confirmation dialog for multiple processes
fn draw_batch_confirmation_dialog(f: &mut Frame, pids: &[u32], process_names: &[String], action_type: &str, area: Rect) {
    use ratatui::layout::Rect;
    
    let size = area;
    
    // Create a larger dialog box for batch operations
    let dialog_width = 70;
    let dialog_height = (10 + pids.len().min(8)) as u16; // Show up to 8 processes
    let x = (size.width.saturating_sub(dialog_width)) / 2;
    let y = (size.height.saturating_sub(dialog_height)) / 2;
    
    let dialog_area = Rect {
        x,
        y,
        width: dialog_width,
        height: dialog_height,
    };
    
    // Draw warning overlay
    f.render_widget(ratatui::widgets::Clear, dialog_area);
    let overlay = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .border_type(ratatui::widgets::BorderType::Thick)
        .style(Style::default().bg(Color::Black));
    f.render_widget(overlay, dialog_area);
    
    let action_name = match action_type {
        "kill" => "Kill processes",
        "stop" => "Stop processes",
        "terminate" => "Terminate processes",
        "continue" => "Continue processes",
        _ => "Perform action on processes",
    };
    
    let mut dialog_content = vec![
        Line::from(vec![Span::styled(
            format!("Confirm Batch Action: {}", action_name),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("This will affect {} process(es):", pids.len()),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        )]),
        Line::from(""),
    ];
    
    // Show first few processes
    for (i, (pid, name)) in pids.iter().zip(process_names.iter()).take(8).enumerate() {
        dialog_content.push(Line::from(vec![Span::raw(
            format!("  {}. {} (PID: {})", i + 1, name, pid)
        )]));
    }
    if pids.len() > 8 {
        dialog_content.push(Line::from(vec![Span::raw(
            format!("  ... and {} more", pids.len() - 8)
        )]));
    }
    
    dialog_content.push(Line::from(""));
    dialog_content.push(Line::from(vec![Span::styled(
        "Press [y] or [Enter] to confirm, [n] or [Esc] to cancel",
        Style::default().fg(Color::Cyan)
    )]));
    
    let dialog_paragraph = Paragraph::new(dialog_content)
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: true });
    
    // Inner area for content
    let inner_area = Rect {
        x: dialog_area.x + 1,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(2),
        height: dialog_area.height.saturating_sub(2),
    };
    
    f.render_widget(dialog_paragraph, inner_area);
}

fn draw_change_nice_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    // Add a visually prominent title box at the top
    let title_chunk = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Make the title box taller
            Constraint::Min(1),
        ])
        .split(size);
    let title = Paragraph::new("Change Nice Value")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, title_chunk[0]);
    let size = title_chunk[1];
    // Add a blank line below the title for spacing
    let spacing_chunk = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(size);
    let size = spacing_chunk[1];

    let process_table_width = (size.width as f32 * 0.55) as u16;
    let right_panel_width = size.width - process_table_width;
    let process_table_height = size.height - 2;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(process_table_width),
            Constraint::Length(right_panel_width),
        ])
        .split(size);

    // --- LEFT: Process Table with highlight ---
    let processes = if app.rule_engine.active_rule.is_some() {
        app.process_manager.apply_rules(&mut app.rule_engine);
        app.process_manager.get_filtered_processes()
    } else {
        app.process_manager.get_processes()
    };    let headers = ["PID", "NAME", "NICE", "CPU%", "USER"];
    let header_cells = headers
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Blue))
        .height(1);

    let visible_processes = processes
        .iter()
        .skip(app.change_nice_scroll_offset)
        .take(process_table_height as usize - 2)
        .enumerate()
        .map(|(i, process)| {
            let idx = app.change_nice_scroll_offset + i;
            let highlight = idx == app.selected_process_index;
            let style = if highlight {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if i % 2 == 0 {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Blue)
            };
            Row::new(vec![
                Cell::from(process.pid.to_string()).style(style),
                Cell::from(process.name.clone()).style(Style::default().fg(Color::Green)),
                Cell::from(process.nice.to_string()).style(Style::default().fg(Color::Yellow)),
                Cell::from(format!("{:.1}%", process.cpu_usage)).style(style),
                Cell::from(process.user.clone().unwrap_or_default()).style(Style::default().fg(Color::Magenta)),
            ])
        })
        .collect::<Vec<_>>();

    let process_table = Table::new(visible_processes)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Processes (↑↓ to move, Enter to select)").style(Style::default().fg(Color::Black)))
        .widths(&[
            Constraint::Length(8),   // PID
            Constraint::Length(20),  // NAME
            Constraint::Length(8),   // NICE
            Constraint::Length(8),   // CPU%
            Constraint::Length(12),  // USER
        ]);
    f.render_widget(process_table, chunks[0]);

    // --- RIGHT: Details, Input, Instructions, Status ---
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Process details
            Constraint::Length(5), // Input box
            Constraint::Min(3),    // Instructions & status
        ])
        .split(chunks[1]);

    // Process details
    let selected = app.selected_process_index.min(processes.len().saturating_sub(1));
    let proc = processes.get(selected);
    let details = if let Some(proc) = proc {
        vec![
            Line::from(vec![Span::styled("Selected Process:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
            Line::from(vec![Span::raw(format!("PID: {}", proc.pid))]),
            Line::from(vec![Span::raw(format!("Name: {}", proc.name))]),
            Line::from(vec![Span::raw(format!("User: {}", proc.user.clone().unwrap_or_default()))]),
            Line::from(vec![Span::raw(format!("Current Nice: {}", proc.nice))]),
        ]
    } else {
        vec![Line::from("No process selected.")]
    };
    let details_box = Paragraph::new(details)
        .block(Block::default().borders(Borders::ALL).title("Details").style(Style::default().fg(Color::Black)));
    f.render_widget(details_box, right_chunks[0]);

    // Input box for nice value
    let input_text = if app.nice_input_state == NiceInputState::EnteringNice {
        format!("New nice value (-20 to 19): {}", app.input_state.nice_input)
    } else {
        "Press Enter to change nice value".to_string()
    };
    // If in selection mode or after a message, use yellow (neutral) for input box
    let input_style = if app.nice_input_state == NiceInputState::SelectingPid {
        Style::default().fg(Color::Yellow)
    } else if let Some((_, is_error)) = &app.input_state.message {
        if *is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        }
    } else {
        Style::default().fg(Color::Black)
    };
    let input_box = Paragraph::new(input_text)
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title("Nice Value Input").style(Style::default().fg(Color::Black)));
    f.render_widget(input_box, right_chunks[1]);

    // Instructions and status
    let mut info = vec![
        Line::from(vec![Span::styled(
            "Instructions:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)
        )]),
        Line::from(vec![Span::raw("- Use ↑/↓ to move selection in the process list.")]),
        Line::from(vec![Span::raw("- Press Enter to select a process and input a new nice value.")]),
        Line::from(vec![Span::raw("- Type the new nice value, then Enter to apply." )]),
        Line::from(vec![Span::raw("- Press Esc to cancel and return.")]),
    ];
    if let Some((msg, is_error)) = &app.input_state.message {
        info.push(Line::from(vec![Span::styled(
            msg,
            if *is_error { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) }
        )]));
    }
    let info_box = Paragraph::new(info)
        .block(Block::default().borders(Borders::ALL).title("Help & Status").style(Style::default().fg(Color::Black)));
    f.render_widget(info_box, right_chunks[2]);
}

//scripting ui

fn draw_rule_input(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(4)
        .constraints([Constraint::Min(3)].as_ref())
        .split(area);

    let input = Paragraph::new(app.input_state.rule_input.as_str())
        .block(
            Block::default()
                .title("Enter Rule (e.g., cpu > 5.0 && mem < 1000)").style(Style::default().fg(Color::Black))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().fg(Color::Black)),
        )
        .style(Style::default().fg(Color::Black));

    f.render_widget(input, chunks[0]);
}

fn get_status_style(status: &str) -> Style {
    match status.trim().to_lowercase().as_str() {
        "running" | "run" | "waking" => Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
        "sleeping" | "idle" | "parked" => Style::default().fg(Color::Blue),
        "disk sleep" => Style::default().fg(Color::Magenta),
        "stopped" | "tracing stop" => Style::default().fg(Color::Yellow),
        "zombie" | "dead" | "wakekill" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Black),
    }
}

fn handle_events(app: &mut App) -> Result<bool, Box<dyn Error>> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            match app.view_mode {
                ViewMode::ProcessList => {
                    if handle_process_list_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::Statistics => {
                    if handle_statistics_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::FilterSort => {
                    if handle_filter_sort_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::Sort => {
                    if handle_sort_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::Filter => {
                    if handle_filter_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::FilterInput => {
                    if handle_filter_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::AdvancedFilter => {
                    if handle_advanced_filter_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::KillStop => {
                    if handle_kill_stop_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::ChangeNice => {
                    if handle_change_nice_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::PerProcessGraph => {
                    if handle_per_process_graph_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::RuleInput => {
                    if handle_script_input(key, app)? {
                    return Ok(true);
                    }
                }
                ViewMode::ProcessLog => {
                    if handle_process_log_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::Help => {
                    // Handle help input - allow Esc to go back
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                            app.view_mode = ViewMode::ProcessList;
                        }
                        _ => {}
                    }
                    return Ok(false);
                }
                ViewMode::GroupedView => {
                    if handle_grouped_view_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::ContainerDetail => {
                    if handle_container_detail_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::NamespaceDetail => {
                    if handle_namespace_detail_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::AlertManagement => {
                    if handle_alert_management_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::AlertEditor => {
                    if handle_alert_editor_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::CheckpointManagement => {
                    if handle_checkpoint_management_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::Scheduler => {
                    if handle_scheduler_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::StartProcess => {
                    if handle_start_process_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::ProfileManagement => {
                    if handle_profile_management_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::ProfileEditor => {
                    if handle_profile_editor_input(key, app)? {
                        return Ok(true);
                    }
                }


                ViewMode::MultiHost => {
                    if handle_multi_host_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::HostManagement => {
                    if handle_host_management_input(key, app)? {
                        return Ok(true);
                    }
                }
                ViewMode::TaskEditor => {
                    if handle_task_editor_input(key, app)? {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

fn handle_process_list_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Char('a') => {
            app.sort_ascending = !app.sort_ascending;
            if let Some(mode) = &app.sort_mode {
                app.process_manager.set_sort(mode, app.sort_ascending);
            }
        }        
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('s') | KeyCode::Char('S') => app.view_mode = ViewMode::Statistics,
        KeyCode::Up => {
            if app.selected_process_index > 0 {
                app.selected_process_index -= 1;
                // Adjust scroll if needed
                if app.selected_process_index < app.scroll_offset {
                    app.scroll_offset = app.selected_process_index;
                }
            }
        }
        KeyCode::Down => {
            let process_len = app.process_manager.get_processes().len();
            if app.selected_process_index + 1 < process_len {
                app.selected_process_index += 1;
                // Adjust scroll if needed
                let bottom = app.scroll_offset + app.display_limit;
                if app.selected_process_index >= bottom {
                    app.scroll_offset = app.selected_process_index - app.display_limit + 1;
                }
            }
        }
        KeyCode::Char('1') => app.view_mode = ViewMode::FilterSort,
        KeyCode::Char('2') => app.view_mode = ViewMode::ChangeNice,
        KeyCode::Char('3') => {
            app.view_mode = ViewMode::KillStop;
            if !app.selected_processes.is_empty() {
                // If we have selected processes, skip selection and go to action
                app.kill_stop_input_state = KillStopInputState::EnteringAction;
            } else {
                // Otherwise start selecting a PID
                app.kill_stop_input_state = KillStopInputState::SelectingPid;
            }
        },
        KeyCode::Char('4') => {
            app.view_mode = ViewMode::PerProcessGraph;
            app.selected_process_index = 0;
            app.per_process_graph_scroll_offset = 0;
            app.selected_process_for_graph = None;
        }
        KeyCode::Char('5') => app.view_mode = ViewMode::ProcessLog,
        KeyCode::Char('6') => app.view_mode = ViewMode::Help,
        KeyCode::Char('g') | KeyCode::Char('G') => {
            app.view_mode = ViewMode::GroupedView;
            app.grouped_view_type = crate::process_group::GroupType::Cgroup;
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
        },
        KeyCode::Char('j') | KeyCode::Char('J') => {
            app.view_mode = ViewMode::Scheduler;
            app.selected_task_index = 0;
            app.scheduler_scroll_offset = 0;
        },
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.view_mode = ViewMode::StartProcess;
            app.input_state.program_path.clear();
            app.input_state.working_dir.clear();
            app.input_state.arguments.clear();
            app.input_state.env_vars.clear();
            app.input_state.current_start_input_field = 0;
        },
        KeyCode::Char('p') | KeyCode::Char('P') => {
            app.view_mode = ViewMode::ProfileManagement;
            app.selected_profile_index = 0;
            app.profile_scroll_offset = 0;
        },
        KeyCode::Char('A') => {
            app.view_mode = ViewMode::AlertManagement;
            app.selected_alert_index = 0;
            app.alert_scroll_offset = 0;
        },
        KeyCode::Char('c') | KeyCode::Char('C') => {
            app.view_mode = ViewMode::CheckpointManagement;
            app.selected_checkpoint_index = 0;
            app.checkpoint_scroll_offset = 0;
        },
        KeyCode::Char('h') | KeyCode::Char('H') => {
            app.view_mode = ViewMode::HostManagement;
            app.selected_host_index = 0;
            app.host_scroll_offset = 0;
            app.host_input.clear();
        },
        KeyCode::Char('m') | KeyCode::Char('M') => {
            // Toggle multi-select mode
            app.multi_select_mode = !app.multi_select_mode;
            if !app.multi_select_mode {
                // Clear selections when exiting multi-select mode
                app.selected_processes.clear();
            }
        },
        KeyCode::Char(' ') | KeyCode::Enter => {
            // Toggle selection of current process in multi-select mode
            if app.multi_select_mode {
                let processes = app.process_manager.get_processes();
                if let Some(process) = processes.get(app.selected_process_index) {
                    if app.selected_processes.contains(&process.pid) {
                        app.selected_processes.remove(&process.pid);
                    } else {
                        app.selected_processes.insert(process.pid);
                    }
                }
            }
        },
        _ => {}
    }
    Ok(false)
}

fn handle_statistics_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('S') => {
            app.view_mode = ViewMode::ProcessList;
            app.stats_scroll_offset = 0;  // Reset scroll when leaving statistics view
            app.current_stats_tab = StatisticsTab::Graphs;  // Reset to default tab
        }
        KeyCode::Char('1') => {
            app.current_stats_tab = StatisticsTab::Graphs;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('2') => {
            app.current_stats_tab = StatisticsTab::Overview;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('3') => {
            app.current_stats_tab = StatisticsTab::CPU;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('4') => {
            app.current_stats_tab = StatisticsTab::Memory;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('5') => {
            app.current_stats_tab = StatisticsTab::Disk;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('6') => {
            app.current_stats_tab = StatisticsTab::Processes;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('7') => {
            app.current_stats_tab = StatisticsTab::Advanced;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Char('8') => {
            app.current_stats_tab = StatisticsTab::Help;
            app.stats_scroll_offset = 0;  // Reset scroll when switching tabs
        }
        KeyCode::Up => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Smooth scrolling - move up by 1/4 of the viewport
                let scroll_amount = 3;
                app.stats_scroll_offset = app.stats_scroll_offset.saturating_sub(scroll_amount);
            }
        }
        KeyCode::Down => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Smooth scrolling - move down by 1/4 of the viewport
                let scroll_amount = 3;
                app.stats_scroll_offset = app.stats_scroll_offset.saturating_add(scroll_amount);
            }
        }
        KeyCode::PageUp => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Page up - move by half the viewport
                let scroll_amount = 10;
                app.stats_scroll_offset = app.stats_scroll_offset.saturating_sub(scroll_amount);
            }
        }
        KeyCode::PageDown => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Page down - move by half the viewport
                let scroll_amount = 10;
                app.stats_scroll_offset = app.stats_scroll_offset.saturating_add(scroll_amount);
        }
        }
        KeyCode::Home => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Jump to top
                app.stats_scroll_offset = 0;
            }
        }
        KeyCode::End => {
            if app.current_stats_tab == StatisticsTab::CPU {
                // Jump to bottom (will be bounded by max_scroll in the render function)
                app.stats_scroll_offset = usize::MAX;
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_filter_sort_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Char('1') => app.view_mode = ViewMode::Sort,
        KeyCode::Char('2') => app.view_mode = ViewMode::Filter,
        KeyCode::Char('3') => {
            app.input_state.advanced_filter_input.clear();
            app.view_mode = ViewMode::AdvancedFilter;
        }
        KeyCode::Char('x') => {
            app.input_state.rule_input.clear();
            app.view_mode = ViewMode::RuleInput;
        }
        
        KeyCode::Backspace | KeyCode::Esc => app.view_mode = ViewMode::ProcessList,
        _ => {}
    }
    Ok(false)
}

fn handle_sort_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Char('1') => {
            app.sort_mode = Some("pid".to_string());
            app.process_manager.set_sort("pid", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('2') => {
            app.sort_mode = Some("mem".to_string());
            app.process_manager.set_sort("mem", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('3') => {
            app.sort_mode = Some("ppid".to_string());
            app.process_manager.set_sort("ppid", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('4') => {
            app.sort_mode = Some("start".to_string());
            app.process_manager.set_sort("start", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('5') => {
            app.sort_mode = Some("nice".to_string());
            app.process_manager.set_sort("nice", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('6') => {
            app.sort_mode = Some("cpu".to_string());
            app.process_manager.set_sort("cpu", app.sort_ascending);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char('a') => {
            app.sort_ascending = !app.sort_ascending;
            if let Some(mode) = &app.sort_mode {
                app.process_manager.set_sort(mode, app.sort_ascending);
            }
        }
        KeyCode::Backspace | KeyCode::Esc => app.view_mode = ViewMode::FilterSort,
        _ => {}
    }
    Ok(false)
}

fn handle_filter_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match app.view_mode {
        ViewMode::Filter => {
            match key.code {
                KeyCode::Char('1') => {
                    app.filter_mode = Some("user".to_string());
                    app.input_state.filter_input.clear();
                    app.view_mode = ViewMode::FilterInput;
                }
                KeyCode::Char('2') => {
                    app.filter_mode = Some("name".to_string());
                    app.input_state.filter_input.clear();
                    app.view_mode = ViewMode::FilterInput;
                }
                KeyCode::Char('3') => {
                    app.filter_mode = Some("pid".to_string());
                    app.input_state.filter_input.clear();
                    app.view_mode = ViewMode::FilterInput;
                }
                KeyCode::Char('4') => {
                    app.filter_mode = Some("ppid".to_string());
                    app.input_state.filter_input.clear();
                    app.view_mode = ViewMode::FilterInput;
                }
                KeyCode::Esc => {
                    app.filter_mode = None;
                    app.input_state.filter_input.clear();
                    app.process_manager.set_filter(None, None);
                    app.view_mode = ViewMode::ProcessList;
                }
                KeyCode::Backspace | KeyCode::Left => {
                    app.view_mode = ViewMode::FilterSort;
                }
                _ => {}
            }
        }
        ViewMode::FilterInput => {
            match key.code {
                KeyCode::Char(c) => {
                    let mode = app.filter_mode.as_deref().unwrap_or("");
                    // Only allow digits for PID and PPID filters
                    if (mode == "pid" || mode == "ppid") && !c.is_ascii_digit() {
                        return Ok(false);
                    }
                    app.input_state.filter_input.push(c);
                }
                KeyCode::Backspace => {
                    app.input_state.filter_input.pop();
                }
                KeyCode::Enter => {
                    if !app.input_state.filter_input.is_empty() {
                        app.process_manager.set_filter(
                            app.filter_mode.clone(),
                            Some(app.input_state.filter_input.clone())
                        );
                        app.view_mode = ViewMode::ProcessList;
                    }
                }
                KeyCode::Left => {
                    app.view_mode = ViewMode::Filter;
                    app.input_state.filter_input.clear();
                }
                KeyCode::Esc => {
                    app.filter_mode = None;
                    app.input_state.filter_input.clear();
                    app.process_manager.set_filter(None, None);
                    app.view_mode = ViewMode::ProcessList;
                }
                _ => {}
            }
        }
        _ => unreachable!(),
    }
    Ok(false)
}

fn handle_kill_stop_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let processes = app.process_manager.get_processes();
    match &mut app.kill_stop_input_state {
        KillStopInputState::SelectingPid => {
            match key.code {
                KeyCode::Up => {
                    if app.selected_process_index > 0 {
                        app.selected_process_index -= 1;
                        if app.selected_process_index < app.scroll_offset {
                            app.scroll_offset = app.selected_process_index;
                        }
                    }
                }
                KeyCode::Down => {
                    if app.selected_process_index + 1 < processes.len() {
                        app.selected_process_index += 1;
                        let bottom = app.scroll_offset + app.display_limit;
                        if app.selected_process_index >= bottom {
                            app.scroll_offset = app.selected_process_index - app.display_limit + 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    if !processes.is_empty() {
                        app.kill_stop_input_state = KillStopInputState::EnteringAction;
                        app.input_state.pid_input.clear();
                        app.input_state.message = None;
                    }
                }
                KeyCode::Esc => {
                    app.view_mode = ViewMode::ProcessList;
                    app.input_state = InputState::default();
                    app.kill_stop_input_state = KillStopInputState::SelectingPid;
                }
                _ => {}
            }
        }
        KillStopInputState::EnteringAction => {
            match key.code {
                KeyCode::Char('k') | KeyCode::Char('s') | KeyCode::Char('c') | KeyCode::Char('t') => {
                    let (action_type, _action_name) = match key.code {
                        KeyCode::Char('k') => ("kill", "Kill process"),
                        KeyCode::Char('s') => ("stop", "Stop process"),
                        KeyCode::Char('c') => ("continue", "Continue process"),
                        KeyCode::Char('t') => ("terminate", "Terminate process"),
                        _ => return Ok(false),
                    };
                    
                    // Check if we have selected processes for batch operation
                    if !app.selected_processes.is_empty() {
                        let selected_pids: Vec<u32> = app.selected_processes.iter().copied().collect();
                        let selected_names: Vec<String> = selected_pids.iter()
                            .filter_map(|&pid| {
                                processes.iter().find(|p| p.pid == pid).map(|p| p.name.clone())
                            })
                            .collect();
                        app.kill_stop_input_state = KillStopInputState::ConfirmingBatchAction {
                            pids: selected_pids,
                            process_names: selected_names,
                            action_type: action_type.to_string(),
                        };
                    } else if let Some(process) = processes.get(app.selected_process_index) {
                        // Single process operation
                        // Check for child processes (only for kill/terminate actions)
                        let children = app.process_manager.get_child_processes(process.pid);
                        if !children.is_empty() && (action_type == "kill" || action_type == "terminate") {
                            // Show dependency warning
                            let children_list: Vec<(u32, String)> = children.iter()
                                .map(|c| (c.pid, c.name.clone()))
                                .collect();
                            app.kill_stop_input_state = KillStopInputState::DependencyWarning {
                                pid: process.pid,
                                process_name: process.name.clone(),
                                action_type: action_type.to_string(),
                                child_count: children.len(),
                                children: children_list,
                            };
                        } else {
                            // No children, go directly to confirmation
                            app.kill_stop_input_state = KillStopInputState::ConfirmingAction {
                                pid: process.pid,
                                process_name: process.name.clone(),
                                action_type: action_type.to_string(),
                            };
                        }
                    }
                }
                KeyCode::Esc => {
                    app.kill_stop_input_state = KillStopInputState::SelectingPid;
                    app.input_state.pid_input.clear();
                }
                _ => {}
            }
        }
        KillStopInputState::DependencyWarning { pid, process_name, action_type, child_count, children } => {
            match key.code {
                KeyCode::Char('p') | KeyCode::Char('1') => {
                    // Kill parent only - proceed to confirmation
                    app.kill_stop_input_state = KillStopInputState::ConfirmingAction {
                        pid: *pid,
                        process_name: process_name.clone(),
                        action_type: action_type.clone(),
                    };
                }
                KeyCode::Char('a') | KeyCode::Char('2') => {
                    // Kill parent and all children
                    if action_type == "kill" {
                        match app.process_manager.kill_process_and_children(*pid) {
                            Ok(killed_pids) => {
                                app.input_state.message = Some((
                                    format!("Successfully killed {} processes (parent + {} children)", 
                                        killed_pids.len(), child_count),
                                    false
                                ));
                                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                            }
                            Err(e) => {
                                app.input_state.message = Some((
                                    format!("Error killing processes: {}", e),
                                    true
                                ));
                                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                            }
                        }
                    } else {
                        // For terminate, kill parent and children separately
                        let mut killed_pids = vec![*pid];
                        for (child_pid, _) in children.iter() {
                            if let Err(e) = app.process_manager.terminate_process(*child_pid) {
                                app.input_state.message = Some((
                                    format!("Error terminating child process {}: {}", child_pid, e),
                                    true
                                ));
                                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                                app.kill_stop_input_state = KillStopInputState::SelectingPid;
                                return Ok(false);
                            }
                            killed_pids.push(*child_pid);
                        }
                        if let Err(e) = app.process_manager.terminate_process(*pid) {
                            app.input_state.message = Some((
                                format!("Error terminating parent process: {}", e),
                                true
                            ));
                        } else {
                            app.input_state.message = Some((
                                format!("Successfully terminated {} processes (parent + {} children)", 
                                    killed_pids.len(), *child_count),
                                false
                            ));
                        }
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                    }
                    app.kill_stop_input_state = KillStopInputState::SelectingPid;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    // Cancel - return to action selection
                    app.kill_stop_input_state = KillStopInputState::EnteringAction;
                }
                _ => {}
            }
        }
        KillStopInputState::ConfirmingAction { pid, process_name: _, action_type } => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    // User confirmed - execute the action
                    let action = match action_type.as_str() {
                        "kill" => {
                            match app.process_manager.kill_process(*pid) {
                                Ok(_) => Some(("Successfully killed process".to_string(), false)),
                                Err(e) => Some((format!("Error killing process: {}", e), true)),
                            }
                        }
                        "stop" => {
                            match app.process_manager.stop_process(*pid) {
                                Ok(_) => Some(("Successfully stopped process".to_string(), false)),
                                Err(e) => Some((format!("Error stopping process: {}", e), true)),
                            }
                        }
                        "continue" => {
                            match app.process_manager.continue_process(*pid) {
                                Ok(_) => Some(("Successfully continued process".to_string(), false)),
                                Err(e) => Some((format!("Error continuing process: {}", e), true)),
                            }
                        }
                        "terminate" => {
                            match app.process_manager.terminate_process(*pid) {
                                Ok(_) => Some(("Successfully sent termination request to process".to_string(), false)),
                                Err(e) => Some((format!("Error sending termination request: {}", e), true)),
                            }
                        }
                        _ => None,
                    };

                    if let Some((msg, is_error)) = action {
                        app.input_state.message = Some((
                            format!("{} {}", msg, *pid),
                            is_error
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                    }
                    
                    // Return to selecting PID
                    app.kill_stop_input_state = KillStopInputState::SelectingPid;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    // User cancelled - return to action selection
                    app.kill_stop_input_state = KillStopInputState::EnteringAction;
                }
                _ => {}
            }
        }
        KillStopInputState::ConfirmingBatchAction { pids, process_names: _, action_type } => {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    // Execute batch action
                    let mut success_count = 0;
                    let mut error_count = 0;
                    
                    for pid in pids.iter() {
                        let result = match action_type.as_str() {
                            "kill" => app.process_manager.kill_process(*pid),
                            "stop" => app.process_manager.stop_process(*pid),
                            "terminate" => app.process_manager.terminate_process(*pid),
                            "continue" => app.process_manager.continue_process(*pid),
                            _ => continue,
                        };
                        
                        if result.is_ok() {
                            success_count += 1;
                        } else {
                            error_count += 1;
                        }
                    }
                    
                    app.input_state.message = Some((
                        format!("Batch {}: {} succeeded, {} failed", action_type, success_count, error_count),
                        error_count > 0,
                    ));
                    app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    app.kill_stop_input_state = KillStopInputState::SelectingPid;
                    app.selected_processes.clear();
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    // User cancelled
                    app.kill_stop_input_state = KillStopInputState::EnteringAction;
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

fn handle_change_nice_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let processes = app.process_manager.get_processes();
    match app.nice_input_state {
        NiceInputState::SelectingPid => {
            match key.code {
                KeyCode::Up => {
                    if app.selected_process_index > 0 {
                        app.selected_process_index -= 1;
                        if app.selected_process_index < app.change_nice_scroll_offset {
                            app.change_nice_scroll_offset = app.selected_process_index;
                        }
                    }
                }
                KeyCode::Down => {
                    if app.selected_process_index + 1 < processes.len() {
                        app.selected_process_index += 1;
                        let bottom = app.change_nice_scroll_offset + (PROCESS_TABLE_HEIGHT - 2);
                        if app.selected_process_index >= bottom {
                            app.change_nice_scroll_offset += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    if !processes.is_empty() {
                        app.nice_input_state = NiceInputState::EnteringNice;
                        app.input_state.nice_input.clear();
                        app.input_state.message = None;
                    }
                }
                KeyCode::Esc => {
                    app.view_mode = ViewMode::ProcessList;
                    app.input_state = InputState::default();
                    app.nice_input_state = NiceInputState::SelectingPid;
                }
                _ => {}
            }
        }
        NiceInputState::EnteringNice => {
            match key.code {
                KeyCode::Char(c) => {
                    if c.is_ascii_digit() || (c == '-' && app.input_state.nice_input.is_empty()) {
                        app.input_state.nice_input.push(c);
                    }
                }
                KeyCode::Backspace => {
                    app.input_state.nice_input.pop();
                }
                KeyCode::Enter => {
                    if !app.input_state.nice_input.is_empty() {
                        if let (Some(proc), Ok(nice)) = (
                            processes.get(app.selected_process_index),
                            app.input_state.nice_input.parse::<i32>(),
                        ) {
                            if nice >= -20 && nice <= 19 {
                                match app.process_manager.set_niceness(proc.pid, nice) {
                                    Ok(_) => {
                                        app.input_state.message = Some((
                                            format!("Successfully changed nice value of process {} to {}", proc.pid, nice),
                                            false
                                        ));
                                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(1));
                                        app.nice_input_state = NiceInputState::SelectingPid;
                                        app.input_state.nice_input.clear();
                                    }
                                    Err(e) => {
                                        app.input_state.message = Some((
                                            format!("Error changing nice value: {}", e),
                                            true
                                        ));
                                        app.nice_input_state = NiceInputState::SelectingPid;
                                        app.input_state.nice_input.clear();
                                    }
                                }
                            } else {
                                app.input_state.message = Some((
                                    "Error: Nice value must be between -20 and 19".to_string(),
                                    true
                                ));
                                app.nice_input_state = NiceInputState::SelectingPid;
                                app.input_state.nice_input.clear();
                            }
                        }
                    }
                }
                KeyCode::Esc => {
                    app.nice_input_state = NiceInputState::SelectingPid;
                    app.input_state.nice_input.clear();
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

fn handle_per_process_graph_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let processes = app.process_manager.get_processes();
    match key.code {
        KeyCode::Char('q') => {
            app.view_mode = ViewMode::ProcessList;
            app.selected_process_for_graph = None;
            Ok(true)
        }
        KeyCode::Left => {
            // Switch to previous process
            if let Some(pid) = app.selected_process_for_graph {
                if let Some(idx) = processes.iter().position(|p| p.pid == pid) {
                    if idx > 0 {
                        app.selected_process_for_graph = Some(processes[idx - 1].pid);
                    }
                }
            }
            Ok(false)
        }
        KeyCode::Right => {
            // Switch to next process
            if let Some(pid) = app.selected_process_for_graph {
                if let Some(idx) = processes.iter().position(|p| p.pid == pid) {
                    if idx + 1 < processes.len() {
                        app.selected_process_for_graph = Some(processes[idx + 1].pid);
                    }
                }
            }
            Ok(false)
        }
        KeyCode::Up => {
            if let Some(_pid) = app.selected_process_for_graph {
                app.selected_process_for_graph = None;
            } else {
                if app.selected_process_index > 0 {
                    app.selected_process_index -= 1;
                    if app.selected_process_index < app.per_process_graph_scroll_offset {
                        app.per_process_graph_scroll_offset = app.selected_process_index;
                    }
                }
            }
            Ok(false)
        }
        KeyCode::Down => {
            if let Some(_pid) = app.selected_process_for_graph {
                app.selected_process_for_graph = None;
            } else {
                let max_index = processes.len().saturating_sub(1);
                if app.selected_process_index < max_index {
                    app.selected_process_index += 1;
                    if app.selected_process_index >= app.per_process_graph_scroll_offset + PROCESS_TABLE_HEIGHT - 2 {
                        app.per_process_graph_scroll_offset = app.selected_process_index - (PROCESS_TABLE_HEIGHT - 3);
                    }
                }
            }
            Ok(false)
        }
        KeyCode::Enter => {
            if app.selected_process_for_graph.is_none() {
                if let Some(process) = processes.get(app.selected_process_index) {
                    app.selected_process_for_graph = Some(process.pid);
                }
            }
            Ok(false)
        }
        KeyCode::Esc => {
            if app.selected_process_for_graph.is_some() {
                app.selected_process_for_graph = None;
            } else {
                app.view_mode = ViewMode::ProcessList;
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_script_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Enter => {
            let rule = app.input_state.rule_input.trim().to_string();
            app.rule_engine.set_rule(rule);
            app.process_manager.apply_rules(&mut app.rule_engine);
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Char(c) => {
            app.input_state.rule_input.push(c);
        }
        KeyCode::Backspace => {
            app.input_state.rule_input.pop();
        }
        _ => {}
    }
    Ok(false)
}


fn render_per_process_graph_tab(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(5),  // Process info
            Constraint::Min(0),     // Content
            Constraint::Length(2),  // Help line
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Per-Process Graph View")
        .style(Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)));
    frame.render_widget(title, chunks[0]);

    if let Some(pid) = app.selected_process_for_graph {
        let processes = app.process_manager.get_processes();
        if let Some(process) = processes.iter().find(|p| p.pid == pid) {
            // Process info box
            let info_lines = vec![
                Line::from(vec![Span::styled(format!("Name: {}", process.name), Style::default().fg(Color::Green))]),
                Line::from(vec![Span::styled(format!("PID: {}", process.pid), Style::default().fg(Color::Yellow)), Span::raw("  "), Span::styled(format!("User: {}", process.user.clone().unwrap_or_default()), Style::default().fg(Color::Magenta))]),
                Line::from(vec![Span::styled(format!("PPID: {}", process.parent_pid.unwrap_or(0)), Style::default().fg(Color::Cyan)), Span::raw("  "), Span::styled(format!("Status: {}", process.status), Style::default().fg(Color::Black))]),
                Line::from(vec![Span::styled(format!("Start: {}", process.start_time_str), Style::default().fg(Color::Black))]),
            ];
            let info_box = Paragraph::new(info_lines)
                .block(Block::default().borders(Borders::ALL).title("Process Info").style(Style::default().fg(Color::Black)));
            frame.render_widget(info_box, chunks[1]);

            // Graphs
            let graph_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(50),  // CPU Graph
                    Constraint::Percentage(50),  // Memory Graph
                ])
                .split(chunks[2]);

            if let Some((cpu_history, mem_history)) = app.graph_data.get_process_history(pid) {
                // Live stats for CPU
                let current_cpu = cpu_history.back().copied().unwrap_or(0.0);
                let min_cpu = cpu_history.iter().cloned().fold(f32::INFINITY, f32::min);
                let max_cpu = cpu_history.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let avg_cpu = if !cpu_history.is_empty() {
                    cpu_history.iter().sum::<f32>() / cpu_history.len() as f32
                } else { 0.0 };
                // CPU Graph
                let cpu_data: Vec<(f64, f64)> = cpu_history.iter()
                    .enumerate()
                    .map(|(i, &usage)| (i as f64, usage as f64))
                    .collect();
                let cpu_dataset = Dataset::default()
                    .name("CPU Usage")
                    .marker(ratatui::symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&cpu_data);
                let cpu_chart = Chart::new(vec![cpu_dataset])
                    .block(Block::default()
                        .title(format!("CPU Usage for {} (PID: {}) | Now: {:.1}%  Min: {:.1}%  Max: {:.1}%  Avg: {:.1}%", process.name, pid, current_cpu, min_cpu, max_cpu, avg_cpu))
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Cyan)))
                    .x_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, cpu_history.len() as f64])
                        .labels(vec![]))
                    .y_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, 100.0])
                        .labels(vec!["0%".into(), "50%".into(), "100%".into()]));
                frame.render_widget(cpu_chart, graph_chunks[0]);

                // Live stats for MEM
                let current_mem = mem_history.back().copied().unwrap_or(0) as f64 / (1024.0 * 1024.0);
                let min_mem = mem_history.iter().cloned().min().unwrap_or(0) as f64 / (1024.0 * 1024.0);
                let max_mem = mem_history.iter().cloned().max().unwrap_or(0) as f64 / (1024.0 * 1024.0);
                let avg_mem = if !mem_history.is_empty() {
                    mem_history.iter().sum::<u64>() as f64 / mem_history.len() as f64 / (1024.0 * 1024.0)
                } else { 0.0 };
                let memory_data: Vec<(f64, f64)> = mem_history.iter()
                    .enumerate()
                    .map(|(i, &usage)| (i as f64, usage as f64 / (1024.0 * 1024.0)))
                    .collect();
                let max_memory = memory_data.iter()
                    .map(|&(_, y)| y)
                    .fold(0.0, f64::max)
                    .max(1.0);
                let memory_dataset = Dataset::default()
                    .name("Memory Usage")
                    .marker(ratatui::symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Green))
                    .data(&memory_data);
                let memory_chart = Chart::new(vec![memory_dataset])
                    .block(Block::default()
                        .title(format!("Memory Usage for {} (PID: {}) | Now: {:.2} MB  Min: {:.2} MB  Max: {:.2} MB  Avg: {:.2} MB", process.name, pid, current_mem, min_mem, max_mem, avg_mem))
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Green)))
                    .x_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, mem_history.len() as f64])
                        .labels(vec![]))
                    .y_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, max_memory * 1.2])
                        .labels(vec![
                            "0 MB".into(),
                            format!("{:.1} MB", max_memory / 2.0).into(),
                            format!("{:.1} MB", max_memory).into(),
                        ]));
                frame.render_widget(memory_chart, graph_chunks[1]);
            }
        }
        // Help line
        let help = Paragraph::new("←/→: Next/Prev process  ↑/↓: Back to list  Enter: Select  Esc: Back  Q: Quit")
            .style(Style::default().fg(Color::Black))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(help, chunks[3]);
    } else {
        // Show process selection list
        let processes = app.process_manager.get_processes();
        let headers = ["PID", "NAME", "CPU%", "MEM(MB)", "USER"];
        let header_cells = headers
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::Blue))
            .height(1);
        let rows: Vec<Row> = processes
            .iter()
            .skip(app.per_process_graph_scroll_offset)
            .take(PROCESS_TABLE_HEIGHT - 2)
            .enumerate()
            .map(|(i, process)| {
                let idx = app.per_process_graph_scroll_offset + i;
                let highlight = idx == app.selected_process_index;
                let style = if highlight {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if i % 2 == 0 {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Blue)
                };
                let memory_mb = process.memory_usage / (1024 * 1024);
                Row::new(vec![
                    Cell::from(process.pid.to_string()).style(style),
                    Cell::from(process.name.clone()).style(Style::default().fg(Color::Green)),
                    Cell::from(format!("{:.1}%", process.cpu_usage)).style(style),
                    Cell::from(format!("{}", memory_mb)).style(style),
                    Cell::from(process.user.clone().unwrap_or_default()).style(Style::default().fg(Color::Magenta)),
                ])
            })
            .collect();
        let table = Table::new(rows)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Select a Process (↑↓ to move, Enter to select, Esc to return)").style(Style::default().fg(Color::Black)))
            .widths(&[
                Constraint::Length(8),   // PID
                Constraint::Length(20),  // NAME
                Constraint::Length(8),   // CPU%
                Constraint::Length(10),  // MEM(MB)
                Constraint::Length(12),  // USER
            ]);
        frame.render_widget(table, chunks[2]);
        // Help line
        let help = Paragraph::new("↑/↓: Move  Enter: Select  Esc: Back  Q: Quit")
            .style(Style::default().fg(Color::Black))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(help, chunks[3]);
    }
}

// fn render_help_tab(frame: &mut ratatui::Frame, area: Rect) {
//     let text = vec![
//         Line::from(vec![Span::styled("Help & Documentation", Style::default().fg(Color::White).add_modifier(Modifier::BOLD))]),
//         Line::from(vec![Span::styled("Navigation:", Style::default().fg(Color::Cyan))]),
//         Line::from(vec![Span::styled("↑/↓ - Scroll through processes", Style::default().fg(Color::Gray))]),
//         Line::from(vec![Span::styled("1-6 - Switch between views", Style::default().fg(Color::Gray))]),
//         Line::from(vec![Span::styled("S - Show statistics", Style::default().fg(Color::Gray))]),
//         Line::from(vec![Span::styled("q - Quit", Style::default().fg(Color::Gray))]),
//     ];
//     let widget = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Help").style(Style::default().fg(Color::Black)));
//     frame.render_widget(widget, area);
// }

//draw_help

fn handle_process_log_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    // For robust scrolling, recalculate max_scroll based on current filtered log and a default height (e.g., 10)
    let log: Vec<_> = if app.log_filter_input.is_empty() {
        app.process_exit_log.make_contiguous().to_vec()
    } else {
        let query = app.log_filter_input.to_lowercase();
        app.process_exit_log
            .iter()
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query)
                    || entry.user.as_ref().map(|u| u.to_lowercase().contains(&query)).unwrap_or(false)
                    || entry.pid.to_string().contains(&query)
            })
            .cloned()
            .collect()
    };
    let log_height = 10; // fallback, real height is used in rendering
    let total = log.len();
    let max_scroll = total.saturating_sub(log_height);
    if app.log_filter_active {
        match key.code {
            KeyCode::Esc => {
                app.log_filter_active = false;
                app.log_filter_input.clear();
                app.log_scroll_offset = 0;
            }
            KeyCode::Enter => {
                app.log_filter_active = false;
                app.log_scroll_offset = 0;
            }
            KeyCode::Backspace => {
                app.log_filter_input.pop();
                app.log_scroll_offset = 0;
            }
            KeyCode::Char(c) => {
                app.log_filter_input.push(c);
                app.log_scroll_offset = 0;
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Char('g') => {
                app.log_group_mode = match app.log_group_mode {
                    LogGroupMode::None => LogGroupMode::Name,
                    LogGroupMode::Name => LogGroupMode::PPID,
                    LogGroupMode::PPID => LogGroupMode::User,
                    LogGroupMode::User => LogGroupMode::None,
                };
                app.log_scroll_offset = 0;
            }
            KeyCode::Char('u') => {
                app.log_group_mode = LogGroupMode::None;
                app.log_scroll_offset = 0;
            }
            KeyCode::Char('/') => {
                app.log_filter_active = true;
                app.log_filter_input.clear();
                app.log_scroll_offset = 0;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                app.view_mode = ViewMode::ProcessList;
                app.log_filter_input.clear();
                app.log_filter_active = false;
                app.log_scroll_offset = 0;
            }
            KeyCode::Up => {
                app.log_scroll_offset = app.log_scroll_offset.saturating_sub(1).min(max_scroll);
            }
            KeyCode::Down => {
                app.log_scroll_offset = (app.log_scroll_offset + 1).min(max_scroll);
            }
            KeyCode::PageUp => {
                app.log_scroll_offset = app.log_scroll_offset.saturating_sub(log_height).min(max_scroll);
            }
            KeyCode::PageDown => {
                app.log_scroll_offset = (app.log_scroll_offset + log_height).min(max_scroll);
            }
            _ => {}
        }
    }
    Ok(false)
}

// Draw container detail view
fn draw_container_detail_view(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::container_view::get_container_details;
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(6),  // Container info
            Constraint::Min(0),     // Process list
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Header
    let title = Paragraph::new("Container Details")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let processes = app.process_manager.get_processes();
    if let Some(container_id) = &app.selected_container_id {
        if let Some(container) = get_container_details(processes, container_id) {
            // Container info
            let memory_mb = container.memory_usage / (1024 * 1024);
            let process_count_str = container.process_count().to_string();
            let info_lines = vec![
                Line::from(vec![Span::styled("Container ID: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(&container.id)]),
                Line::from(vec![Span::styled("Name: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(&container.name)]),
                Line::from(vec![Span::styled("Total CPU: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(format!("{:.1}%", container.cpu_usage), Style::default().fg(Color::Cyan))]),
                Line::from(vec![Span::styled("Total Memory: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(format!("{} MB", memory_mb), Style::default().fg(Color::Green))]),
                Line::from(vec![Span::styled("Process Count: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(&process_count_str)]),
            ];
            let info = Paragraph::new(info_lines)
                .block(Block::default().borders(Borders::ALL).title("Container Information").style(Style::default().fg(Color::Black)));
            f.render_widget(info, chunks[1]);

            // Process list
            if container.processes.is_empty() {
                let empty_msg = Paragraph::new("No processes found in this container")
                    .style(Style::default().fg(Color::Yellow))
                    .block(Block::default().borders(Borders::ALL).title("Processes in Container").style(Style::default().fg(Color::Black)));
                f.render_widget(empty_msg, chunks[2]);
            } else {
                let headers = ["PID", "NAME", "CPU%", "MEM(MB)", "USER"];
                let header_cells = headers.iter().map(|h| Cell::from(*h).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                let header = Row::new(header_cells).style(Style::default().bg(Color::Blue)).height(1);

                let visible_height = chunks[2].height as usize - 2;
                let start_idx = app.detail_view_scroll_offset.min(container.processes.len().saturating_sub(visible_height));
                let end_idx = (start_idx + visible_height).min(container.processes.len());

                let rows: Vec<Row> = container.processes.iter().skip(start_idx).take(end_idx - start_idx)
                    .map(|proc| {
                        Row::new(vec![
                            Cell::from(proc.pid.to_string()),
                            Cell::from(proc.name.clone()),
                            Cell::from(format!("{:.1}%", proc.cpu_usage)),
                            Cell::from(format!("{}", proc.memory_usage / (1024 * 1024))),
                            Cell::from(proc.user.clone().unwrap_or_default()),
                        ])
                    })
                    .collect();

                let table = Table::new(rows)
                    .header(header)
                    .block(Block::default().borders(Borders::ALL).title("Processes in Container").style(Style::default().fg(Color::Black)))
                    .widths(&[
                        Constraint::Length(8),
                        Constraint::Length(20),
                        Constraint::Length(8),
                        Constraint::Length(10),
                        Constraint::Length(12),
                    ]);
                f.render_widget(table, chunks[2]);
            }
        } else {
            // Container not found
            let error_msg = Paragraph::new(format!("Container '{}' not found or has no processes", container_id))
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title("Error").style(Style::default().fg(Color::Black)));
            f.render_widget(error_msg, chunks[1]);
            
            let empty_msg = Paragraph::new("No container data available")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Processes in Container").style(Style::default().fg(Color::Black)));
            f.render_widget(empty_msg, chunks[2]);
        }
    } else {
        // No container selected
        let error_msg = Paragraph::new("No container selected")
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title("Error").style(Style::default().fg(Color::Black)));
        f.render_widget(error_msg, chunks[1]);
        
        let empty_msg = Paragraph::new("No container data available")
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Processes in Container").style(Style::default().fg(Color::Black)));
        f.render_widget(empty_msg, chunks[2]);
    }

    // Menu
    let menu = Paragraph::new("↑/↓: Scroll  |  [Esc] Back")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Draw namespace detail view
fn draw_namespace_detail_view(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::namespace_view::get_namespace_group_details;
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(6),  // Namespace info
            Constraint::Min(0),     // Process list
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Header
    let title = Paragraph::new("Namespace Details")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let processes = app.process_manager.get_processes();
    if let Some((ns_type, ns_id)) = &app.selected_namespace {
        if let Some(group) = get_namespace_group_details(processes, ns_type, *ns_id) {
            // Namespace info
            let memory_mb = group.memory_usage / (1024 * 1024);
            let ns_id_str = ns_id.to_string();
            let process_count_str = group.process_count().to_string();
            let info_lines = vec![
                Line::from(vec![Span::styled("Namespace Type: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(ns_type)]),
                Line::from(vec![Span::styled("Namespace ID: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(&ns_id_str)]),
                Line::from(vec![Span::styled("Total CPU: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(format!("{:.1}%", group.cpu_usage), Style::default().fg(Color::Cyan))]),
                Line::from(vec![Span::styled("Total Memory: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::styled(format!("{} MB", memory_mb), Style::default().fg(Color::Green))]),
                Line::from(vec![Span::styled("Process Count: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)), Span::raw(&process_count_str)]),
            ];
            let info = Paragraph::new(info_lines)
                .block(Block::default().borders(Borders::ALL).title("Namespace Information").style(Style::default().fg(Color::Black)));
            f.render_widget(info, chunks[1]);

            // Process list
            let headers = ["PID", "NAME", "CPU%", "MEM(MB)", "USER"];
            let header_cells = headers.iter().map(|h| Cell::from(*h).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
            let header = Row::new(header_cells).style(Style::default().bg(Color::Blue)).height(1);

            let visible_height = chunks[2].height as usize - 2;
            let start_idx = app.detail_view_scroll_offset.min(group.processes.len().saturating_sub(visible_height));
            let end_idx = (start_idx + visible_height).min(group.processes.len());

            let rows: Vec<Row> = group.processes.iter().skip(start_idx).take(end_idx - start_idx)
                .map(|proc| {
                    Row::new(vec![
                        Cell::from(proc.pid.to_string()),
                        Cell::from(proc.name.clone()),
                        Cell::from(format!("{:.1}%", proc.cpu_usage)),
                        Cell::from(format!("{}", proc.memory_usage / (1024 * 1024))),
                        Cell::from(proc.user.clone().unwrap_or_default()),
                    ])
                })
                .collect();

            let table = Table::new(rows)
                .header(header)
                .block(Block::default().borders(Borders::ALL).title("Processes in Namespace").style(Style::default().fg(Color::Black)))
                .widths(&[
                    Constraint::Length(8),
                    Constraint::Length(20),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Length(12),
                ]);
            f.render_widget(table, chunks[2]);
        }
    }

    // Menu
    let menu = Paragraph::new("↑/↓: Scroll  |  [Esc] Back")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Draw grouped view for cgroups, containers, and namespaces
fn draw_grouped_view(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::process_group::{ProcessGroupManager, GroupType};
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(0),     // Content
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Header
    let group_type_name = match app.grouped_view_type {
        GroupType::Cgroup => "Cgroup",
        GroupType::Container => "Container",
        GroupType::Namespace(ref ns) => ns,
        GroupType::Username => "Username",
    };
    let title = Paragraph::new(format!("Grouped View: {}", group_type_name))
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Get grouped processes
    let processes = app.process_manager.get_processes();
    let groups: Vec<crate::process_group::ProcessGroup> = match app.grouped_view_type {
        GroupType::Cgroup => ProcessGroupManager::group_by_cgroup(processes),
        GroupType::Container => ProcessGroupManager::group_by_container(processes),
        GroupType::Namespace(ref ns_type) => ProcessGroupManager::group_by_namespace(processes, ns_type),
        GroupType::Username => ProcessGroupManager::group_by_username(processes),
    };

    // Sort groups - maintain stability for expanded groups to prevent jumping
    let mut sorted_groups = groups;
    
    if app.group_view_frozen && !app.frozen_group_order.is_empty() {
        // Maintain frozen order for all groups
        let mut frozen_groups = Vec::new();
        
        // Separate groups into frozen (in order) and others
        for group_id in &app.frozen_group_order {
            if let Some(pos) = sorted_groups.iter().position(|g| &g.group_id == group_id) {
                frozen_groups.push(sorted_groups.remove(pos));
            }
        }
        // Sort remaining groups by CPU
        sorted_groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
        
        // Combine: frozen groups first (in their order), then others sorted by CPU
        let mut final_groups = frozen_groups;
        final_groups.extend(sorted_groups);
        sorted_groups = final_groups;
    } else if !app.expanded_groups.is_empty() && !app.frozen_group_order.is_empty() {
        // Auto-stabilize: maintain order for expanded groups, sort others by CPU
        let mut stable_groups = Vec::new();
        
        // Keep expanded groups in their current order
        for group_id in &app.frozen_group_order {
            if app.expanded_groups.contains(group_id) {
                if let Some(pos) = sorted_groups.iter().position(|g| &g.group_id == group_id) {
                    stable_groups.push(sorted_groups.remove(pos));
                }
            }
        }
        
        // Sort remaining groups by CPU
        sorted_groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
        
        // Insert stable groups at their original positions (if possible) or at top
        // For simplicity, put stable groups first, then others
        let mut final_groups = stable_groups;
        final_groups.extend(sorted_groups);
        sorted_groups = final_groups;
        
        // Update frozen order to maintain stability
        app.frozen_group_order = sorted_groups.iter().map(|g| g.group_id.clone()).collect();
    } else {
        // Normal sort by CPU usage (descending)
        sorted_groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
        
        // Update frozen order when groups change (for future stability)
        app.frozen_group_order = sorted_groups.iter().map(|g| g.group_id.clone()).collect();
    }

    // Build list items for groups
    // Note: Scroll offset is based on groups, expanded processes are shown inline
    let visible_height = chunks[1].height as usize - 2;
    let start_idx = app.grouped_view_scroll_offset.min(sorted_groups.len().saturating_sub(1));
    let end_idx = (start_idx + visible_height.min(20)).min(sorted_groups.len()); // Limit to reasonable number

    let mut items = Vec::new();
    for (i, group) in sorted_groups.iter().enumerate().skip(start_idx).take(end_idx - start_idx) {
        let is_expanded = app.expanded_groups.contains(&group.group_id);
        let idx_in_visible = i - start_idx;
        let is_selected = idx_in_visible == app.selected_group_index;
        
        let expand_indicator = if is_expanded { "▼" } else { "▶" };
        let memory_mb = group.total_memory / (1024 * 1024);
        
        // Get display name for container groups, namespace groups, and username groups
        let display_name = match &app.grouped_view_type {
            GroupType::Container => {
                if group.group_id == "No container" {
                    "No container".to_string()
                } else {
                    use crate::container_view::get_container_name;
                    get_container_name(&group.group_id)
                }
            }
            GroupType::Namespace(ns_type) => {
                // For namespace groups, show a cleaner format
                // group_id format is "namespace_type:namespace_id"
                // Note: "None" groups are no longer created to avoid namespace ID 0 collision
                if let Some(id_str) = group.group_id.split(':').nth(1) {
                    format!("{}: {}", ns_type, id_str)
                } else {
                    // Fallback to full group_id if parsing fails (shouldn't happen)
                    group.group_id.clone()
                }
            }
            GroupType::Username => {
                // For username groups, the group_id is already the username
                group.group_id.clone()
            }
            _ => group.group_id.clone(),
        };
        
        let line = format!("{} {} | CPU: {:.1}% | MEM: {}MB | Processes: {}", 
            expand_indicator, display_name, group.total_cpu, memory_mb, group.process_count());
        
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black)
        };
        
        items.push(ListItem::new(Span::styled(line, style)));
        
        // If expanded, show processes in the group (sorted by CPU descending)
        if is_expanded {
            let mut sorted_procs = group.processes.clone();
            sorted_procs.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
            for process in &sorted_procs {
                let proc_line = format!("  └─ {} (PID: {}) | CPU: {:.1}% | MEM: {}MB",
                    process.name, process.pid, process.cpu_usage, process.memory_usage / (1024 * 1024));
                items.push(ListItem::new(Span::styled(proc_line, Style::default().fg(Color::Cyan))));
            }
        }
    }

    // Update title to show freeze status
    let title_text = if app.group_view_frozen {
        "Groups (Enter: expand/collapse, 1/2/3: switch type, [f]: freeze/unfreeze) [FROZEN]"
    } else {
        "Groups (Enter: expand/collapse, 1/2/3: switch type, [f]: freeze/unfreeze)"
    };
    
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title_text))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Menu
    let menu_text = vec![
        Line::from(vec![
            Span::styled("[↑/↓] Navigate  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[Enter] Expand/Collapse  ", Style::default().fg(Color::Yellow)),
            Span::raw("| "),
            Span::styled("[1] Cgroup  ", Style::default().fg(Color::Green)),
            Span::raw("| "),
            Span::styled("[2] Container  ", Style::default().fg(Color::Blue)),
            Span::raw("| "),
            Span::styled("[3] Namespace  ", Style::default().fg(Color::Magenta)),
            Span::raw("| "),
            Span::styled("[4] Username  ", Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("[f] Freeze  ", Style::default().fg(Color::Red)),
            Span::raw("| "),
            Span::styled("[Esc] Back", Style::default().fg(Color::Black)),
        ]),
    ];
    let menu = Paragraph::new(menu_text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[2]);
}

// Handle keyboard input for grouped view
fn handle_grouped_view_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    use crate::process_group::{ProcessGroupManager, GroupType};
    
    let processes = app.process_manager.get_processes();
    let mut groups: Vec<crate::process_group::ProcessGroup> = match app.grouped_view_type {
        GroupType::Cgroup => ProcessGroupManager::group_by_cgroup(processes),
        GroupType::Container => ProcessGroupManager::group_by_container(processes),
        GroupType::Namespace(ref ns_type) => ProcessGroupManager::group_by_namespace(processes, ns_type),
        GroupType::Username => ProcessGroupManager::group_by_username(processes),
    };
    
    // Sort groups the same way as in draw_grouped_view to ensure index matching
    if app.group_view_frozen && !app.frozen_group_order.is_empty() {
        // Maintain frozen order
        let mut frozen_groups = Vec::new();
        
        for group_id in &app.frozen_group_order {
            if let Some(pos) = groups.iter().position(|g| &g.group_id == group_id) {
                frozen_groups.push(groups.remove(pos));
            }
        }
        // Sort remaining groups by CPU
        groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
        let mut final_groups = frozen_groups;
        final_groups.extend(groups);
        groups = final_groups;
    } else if !app.expanded_groups.is_empty() && !app.frozen_group_order.is_empty() {
        // Auto-stabilize: maintain order for expanded groups
        let mut stable_groups = Vec::new();
        
        for group_id in &app.frozen_group_order {
            if app.expanded_groups.contains(group_id) {
                if let Some(pos) = groups.iter().position(|g| &g.group_id == group_id) {
                    stable_groups.push(groups.remove(pos));
                }
            }
        }
        groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
        let mut final_groups = stable_groups;
        final_groups.extend(groups);
        groups = final_groups;
    } else {
        // Normal sort by CPU usage
        groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
    }
    
    let num_groups = groups.len();
    
    // Convert visible index to actual index in sorted groups (accounting for scroll offset)
    let actual_selected_index = app.grouped_view_scroll_offset + app.selected_group_index;
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
            app.expanded_groups.clear();
            app.group_view_frozen = false;
            app.frozen_group_order.clear(); // Clear frozen groups when leaving grouped view
        }
        KeyCode::Up => {
            if app.selected_group_index > 0 {
                app.selected_group_index -= 1;
            } else if app.grouped_view_scroll_offset > 0 {
                // Scroll up
                app.grouped_view_scroll_offset -= 1;
                // Keep selected_group_index at 0 (top of visible area)
            }
        }
        KeyCode::Down => {
            // Check if we can move down within visible groups
            let visible_height = 10; // Approximate visible height
            let max_visible_index = visible_height.min(num_groups.saturating_sub(app.grouped_view_scroll_offset));
            
            if app.selected_group_index + 1 < max_visible_index {
                app.selected_group_index += 1;
            } else if actual_selected_index + 1 < num_groups {
                // Move to next group and adjust scroll
                app.grouped_view_scroll_offset += 1;
                // Keep selected_group_index at the bottom of visible area
                app.selected_group_index = (max_visible_index - 1).min(visible_height - 1);
            }
        }
        KeyCode::Enter => {
            // Toggle expand/collapse or drill down
            // Use actual index accounting for scroll offset, but ensure it's within bounds
            let safe_index = actual_selected_index.min(num_groups.saturating_sub(1));
            if let Some(group) = groups.get(safe_index) {
                match &app.grouped_view_type {
                    GroupType::Container => {
                        // Drill down to container detail
                        app.selected_container_id = Some(group.group_id.clone());
                        app.view_mode = ViewMode::ContainerDetail;
                        app.detail_view_scroll_offset = 0;
                    }
                    GroupType::Namespace(ns_type) => {
                        // Drill down to namespace detail
                        // Extract namespace ID from group_id (format: "type:id")
                        // Note: group_id should always be "type:id" format since we removed "None" groups
                        if let Some(id_str) = group.group_id.split(':').nth(1) {
                            if let Ok(ns_id) = id_str.parse::<u64>() {
                                app.selected_namespace = Some((ns_type.clone(), ns_id));
                                app.view_mode = ViewMode::NamespaceDetail;
                                app.detail_view_scroll_offset = 0;
                            } else {
                                // Invalid namespace ID format - this shouldn't happen with current logic
                                app.input_state.message = Some((
                                    format!("Invalid namespace ID format: {}", id_str),
                                    true
                                ));
                            }
                        } else {
                            // Malformed group_id - this shouldn't happen
                            app.input_state.message = Some((
                                format!("Invalid namespace group format: {}", group.group_id),
                                true
                            ));
                        }
                    }
                    GroupType::Cgroup | GroupType::Username => {
                        // Toggle expand/collapse for cgroups and username groups
                        if app.expanded_groups.contains(&group.group_id) {
                            app.expanded_groups.remove(&group.group_id);
                        } else {
                            app.expanded_groups.insert(group.group_id.clone());
                        }
                    }
                }
            }
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            // Toggle freeze/unfreeze group order
            app.group_view_frozen = !app.group_view_frozen;
            if app.group_view_frozen {
                // Freeze current order
                let current_groups: Vec<crate::process_group::ProcessGroup> = match app.grouped_view_type {
                    GroupType::Cgroup => ProcessGroupManager::group_by_cgroup(processes),
                    GroupType::Container => ProcessGroupManager::group_by_container(processes),
                    GroupType::Namespace(ref ns_type) => ProcessGroupManager::group_by_namespace(processes, ns_type),
                    GroupType::Username => ProcessGroupManager::group_by_username(processes),
                };
                let mut sorted = current_groups;
                sorted.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap_or(std::cmp::Ordering::Equal));
                app.frozen_group_order = sorted.iter().map(|g| g.group_id.clone()).collect();
                app.input_state.message = Some(("Group order frozen - expanded groups will stay in place".to_string(), false));
            } else {
                app.frozen_group_order.clear();
                app.input_state.message = Some(("Group order unfrozen - groups will sort by CPU".to_string(), false));
            }
        }
        KeyCode::Char('1') => {
            app.grouped_view_type = GroupType::Cgroup;
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
            app.current_namespace_type = None;
            app.group_view_frozen = false;
            app.frozen_group_order.clear();
        }
        KeyCode::Char('2') => {
            app.grouped_view_type = GroupType::Container;
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
            app.current_namespace_type = None;
            app.group_view_frozen = false;
            app.frozen_group_order.clear();
        }
        KeyCode::Char('4') => {
            app.grouped_view_type = GroupType::Username;
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
            app.current_namespace_type = None;
            app.group_view_frozen = false;
            app.frozen_group_order.clear();
        }
        KeyCode::Char('3') => {
            // Switch to namespace grouping - cycle through available namespace types
            let ns_types = ProcessGroupManager::get_available_namespace_types(processes);
            if ns_types.is_empty() {
                app.input_state.message = Some(("No namespace types available".to_string(), true));
            } else {
                // If already in namespace mode, cycle to next namespace type
                let current_ns = match &app.grouped_view_type {
                    GroupType::Namespace(ns) => Some(ns.clone()),
                    _ => None,
                };
                
                if let Some(current) = current_ns {
                    // Find current index and move to next
                    if let Some(current_idx) = ns_types.iter().position(|ns| ns == &current) {
                        let next_idx = (current_idx + 1) % ns_types.len();
                        app.grouped_view_type = GroupType::Namespace(ns_types[next_idx].clone());
                        app.current_namespace_type = Some(ns_types[next_idx].clone());
                    } else {
                        // Current not found, use first
                        app.grouped_view_type = GroupType::Namespace(ns_types[0].clone());
                        app.current_namespace_type = Some(ns_types[0].clone());
                    }
                } else {
                    // Not in namespace mode, switch to first namespace type
                    app.grouped_view_type = GroupType::Namespace(ns_types[0].clone());
                    app.current_namespace_type = Some(ns_types[0].clone());
                }
            }
            app.selected_group_index = 0;
            app.grouped_view_scroll_offset = 0;
        }
        _ => {}
    }
    Ok(false)
}

// Handle keyboard input for container detail view
fn handle_container_detail_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    use crate::container_view::get_container_details;
    
    match key.code {
        KeyCode::Esc => {
            // Always go back to grouped view when Esc is pressed
            app.view_mode = ViewMode::GroupedView;
            app.detail_view_scroll_offset = 0;
            return Ok(false); // Key was handled, but don't exit app
        }
        KeyCode::Up => {
            let processes = app.process_manager.get_processes();
            if let Some(container_id) = &app.selected_container_id {
                if let Some(container) = get_container_details(processes, container_id) {
                    let num_processes = container.processes.len();
                    let visible_height = 10; // Approximate
                    app.detail_view_scroll_offset = app.detail_view_scroll_offset.saturating_sub(1)
                        .min(num_processes.saturating_sub(visible_height));
                    return Ok(false); // Key handled, don't exit
                }
            }
        }
        KeyCode::Down => {
            let processes = app.process_manager.get_processes();
            if let Some(container_id) = &app.selected_container_id {
                if let Some(container) = get_container_details(processes, container_id) {
                    let num_processes = container.processes.len();
                    let visible_height = 10; // Approximate
                    let max_scroll = num_processes.saturating_sub(visible_height);
                    app.detail_view_scroll_offset = (app.detail_view_scroll_offset + 1).min(max_scroll);
                    return Ok(false); // Key handled, don't exit
                }
            }
        }
        KeyCode::PageUp => {
            let processes = app.process_manager.get_processes();
            if let Some(container_id) = &app.selected_container_id {
                if let Some(container) = get_container_details(processes, container_id) {
                    let num_processes = container.processes.len();
                    let visible_height = 10; // Approximate
                    app.detail_view_scroll_offset = app.detail_view_scroll_offset.saturating_sub(visible_height)
                        .min(num_processes.saturating_sub(visible_height));
                    return Ok(false); // Key handled, don't exit
                }
            }
        }
        KeyCode::PageDown => {
            let processes = app.process_manager.get_processes();
            if let Some(container_id) = &app.selected_container_id {
                if let Some(container) = get_container_details(processes, container_id) {
                    let num_processes = container.processes.len();
                    let visible_height = 10; // Approximate
                    let max_scroll = num_processes.saturating_sub(visible_height);
                    app.detail_view_scroll_offset = (app.detail_view_scroll_offset + visible_height).min(max_scroll);
                    return Ok(false); // Key handled, don't exit
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// Handle keyboard input for namespace detail view
fn handle_namespace_detail_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    use crate::namespace_view::get_namespace_group_details;
    
    let processes = app.process_manager.get_processes();
    if let Some((ns_type, ns_id)) = &app.selected_namespace {
        if let Some(group) = get_namespace_group_details(processes, ns_type, *ns_id) {
            let num_processes = group.processes.len();
            let visible_height = 10; // Approximate
            
            match key.code {
                KeyCode::Esc => {
                    app.view_mode = ViewMode::GroupedView;
                    app.detail_view_scroll_offset = 0;
                }
                KeyCode::Up => {
                    app.detail_view_scroll_offset = app.detail_view_scroll_offset.saturating_sub(1)
                        .min(num_processes.saturating_sub(visible_height));
                }
                KeyCode::Down => {
                    let max_scroll = num_processes.saturating_sub(visible_height);
                    app.detail_view_scroll_offset = (app.detail_view_scroll_offset + 1).min(max_scroll);
                }
                KeyCode::PageUp => {
                    app.detail_view_scroll_offset = app.detail_view_scroll_offset.saturating_sub(visible_height)
                        .min(num_processes.saturating_sub(visible_height));
                }
                KeyCode::PageDown => {
                    let max_scroll = num_processes.saturating_sub(visible_height);
                    app.detail_view_scroll_offset = (app.detail_view_scroll_offset + visible_height).min(max_scroll);
                }
                _ => {}
            }
        }
    } else {
        // No namespace selected, just go back
        if key.code == KeyCode::Esc {
            app.view_mode = ViewMode::GroupedView;
        }
    }
    Ok(false)
}

// Draw scheduler view
fn draw_scheduler_view(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::scheduler::{ScheduleType, ScheduleAction};
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Percentage(60), // Task list
            Constraint::Percentage(40), // Log
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Header
    let title = Paragraph::new("Job Scheduler")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Task list
    let tasks = app.scheduler.get_tasks();
    let visible_height = chunks[1].height as usize - 2;
    let start_idx = app.scheduler_scroll_offset.min(tasks.len().saturating_sub(visible_height));
    let end_idx = (start_idx + visible_height).min(tasks.len());

    let mut items = Vec::new();
    for (i, task) in tasks.iter().enumerate().skip(start_idx).take(end_idx - start_idx) {
        let idx_in_visible = i - start_idx;
        let is_selected = idx_in_visible == app.selected_task_index;
        
        let status = if task.enabled { "✓" } else { "✗" };
        let schedule_str = match &task.schedule {
            ScheduleType::Cron(expr) => format!("Cron: {}", expr),
            ScheduleType::Interval(secs) => format!("Every {}s", secs),
            ScheduleType::Once(_) => "Once".to_string(),
        };
        
        let action_str = match &task.action {
            ScheduleAction::RestartProcess { pattern } => format!("Restart: {}", pattern),
            ScheduleAction::StartProcess { program, args } => {
                if args.is_empty() {
                    format!("Start: {}", program)
                } else {
                    format!("Start: {} {}", program, args.join(" "))
                }
            }
            ScheduleAction::CleanupIdle { cpu_threshold, memory_threshold, action, .. } => {
                format!("Cleanup: CPU<{}%, MEM>{}MB, {}", 
                    cpu_threshold, memory_threshold / (1024*1024), action)
            }
            ScheduleAction::ApplyRule { rule } => format!("Rule: {}", rule),
            ScheduleAction::KillProcess { pid } => format!("Kill PID: {}", pid),
            ScheduleAction::StopProcess { pid } => format!("Stop PID: {}", pid),
            ScheduleAction::ContinueProcess { pid } => format!("Continue PID: {}", pid),
            ScheduleAction::ReniceProcess { pid, nice } => format!("Renice PID: {} to {}", pid, nice),
        };
        
        let line = format!("{} {} | {} | {}", status, task.name, schedule_str, action_str);
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if task.enabled {
            Style::default().fg(Color::Black)
        } else {
            Style::default().fg(Color::Black)
        };
        
        items.push(ListItem::new(Span::styled(line, style)));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Scheduled Tasks (Enter: toggle, A/+: add, -: delete)").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Log
    let log = app.scheduler.get_task_log();
    let log_items: Vec<ListItem> = log.iter().rev().take(20)
        .map(|(name, time, result)| {
            let time_str = format!("{}", chrono::DateTime::<chrono::Local>::from(*time).format("%H:%M:%S"));
            let line = format!("[{}] {}: {}", time_str, name, result);
            ListItem::new(Span::styled(line, Style::default().fg(Color::Cyan)))
        })
        .collect();
    
    let log_list = List::new(log_items)
        .block(Block::default().borders(Borders::ALL).title("Task Execution Log").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(log_list, chunks[2]);

    // Menu
    let menu = Paragraph::new("↑/↓: Navigate  |  [Enter] Toggle  |  [A/+] Add  |  [-] Delete  |  [Esc] Back  |  [S] Save")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Handle keyboard input for scheduler view
fn handle_scheduler_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let tasks = app.scheduler.get_tasks();
    let num_tasks = tasks.len();
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
            app.selected_task_index = 0;
            app.scheduler_scroll_offset = 0;
        }
        KeyCode::Up => {
            if app.selected_task_index > 0 {
                app.selected_task_index -= 1;
                if app.selected_task_index < app.scheduler_scroll_offset {
                    app.scheduler_scroll_offset = app.selected_task_index;
                }
            }
        }
        KeyCode::Down => {
            if app.selected_task_index + 1 < num_tasks {
                app.selected_task_index += 1;
                let visible_height = 10;
                if app.selected_task_index >= app.scheduler_scroll_offset + visible_height {
                    app.scheduler_scroll_offset = app.selected_task_index - visible_height + 1;
                }
            }
        }
        KeyCode::Enter => {
            // Toggle task enabled/disabled
            app.scheduler.toggle_task(app.selected_task_index);
        }
        KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('+') | KeyCode::Char('=') => {
            // Open task editor to create new task
            app.view_mode = ViewMode::TaskEditor;
            app.input_state.task_name.clear();
            app.input_state.task_schedule_type.clear();
            app.input_state.task_schedule_value.clear();
            app.input_state.task_action_type.clear();
            app.input_state.task_action_value.clear();
            app.input_state.current_task_field = 0;
        }
        KeyCode::Char('-') => {
            // Delete selected task
            if app.selected_task_index < num_tasks {
                app.scheduler.remove_task(app.selected_task_index);
                if app.selected_task_index >= app.scheduler.get_tasks().len() && app.selected_task_index > 0 {
                    app.selected_task_index -= 1;
                }
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Save tasks to config file
            let tasks = app.scheduler.get_tasks();
            match crate::scheduler::save_tasks(tasks) {
                Ok(_) => {
                    app.input_state.message = Some(("Tasks saved successfully".to_string(), false));
                }
                Err(e) => {
                    app.input_state.message = Some((format!("Error saving tasks: {}", e), true));
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// Draw start process menu
fn draw_start_process_menu(f: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::layout::Rect;
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(10), // Input fields - increased from 8 to 10
            Constraint::Min(5),     // Instructions
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Start New Process")
        .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, chunks[0]);

    // Input fields
    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Program path - increased to 3 for text visibility
            Constraint::Length(3),  // Working directory - increased to 3
            Constraint::Length(3),  // Arguments - increased to 3
            Constraint::Length(0),  // Removed extra spacer
        ])
        .split(chunks[1]);

    let fields = [
        ("Program Path", &app.input_state.program_path, 0),
        ("Working Directory (optional)", &app.input_state.working_dir, 1),
        ("Arguments (space-separated)", &app.input_state.arguments, 2),
    ];

    for (i, (label, value, field_idx)) in fields.iter().enumerate() {
        let is_active = app.input_state.current_start_input_field == *field_idx;
        let style = if is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black)
        };
        // Add cursor indicator when field is active
        let cursor = if is_active { "_" } else { "" };
        let content = format!("{}: {}{}", label, value, cursor);
        let para = Paragraph::new(content)
            .style(style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(para, field_chunks[i]);
    }

    // Instructions
    let instructions = vec![
        Line::from(vec![Span::styled("Instructions:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::raw("1. Enter program path (e.g., /usr/bin/sleep)")]),
        Line::from(vec![Span::raw("2. Optionally enter working directory")]),
        Line::from(vec![Span::raw("3. Optionally enter command-line arguments")]),
        Line::from(vec![Span::raw("4. Press [Tab] to switch fields, [Enter] to start process")]),
        Line::from(vec![Span::raw("5. Press [Esc] to cancel")]),
    ];
    let inst_para = Paragraph::new(instructions)
        .block(Block::default().borders(Borders::ALL).title("Instructions").style(Style::default().fg(Color::Black)));
    f.render_widget(inst_para, chunks[2]);

    // Menu
    let menu = Paragraph::new("[Tab] Next field  |  [Enter] Start  |  [Esc] Cancel")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);

    // Show message if any
    if let Some((msg, is_error)) = &app.input_state.message {
        let msg_para = Paragraph::new(msg.as_str())
            .style(if *is_error { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) })
            .block(Block::default().borders(Borders::ALL).title("Status").style(Style::default().fg(Color::Black)));
        let msg_area = Rect {
            x: size.width / 4,
            y: size.height / 2,
            width: size.width / 2,
            height: 5,
        };
        f.render_widget(msg_para, msg_area);
    }
}

// Handle keyboard input for start process view
fn handle_start_process_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Tab => {
            // Switch to next field
            app.input_state.current_start_input_field = (app.input_state.current_start_input_field + 1) % 3;
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            // Add character to current field (only if no Ctrl/Alt modifiers)
            match app.input_state.current_start_input_field {
                0 => app.input_state.program_path.push(c),
                1 => app.input_state.working_dir.push(c),
                2 => app.input_state.arguments.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            // Remove character from current field
            match app.input_state.current_start_input_field {
                0 => { app.input_state.program_path.pop(); }
                1 => { app.input_state.working_dir.pop(); }
                2 => { app.input_state.arguments.pop(); }
                _ => {}
            }
        }
        KeyCode::Enter => {
            // Start the process
            if app.input_state.program_path.is_empty() {
                app.input_state.message = Some((
                    "Error: Program path is required".to_string(),
                    true
                ));
                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
            } else {
                // Parse arguments
                let args: Vec<&str> = if app.input_state.arguments.is_empty() {
                    vec![]
                } else {
                    app.input_state.arguments.split_whitespace().collect()
                };
                
                // Parse working directory
                let working_dir = if app.input_state.working_dir.is_empty() {
                    None
                } else {
                    Some(app.input_state.working_dir.as_str())
                };
                
                // Start the process
                match app.process_manager.start_process(
                    &app.input_state.program_path,
                    &args,
                    working_dir,
                    &app.input_state.env_vars,
                ) {
                    Ok(pid) => {
                        app.input_state.message = Some((
                            format!("Successfully started process with PID: {}", pid),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                        // Clear inputs
                        app.input_state.program_path.clear();
                        app.input_state.working_dir.clear();
                        app.input_state.arguments.clear();
                        app.input_state.env_vars.clear();
                        app.input_state.current_start_input_field = 0;
                    }
                    Err(e) => {
                        app.input_state.message = Some((
                            format!("Error starting process: {}", e),
                            true
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                    }
                }
            }
        }
        KeyCode::Esc => {
            // Cancel and return to process list
            app.view_mode = ViewMode::ProcessList;
            app.input_state.program_path.clear();
            app.input_state.working_dir.clear();
            app.input_state.arguments.clear();
            app.input_state.env_vars.clear();
            app.input_state.current_start_input_field = 0;
        }
        _ => {}
    }
    Ok(false)
}

// Draw advanced filter input menu
fn draw_advanced_filter_input(f: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::layout::Rect;
    
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(5),  // Input field
            Constraint::Min(10),    // Help/Examples
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Advanced Filter")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, chunks[0]);

    // Input field
    let input_text = if app.input_state.advanced_filter_input.is_empty() {
        "Enter filter expression...".to_string()
    } else {
        app.input_state.advanced_filter_input.clone()
    };
    let input_para = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL).title("Filter Expression").style(Style::default().fg(Color::Black)));
    f.render_widget(input_para, chunks[1]);

    // Help and examples
    let help_text = vec![
        Line::from(vec![Span::styled("Syntax Help:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
        Line::from(""),
        Line::from(vec![Span::styled("Fields:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
        Line::from("  String: name, user, status"),
        Line::from("  Numeric: pid, ppid, cpu, memory, nice"),
        Line::from(""),
        Line::from(vec![Span::styled("Operators:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
        Line::from("  String: ==, !=, ~ (regex)"),
        Line::from("  Numeric: ==, !=, >, <, >=, <="),
        Line::from("  Boolean: AND, OR, NOT"),
        Line::from(""),
        Line::from(vec![Span::styled("Examples:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
        Line::from("  name ~ \"firefox|chrome\" AND cpu > 10"),
        Line::from("  user == \"root\" OR (memory > 5000 AND status == \"running\")"),
        Line::from("  NOT (pid == 1234) AND ppid == 1"),
        Line::from("  cpu > 50 AND memory < 1000"),
    ];
    let help_para = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title("Help & Examples").style(Style::default().fg(Color::Black)))
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(help_para, chunks[2]);

    // Menu
    let menu = Paragraph::new("[Enter] Apply  |  [Esc] Cancel  |  [Backspace] Delete")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);

    // Show message if any
    if let Some((msg, is_error)) = &app.input_state.message {
        let msg_para = Paragraph::new(msg.as_str())
            .style(if *is_error { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) })
            .block(Block::default().borders(Borders::ALL).title("Status").style(Style::default().fg(Color::Black)));
        let msg_area = Rect {
            x: size.width / 4,
            y: size.height / 2,
            width: size.width / 2,
            height: 5,
        };
        f.render_widget(msg_para, msg_area);
    }
}

// Handle keyboard input for advanced filter
fn handle_advanced_filter_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Char(c) => {
            app.input_state.advanced_filter_input.push(c);
        }
        KeyCode::Backspace => {
            app.input_state.advanced_filter_input.pop();
        }
        KeyCode::Enter => {
            // Apply filter
            let filter_str = app.input_state.advanced_filter_input.trim();
            if filter_str.is_empty() {
                // Clear filter
                if let Err(e) = app.process_manager.set_advanced_filter_string("") {
                    app.input_state.message = Some((
                        format!("Error: {}", e),
                        true
                    ));
                } else {
                    app.input_state.message = Some((
                        "Filter cleared".to_string(),
                        false
                    ));
                }
                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                app.view_mode = ViewMode::ProcessList;
            } else {
                match app.process_manager.set_advanced_filter_string(filter_str) {
                    Ok(_) => {
                        app.input_state.message = Some((
                            format!("Filter applied: {}", filter_str),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                        app.view_mode = ViewMode::ProcessList;
                    }
                    Err(e) => {
                        app.input_state.message = Some((
                            format!("Filter error: {}", e),
                            true
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    }
                }
            }
        }
        KeyCode::Esc => {
            // Cancel and return
            app.view_mode = ViewMode::FilterSort;
            app.input_state.advanced_filter_input.clear();
        }
        _ => {}
    }
    Ok(false)
}

// Draw profile management view
fn draw_profile_management(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Profile list
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Title
    let active_profile = app.profile_manager.get_active_profile()
        .map(|s| format!(" (Active: {})", s))
        .unwrap_or_default();
    let title = Paragraph::new(format!("Profile Management{}", active_profile))
        .style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, chunks[0]);

    // Profile list
    let profiles = app.profile_manager.get_profiles();
    let items: Vec<ListItem> = profiles.iter()
        .enumerate()
        .map(|(i, profile)| {
            let is_active = app.profile_manager.get_active_profile() == Some(profile.name.as_str());
            let is_selected = i == app.selected_profile_index;
            let prefix = if is_active { "[ACTIVE] " } else { "" };
            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Black)
            };
            ListItem::new(Span::styled(
                format!("{}{} (Prioritize: {}, Hide: {}, Nice: {})",
                    prefix,
                    profile.name,
                    profile.prioritize_processes.len(),
                    profile.hide_processes.len(),
                    profile.nice_adjustments.len()
                ),
                style
            ))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Profiles").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Menu
    let menu = Paragraph::new("[+] Create  |  [Enter] Activate/Toggle  |  [E] Edit  |  [-] Delete  |  [Esc] Back")
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[2]);
}

// Handle keyboard input for profile management
fn handle_profile_management_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let profiles = app.profile_manager.get_profiles();
    let num_profiles = profiles.len();
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Up => {
            if app.selected_profile_index > 0 {
                app.selected_profile_index -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_profile_index + 1 < num_profiles {
                app.selected_profile_index += 1;
            }
        }
        KeyCode::Char('+') => {
            // Create new profile (simplified - just create with default name)
            let new_name = format!("Profile {}", profiles.len() + 1);
            let new_profile = crate::profile::Profile::new(new_name.clone());
            app.profile_manager.add_profile(new_profile);
            app.selected_profile_index = app.profile_manager.get_profiles().len() - 1;
        }
        KeyCode::Enter => {
            // Toggle active profile
            if let Some(profile) = profiles.get(app.selected_profile_index) {
                let current_active = app.profile_manager.get_active_profile();
                if current_active == Some(profile.name.as_str()) {
                    // Deactivate
                    app.profile_manager.set_active_profile(None);
                } else {
                    // Activate
                    app.profile_manager.set_active_profile(Some(profile.name.clone()));
                    
                    // Apply nice value adjustments for this profile
                    let profile_mgr = &app.profile_manager;
                    let (_success, _fail) = app.process_manager.apply_nice_adjustments(|name| {
                        profile_mgr.get_nice_adjustment(name)
                    });
                    // Note: Not showing feedback messages to keep UI clean
                    // Users will see nice values change in the process list
                }
            }
        }
        KeyCode::Char('-') => {
            // Delete profile
            let profile_name = profiles.get(app.selected_profile_index).map(|p| p.name.clone());
            if let Some(name) = profile_name {
                app.profile_manager.remove_profile(&name);
                if app.selected_profile_index >= app.profile_manager.get_profiles().len() && app.selected_profile_index > 0 {
                    app.selected_profile_index -= 1;
                }
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            // Edit profile - load into editor
            if let Some(profile) = profiles.get(app.selected_profile_index) {
                app.profile_edit_name = profile.name.clone();
                app.profile_edit_prioritize = profile.prioritize_processes.join(", ");
                app.profile_edit_hide = profile.hide_processes.join(", ");
                // Format nice_adjustments as: "name1:10, name2:5"
                app.profile_edit_nice = profile.nice_adjustments.iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ");
                app.view_mode = ViewMode::ProfileEditor;
                app.profile_edit_mode = true;
                app.profile_edit_current_field = 0;
            }
        }
        _ => {}
    }
    Ok(false)
}




// Draw profile editor
fn draw_profile_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default() // Removed `let size = area;` as `area` can be used directly
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Min(3),
        ])
        .split(area); // Changed `split(size)` to `split(area)`

    let title = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Edit Profile: {} ", app.profile_edit_name))
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(title, chunks[0]);

    // Helper to get style for field
    let get_style = |idx: usize, default_color: Color| {
        if app.profile_edit_current_field == idx {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(default_color)
        }
    };

    let prioblk = Block::default().borders(Borders::ALL)
        .title(" Prioritize (comma-separated) ").style(Style::default().fg(Color::Black))
        .border_style(get_style(0, Color::Green));
    let prio = Paragraph::new(app.profile_edit_prioritize.as_str())
        .block(prioblk).style(get_style(0, Color::Green));
    f.render_widget(prio, chunks[1]);

    let hideblk = Block::default().borders(Borders::ALL)
        .title(" Hide (comma-separated) ").style(Style::default().fg(Color::Black))
        .border_style(get_style(1, Color::Red));
    let hide = Paragraph::new(app.profile_edit_hide.as_str())
        .block(hideblk).style(get_style(1, Color::Red));
    f.render_widget(hide, chunks[2]);

    let niceblk = Block::default().borders(Borders::ALL)
        .title(" Nice (name:val, name:val) ").style(Style::default().fg(Color::Black))
        .border_style(get_style(2, Color::Magenta));
    let nice = Paragraph::new(app.profile_edit_nice.as_str())
        .block(niceblk).style(get_style(2, Color::Magenta));
    f.render_widget(nice, chunks[3]);

    let inst = Paragraph::new(
        "Type to edit. [Tab] Next Field. [Enter] Save  |  [Esc] Cancel"
    )
    .block(Block::default().borders(Borders::ALL).title(" Instructions ").style(Style::default().fg(Color::Black)))
    .style(Style::default().fg(Color::Black));
    f.render_widget(inst, chunks[4]);
}

fn handle_profile_editor_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Tab => {
            // Cycle through fields: 0 -> 1 -> 2 -> 0
            app.profile_edit_current_field = (app.profile_edit_current_field + 1) % 3;
        }
        KeyCode::BackTab => {
            // Cycle backwards
            if app.profile_edit_current_field == 0 {
                app.profile_edit_current_field = 2;
            } else {
                app.profile_edit_current_field -= 1;
            }
        }
        KeyCode::Enter => {
            let prio: Vec<String> = app.profile_edit_prioritize.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let hide: Vec<String> = app.profile_edit_hide.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let nice: std::collections::HashMap<String, i32> = app.profile_edit_nice.split(',').filter_map(|s| {
                let p: Vec<&str> = s.split(':').collect();
                if p.len() == 2 { Some((p[0].trim().to_string(), p[1].trim().parse::<i32>().ok()?)) } else { None }
            }).collect();
            
            let prof = crate::profile::Profile {
                name: app.profile_edit_name.clone(),
                prioritize_processes: prio,
                hide_processes: hide,
                nice_adjustments: nice,
            };
            app.profile_manager.add_profile(prof);
            app.view_mode = ViewMode::ProfileManagement;
            app.input_state.message = Some(("Profile saved".to_string(), false));
            app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProfileManagement;
        }
        KeyCode::Char(c) => {
            match app.profile_edit_current_field {
                0 => app.profile_edit_prioritize.push(c),
                1 => app.profile_edit_hide.push(c),
                2 => app.profile_edit_nice.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match app.profile_edit_current_field {
                0 => { app.profile_edit_prioritize.pop(); },
                1 => { app.profile_edit_hide.pop(); },
                2 => { app.profile_edit_nice.pop(); },
                _ => {}
            }
        }
        _ => {}
    }
    Ok(false)
}

// Draw alert management view
fn draw_alert_management(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(8),   // Alert list (Reduced to give more space to active alerts)
            Constraint::Min(15),    // Active alerts (Increased)
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Title
    let active_count = app.alert_manager.get_active_alerts().len();
    let title_text = if active_count > 0 {
        format!("Alert Management ({} Active)", active_count)
    } else {
        "Alert Management".to_string()
    };
    let title = Paragraph::new(title_text)
        .style(Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick).style(Style::default().fg(Color::Black)));
    f.render_widget(title, chunks[0]);

    // Alert list
    let alerts = app.alert_manager.get_alerts();
    let items: Vec<ListItem> = alerts.iter()
        .enumerate()
        .map(|(i, alert)| {
            let is_selected = i == app.selected_alert_index;
            let status = if alert.enabled { "[ENABLED]" } else { "[DISABLED]" };
            let condition_str = match &alert.condition {
                crate::alert::AlertCondition::CpuGreaterThan { threshold, duration_secs } => {
                    format!("CPU > {}% for {}s", threshold, duration_secs)
                }
                crate::alert::AlertCondition::MemoryGreaterThan { threshold_mb, duration_secs } => {
                    format!("Memory > {}MB for {}s", threshold_mb, duration_secs)
                }
                crate::alert::AlertCondition::IoGreaterThan { threshold_mb_per_sec, duration_secs } => {
                    format!("I/O > {}MB/s for {}s", threshold_mb_per_sec, duration_secs)
                }
                crate::alert::AlertCondition::ProcessDied { pattern } => {
                    format!("Process died: {}", pattern)
                }
            };
            let style = if is_selected {
                Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if alert.enabled {
                Style::default().fg(Color::Black)
            } else {
                Style::default().fg(Color::Black)
            };
            ListItem::new(Span::styled(
                format!("{} {}: {}", status, alert.name, condition_str),
                style
            ))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Alerts").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Active alerts
    let active_alerts = app.alert_manager.get_active_alerts();
    // Show newest first
    let alert_items: Vec<ListItem> = active_alerts.iter()
        .rev() // Reverse iterator
        .take(50) // Limit to 50 most recent
        .map(|alert| {
            ListItem::new(Span::styled(
                format!("⚠️  {}: {}", alert.alert_name, alert.message),
                Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)
            ))
        })
        .collect();

    let alert_list = List::new(alert_items)
        .block(Block::default().borders(Borders::ALL).title("Active Alerts").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(alert_list, chunks[2]);

    // Menu
    let menu = Paragraph::new("[c] CPU | [m] Mem | [d] Death | [Enter] Toggle | [e] Edit | [-] Delete | [C] Clear Active | [Esc] Back")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Handle keyboard input for alert management
fn handle_alert_management_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let alerts = app.alert_manager.get_alerts();
    let num_alerts = alerts.len();
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Up => {
            if app.selected_alert_index > 0 {
                app.selected_alert_index -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_alert_index + 1 < num_alerts {
                app.selected_alert_index += 1;
            }
        }
        KeyCode::Char('c') => {
            // Create CPU alert (Low threshold for testing)
            let new_alert = crate::alert::Alert {
                name: format!("High CPU Alert {}", alerts.len() + 1),
                condition: crate::alert::AlertCondition::CpuGreaterThan {
                    threshold: 5.0, // 5% CPU
                    duration_secs: 5, // 5 seconds
                },
                target: crate::alert::AlertTarget::All,
                enabled: true,
            };
            app.alert_manager.add_alert(new_alert);
            app.selected_alert_index = app.alert_manager.get_alerts().len() - 1;
        }
        KeyCode::Char('m') => {
            // Create Memory alert
            let new_alert = crate::alert::Alert {
                name: format!("High Memory Alert {}", alerts.len() + 1),
                condition: crate::alert::AlertCondition::MemoryGreaterThan {
                    threshold_mb: 100, // 100 MB
                    duration_secs: 5,
                },
                target: crate::alert::AlertTarget::All,
                enabled: true,
            };
            app.alert_manager.add_alert(new_alert);
            app.selected_alert_index = app.alert_manager.get_alerts().len() - 1;
        }
        KeyCode::Char('d') => {
            // Create Process Death alert (Targeting 'sleep')
            let new_alert = crate::alert::Alert {
                name: format!("Sleep Death Alert {}", alerts.len() + 1),
                condition: crate::alert::AlertCondition::ProcessDied {
                    pattern: "sleep".to_string(),
                },
                target: crate::alert::AlertTarget::Pattern("sleep".to_string()),
                enabled: true,
            };
            app.alert_manager.add_alert(new_alert);
            app.selected_alert_index = app.alert_manager.get_alerts().len() - 1;
        }
        KeyCode::Enter => {
            // Toggle alert
            app.alert_manager.toggle_alert(app.selected_alert_index);
        }
        KeyCode::Char('e') => {
            // Edit alert
            if let Some(alert) = alerts.get(app.selected_alert_index) {
                app.alert_edit_mode = true;
                app.alert_edit_name = alert.name.clone();
                app.alert_edit_current_field = 0;
                
                match &alert.condition {
                    crate::alert::AlertCondition::CpuGreaterThan { threshold, duration_secs } => {
                        app.alert_edit_threshold = threshold.to_string();
                        app.alert_edit_duration = duration_secs.to_string();
                    }
                    crate::alert::AlertCondition::MemoryGreaterThan { threshold_mb, duration_secs } => {
                        app.alert_edit_threshold = threshold_mb.to_string();
                        app.alert_edit_duration = duration_secs.to_string();
                    }
                    crate::alert::AlertCondition::ProcessDied { pattern: _ } => {
                        app.alert_edit_threshold = "N/A".to_string();
                        app.alert_edit_duration = "N/A".to_string();
                    }
                    crate::alert::AlertCondition::IoGreaterThan { threshold_mb_per_sec, duration_secs } => {
                        app.alert_edit_threshold = threshold_mb_per_sec.to_string();
                        app.alert_edit_duration = duration_secs.to_string();
                    }
                }
                app.view_mode = ViewMode::AlertEditor;
            }
        }
        KeyCode::Char('-') => {
            // Delete alert
            app.alert_manager.remove_alert(app.selected_alert_index);
            if app.selected_alert_index >= app.alert_manager.get_alerts().len() && app.selected_alert_index > 0 {
                app.selected_alert_index -= 1;
            }
        }
        KeyCode::Char('C') => {
            // Clear all active alerts
            app.alert_manager.clear_all_active_alerts();
        }
        _ => {}
    }
    Ok(false)
}



fn draw_alert_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Name
            Constraint::Length(3), // Threshold
            Constraint::Length(3), // Duration
            Constraint::Min(1),    // Instructions
        ])
        .split(area);

    let title = Paragraph::new("Edit Alert")
        .style(Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)));
    f.render_widget(title, chunks[0]);

    let get_style = |idx: usize, color: Color| {
        if app.alert_edit_current_field == idx {
            Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black)
        }
    };

    let name_blk = Block::default().borders(Borders::ALL)
        .title(" Name ").style(Style::default().fg(Color::Black))
        .border_style(get_style(0, Color::Cyan));
    let name = Paragraph::new(app.alert_edit_name.as_str())
        .block(name_blk).style(get_style(0, Color::Cyan));
    f.render_widget(name, chunks[1]);

    let thresh_blk = Block::default().borders(Borders::ALL)
        .title(" Threshold (CPU % or Mem MB) ").style(Style::default().fg(Color::Black))
        .border_style(get_style(1, Color::Green));
    let thresh = Paragraph::new(app.alert_edit_threshold.as_str())
        .block(thresh_blk).style(get_style(1, Color::Green));
    f.render_widget(thresh, chunks[2]);

    let dur_blk = Block::default().borders(Borders::ALL)
        .title(" Duration (seconds) ").style(Style::default().fg(Color::Black))
        .border_style(get_style(2, Color::Magenta));
    let dur = Paragraph::new(app.alert_edit_duration.as_str())
        .block(dur_blk).style(get_style(2, Color::Magenta));
    f.render_widget(dur, chunks[3]);

    let inst = Paragraph::new(
        "Type to edit. [Tab] Next Field. [Enter] Save  |  [Esc] Cancel"
    )
    .block(Block::default().borders(Borders::ALL).title(" Instructions ").style(Style::default().fg(Color::Black)))
    .style(Style::default().fg(Color::Black));
    f.render_widget(inst, chunks[4]);
}

fn handle_alert_editor_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::AlertManagement;
            app.alert_edit_mode = false;
        }
        KeyCode::Tab => {
            app.alert_edit_current_field = (app.alert_edit_current_field + 1) % 3;
        }
        KeyCode::BackTab => {
            if app.alert_edit_current_field == 0 {
                app.alert_edit_current_field = 2;
            } else {
                app.alert_edit_current_field -= 1;
            }
        }
        KeyCode::Enter => {
            // Save changes
            if let Some(alert) = app.alert_manager.get_alerts_mut().get_mut(app.selected_alert_index) {
                alert.name = app.alert_edit_name.clone();
                
                // Parse threshold and duration
                let threshold_val = app.alert_edit_threshold.parse::<f32>().unwrap_or(0.0);
                let duration_val = app.alert_edit_duration.parse::<u64>().unwrap_or(0);
                
                match &mut alert.condition {
                    crate::alert::AlertCondition::CpuGreaterThan { threshold, duration_secs } => {
                        *threshold = threshold_val;
                        *duration_secs = duration_val;
                    }
                    crate::alert::AlertCondition::MemoryGreaterThan { threshold_mb, duration_secs } => {
                        *threshold_mb = threshold_val as u64;
                        *duration_secs = duration_val;
                    }
                    _ => {} // ProcessDied doesn't use these fields currently
                }
            }
            app.view_mode = ViewMode::AlertManagement;
            app.alert_edit_mode = false;
        }
        KeyCode::Char(c) => {
            match app.alert_edit_current_field {
                0 => app.alert_edit_name.push(c),
                1 => app.alert_edit_threshold.push(c),
                2 => app.alert_edit_duration.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match app.alert_edit_current_field {
                0 => { app.alert_edit_name.pop(); },
                1 => { app.alert_edit_threshold.pop(); },
                2 => { app.alert_edit_duration.pop(); },
                _ => {}
            }
        }
        _ => {}
    }
    Ok(false)
}

// Draw checkpoint management view
fn draw_checkpoint_management(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(15),    // Checkpoint list
            Constraint::Length(3),  // Status/Menu
        ])
        .split(size);

    // Title
    let criu_status = if app.criu_manager.is_available() {
        " (CRIU Available)"
    } else {
        " (CRIU Not Available - Install CRIU to use checkpoints)"
    };
    let title = Paragraph::new(format!("Checkpoint Management{}", criu_status))
        .style(Style::default().fg(if app.criu_manager.is_available() { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, chunks[0]);

    // Checkpoint list
    let checkpoints = app.criu_manager.list_checkpoints();
    let items: Vec<ListItem> = checkpoints.iter()
        .enumerate()
        .map(|(i, checkpoint)| {
            let is_selected = i == app.selected_checkpoint_index;
            let time_str = format!("Created: {:?}", checkpoint.created_at);
            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Black)
            };
            ListItem::new(Span::styled(
                format!("{} | PID: {} | {} | {}", 
                    checkpoint.checkpoint_id,
                    checkpoint.pid,
                    checkpoint.process_name,
                    time_str
                ),
                style
            ))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Checkpoints").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Menu
    let menu_text = if app.criu_manager.is_available() {
        "[+] Create Checkpoint  |  [Enter] Restore  |  [-] Delete  |  [Esc] Back"
    } else {
        "CRIU not available. Install CRIU to use checkpoint features.  |  [Esc] Back"
    };
    let menu = Paragraph::new(menu_text)
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[2]);
}

// Handle keyboard input for checkpoint management
fn handle_checkpoint_management_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    if !app.criu_manager.is_available() {
        match key.code {
            KeyCode::Esc => {
                app.view_mode = ViewMode::ProcessList;
            }
            _ => {}
        }
        return Ok(false);
    }
    
    let checkpoints = app.criu_manager.list_checkpoints();
    let num_checkpoints = checkpoints.len();
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
        }
        KeyCode::Up => {
            if app.selected_checkpoint_index > 0 {
                app.selected_checkpoint_index -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_checkpoint_index + 1 < num_checkpoints {
                app.selected_checkpoint_index += 1;
            }
        }
        KeyCode::Char('+') => {
            // Create checkpoint for selected process
            let processes = app.process_manager.get_processes();
            if let Some(process) = processes.get(app.selected_process_index) {
                match app.criu_manager.checkpoint_process(
                    process.pid,
                    &process.name,
                    None
                ) {
                    Ok(checkpoint) => {
                        app.input_state.message = Some((
                            format!("Checkpoint created: {} for PID {}", checkpoint.checkpoint_id, process.pid),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    }
                    Err(e) => {
                        app.input_state.message = Some((
                            format!("Failed to create checkpoint: {}", e),
                            true
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    }
                }
            } else {
                app.input_state.message = Some((
                    "No process selected. Please select a process first.".to_string(),
                    true
                ));
                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
            }
        }
        KeyCode::Enter => {
            // Restore checkpoint
            if let Some(checkpoint) = checkpoints.get(app.selected_checkpoint_index) {
                match app.criu_manager.restore_process(&checkpoint.checkpoint_id) {
                    Ok(pid) => {
                        app.input_state.message = Some((
                            format!("Process restored from checkpoint: {} (PID: {})", checkpoint.checkpoint_id, pid),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    }
                    Err(e) => {
                        app.input_state.message = Some((
                            format!("Failed to restore checkpoint: {}", e),
                            true
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(3));
                    }
                }
            }
        }
        KeyCode::Char('-') => {
            // Delete checkpoint
            if let Some(checkpoint) = checkpoints.get(app.selected_checkpoint_index) {
                match app.criu_manager.delete_checkpoint(&checkpoint.checkpoint_id) {
                    Ok(_) => {
                        app.input_state.message = Some((
                            format!("Checkpoint deleted: {}", checkpoint.checkpoint_id),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                        if app.selected_checkpoint_index >= app.criu_manager.list_checkpoints().len() && app.selected_checkpoint_index > 0 {
                            app.selected_checkpoint_index -= 1;
                        }
                    }
                    Err(e) => {
                        app.input_state.message = Some((
                            format!("Failed to delete checkpoint: {}", e),
                            true
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// Draw host management view
fn draw_host_management(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(15),   // Host list
            Constraint::Length(5),  // Input/Status
            Constraint::Length(3),  // Menu
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Host Management")
        .style(Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick).style(Style::default().fg(Color::Black)));
    f.render_widget(title, chunks[0]);

    // Host list
    let hosts = app.coordinator.get_hosts();
    let items: Vec<ListItem> = hosts.iter()
        .enumerate()
        .map(|(i, host)| {
            let is_selected = i == app.selected_host_index;
            let status = if host.connected {
                "[CONNECTED]"
            } else {
                "[DISCONNECTED]"
            };
            let style = if is_selected {
                Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Black)
            };
            ListItem::new(Span::styled(
                format!("{} {} ({}) - {}", status, host.name, host.address, 
                    if host.connected { "Connected" } else { "Not Connected" }),
                style
            ))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Remote Hosts").style(Style::default().fg(Color::Black)))
        .style(Style::default());
    f.render_widget(list, chunks[1]);

    // Input field
    let input_text = if app.host_input.is_empty() {
        "Enter host address (IP:port or hostname:port)...".to_string()
    } else {
        app.host_input.clone()
    };
    let input_para = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Black))
        .block(Block::default().borders(Borders::ALL).title("Add Host").style(Style::default().fg(Color::Black)));
    f.render_widget(input_para, chunks[2]);

    // Menu
    let menu = Paragraph::new("[+] Add Host  |  [Enter] Add  |  [-] Remove  |  [T] Toggle Multi-Host  |  [Esc] Back")
        .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Black)))
        .style(Style::default().fg(Color::Black))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Handle keyboard input for host management
fn handle_host_management_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    let hosts = app.coordinator.get_hosts();
    let num_hosts = hosts.len();
    
    match key.code {
        KeyCode::Esc => {
            app.view_mode = ViewMode::ProcessList;
            app.host_input.clear();
        }
        KeyCode::Up => {
            if app.selected_host_index > 0 {
                app.selected_host_index -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_host_index + 1 < num_hosts {
                app.selected_host_index += 1;
            }
        }
        KeyCode::Enter => {
            // Add host
            if !app.host_input.trim().is_empty() {
                let address = app.host_input.trim().to_string();
                let name = address.clone();
                app.coordinator.add_host(address.clone(), name);
                app.host_input.clear();
                
                app.input_state.message = Some((
                    format!("Host added: {}. Connection will be tested on refresh.", address),
                    false
                ));
                app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
            }
        }
        KeyCode::Char(c) => {
            // If user is typing in input field, add character to input
            // Only process shortcuts if input is empty
            if !app.host_input.is_empty() {
                // User is typing - add all characters to input (including 't', 'T', '-')
                app.host_input.push(c);
            } else {
                // Input is empty - process shortcuts
                match c {
                    '-' => {
                        // Remove host
                        let host_address = hosts.get(app.selected_host_index).map(|h| h.address.clone());
                        if let Some(address) = host_address {
                            app.coordinator.remove_host(&address);
                            if app.selected_host_index >= app.coordinator.get_hosts().len() && app.selected_host_index > 0 {
                                app.selected_host_index -= 1;
                            }
                        }
                    }
                    't' | 'T' => {
                        // Toggle multi-host mode
                        app.multi_host_mode = !app.multi_host_mode;
                        app.view_mode = ViewMode::ProcessList;
                        app.input_state.message = Some((
                            format!("Multi-host mode: {}", if app.multi_host_mode { "ON" } else { "OFF" }),
                            false
                        ));
                        app.input_state.message_timeout = Some(std::time::Instant::now() + Duration::from_secs(2));
                    }
                    '+' => {
                        // Focus input field (clear and ready for input)
                        app.host_input.clear();
                    }
                    _ => {
                        // Start typing in input field
                        app.host_input.push(c);
                    }
                }
            }
        }
        KeyCode::Backspace => {
            app.host_input.pop();
        }
        _ => {}
    }
    Ok(false)
}

// Draw task editor view
fn draw_task_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let size = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(15), // Input fields
            Constraint::Min(5),     // Instructions
            Constraint::Length(3),   // Menu
        ])
        .split(size);

    // Title
    let title = Paragraph::new("Create Scheduled Task")
        .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Thick));
    f.render_widget(title, chunks[0]);

    // Input fields
    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Task name
            Constraint::Length(3),  // Schedule type
            Constraint::Length(3),  // Schedule value
            Constraint::Length(3),  // Action type
            Constraint::Length(3),  // Action value
        ])
        .split(chunks[1]);

    let fields = [
        ("Task Name", &app.input_state.task_name, 0),
        ("Schedule Type (cron/interval/once)", &app.input_state.task_schedule_type, 1),
        ("Schedule Value (e.g., '0 * * * *' or '60')", &app.input_state.task_schedule_value, 2),
        ("Action Type (restart/start/cleanup/rule)", &app.input_state.task_action_type, 3),
        ("Action Value (pattern/program/params/rule)", &app.input_state.task_action_value, 4),
    ];

    for (i, (label, value, field_idx)) in fields.iter().enumerate() {
        let is_active = app.input_state.current_task_field == *field_idx;
        let style = if is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black)
        };
        let cursor = if is_active { "_" } else { "" };
        let content = format!("{}: {}{}", label, value, cursor);
        let para = Paragraph::new(content)
            .style(style)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(para, field_chunks[i]);
    }

    // Instructions
    let instructions = vec![
        Line::from(vec![Span::styled("Instructions:", Style::default().fg(Color::Black).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::raw("1. Enter task name (e.g., 'Test Restart')")]),
        Line::from(vec![Span::raw("2. Schedule Type: 'cron' (e.g., '0 * * * *'), 'interval' (seconds), or 'once' (timestamp)")]),
        Line::from(vec![Span::raw("3. Schedule Value: cron expression, interval in seconds, or timestamp")]),
        Line::from(vec![Span::raw("4. Action Type: 'restart' (kill process), 'start' (start process), 'cleanup' (cleanup idle), or 'rule' (apply rule)")]),
        Line::from(vec![Span::raw("5. Action Value: pattern (restart), program name/path (start), cleanup params, or rule expression")]),
        Line::from(vec![Span::raw("6. Press [Tab] to switch fields, [Enter] to save task, [Esc] to cancel")]),
    ];
    let inst_para = Paragraph::new(instructions)
        .block(Block::default().borders(Borders::ALL).title("Instructions").style(Style::default().fg(Color::Black)));
    f.render_widget(inst_para, chunks[2]);

    // Menu
    let menu = Paragraph::new("[Tab] Next field  |  [Enter] Save  |  [Esc] Cancel")
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    f.render_widget(menu, chunks[3]);
}

// Handle keyboard input for task editor
fn handle_task_editor_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    match key.code {
        KeyCode::Tab => {
            app.input_state.current_task_field = (app.input_state.current_task_field + 1) % 5;
        }
        KeyCode::BackTab => {
            if app.input_state.current_task_field == 0 {
                app.input_state.current_task_field = 4;
            } else {
                app.input_state.current_task_field -= 1;
            }
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            match app.input_state.current_task_field {
                0 => app.input_state.task_name.push(c),
                1 => app.input_state.task_schedule_type.push(c),
                2 => app.input_state.task_schedule_value.push(c),
                3 => app.input_state.task_action_type.push(c),
                4 => app.input_state.task_action_value.push(c),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match app.input_state.current_task_field {
                0 => { app.input_state.task_name.pop(); }
                1 => { app.input_state.task_schedule_type.pop(); }
                2 => { app.input_state.task_schedule_value.pop(); }
                3 => { app.input_state.task_action_type.pop(); }
                4 => { app.input_state.task_action_value.pop(); }
                _ => {}
            }
        }
        KeyCode::Enter => {
            // Validate and create task
            if app.input_state.task_name.trim().is_empty() {
                app.input_state.message = Some(("Task name is required".to_string(), true));
                return Ok(false);
            }

            // Parse schedule
            let schedule = match app.input_state.task_schedule_type.trim().to_lowercase().as_str() {
                "cron" => {
                    if app.input_state.task_schedule_value.trim().is_empty() {
                        app.input_state.message = Some(("Cron expression is required".to_string(), true));
                        return Ok(false);
                    }
                    crate::scheduler::ScheduleType::Cron(app.input_state.task_schedule_value.trim().to_string())
                }
                "interval" => {
                    match app.input_state.task_schedule_value.trim().parse::<u64>() {
                        Ok(secs) => crate::scheduler::ScheduleType::Interval(secs),
                        Err(_) => {
                            app.input_state.message = Some(("Invalid interval value (must be a number)".to_string(), true));
                            return Ok(false);
                        }
                    }
                }
                "once" => {
                    match app.input_state.task_schedule_value.trim().parse::<u64>() {
                        Ok(timestamp) => {
                            use std::time::{UNIX_EPOCH, Duration};
                            crate::scheduler::ScheduleType::Once(UNIX_EPOCH + Duration::from_secs(timestamp))
                        }
                        Err(_) => {
                            app.input_state.message = Some(("Invalid timestamp value (must be a number)".to_string(), true));
                            return Ok(false);
                        }
                    }
                }
                _ => {
                    app.input_state.message = Some(("Invalid schedule type (must be 'cron', 'interval', or 'once')".to_string(), true));
                    return Ok(false);
                }
            };

            // Parse action
            let action = match app.input_state.task_action_type.trim().to_lowercase().as_str() {
                "restart" => {
                    if app.input_state.task_action_value.trim().is_empty() {
                        app.input_state.message = Some(("Process pattern is required for restart action".to_string(), true));
                        return Ok(false);
                    }
                    crate::scheduler::ScheduleAction::RestartProcess {
                        pattern: app.input_state.task_action_value.trim().to_string()
                    }
                }
                "start" => {
                    if app.input_state.task_action_value.trim().is_empty() {
                        app.input_state.message = Some(("Program name/path is required for start action".to_string(), true));
                        return Ok(false);
                    }
                    // Parse program and optional arguments (space-separated)
                    let parts: Vec<String> = app.input_state.task_action_value.trim().split_whitespace().map(|s| s.to_string()).collect();
                    let program = parts[0].clone();
                    let args = if parts.len() > 1 {
                        parts[1..].to_vec()
                    } else {
                        Vec::new()
                    };
                    crate::scheduler::ScheduleAction::StartProcess {
                        program,
                        args,
                    }
                }
                "cleanup" => {
                    // Parse cleanup params: "cpu_threshold,memory_threshold,duration,action"
                    let parts: Vec<&str> = app.input_state.task_action_value.split(',').map(|s| s.trim()).collect();
                    if parts.len() != 4 {
                        app.input_state.message = Some(("Cleanup requires: cpu_threshold,memory_threshold,duration_seconds,action".to_string(), true));
                        return Ok(false);
                    }
                    let cpu_threshold = parts[0].parse::<f32>().unwrap_or(0.0);
                    let memory_threshold = parts[1].parse::<u64>().unwrap_or(0);
                    let duration = parts[2].parse::<u64>().unwrap_or(0);
                    let action_str = parts[3].to_string();
                    crate::scheduler::ScheduleAction::CleanupIdle {
                        cpu_threshold,
                        memory_threshold,
                        duration_seconds: duration,
                        action: action_str,
                    }
                }
                "rule" => {
                    if app.input_state.task_action_value.trim().is_empty() {
                        app.input_state.message = Some(("Rule expression is required".to_string(), true));
                        return Ok(false);
                    }
                    crate::scheduler::ScheduleAction::ApplyRule {
                        rule: app.input_state.task_action_value.trim().to_string()
                    }
                }
                _ => {
                    app.input_state.message = Some(("Invalid action type (must be 'restart', 'start', 'cleanup', or 'rule')".to_string(), true));
                    return Ok(false);
                }
            };

            // Create and add task
            let task = crate::scheduler::ScheduledTask::new(
                app.input_state.task_name.trim().to_string(),
                schedule,
                action,
            );
            app.scheduler.add_task(task.clone());
            
            app.view_mode = ViewMode::Scheduler;
            app.input_state.message = Some((format!("Task '{}' created successfully", task.name), false));
            
            // Clear fields
            app.input_state.task_name.clear();
            app.input_state.task_schedule_type.clear();
            app.input_state.task_schedule_value.clear();
            app.input_state.task_action_type.clear();
            app.input_state.task_action_value.clear();
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Scheduler;
            // Clear fields
            app.input_state.task_name.clear();
            app.input_state.task_schedule_type.clear();
            app.input_state.task_schedule_value.clear();
            app.input_state.task_action_type.clear();
            app.input_state.task_action_value.clear();
        }
        _ => {}
    }
    Ok(false)
}

// Draw multi-host view (shows processes from all hosts)
fn draw_multi_host_view(f: &mut Frame, app: &mut App, area: Rect) {
    // Redirect to process list with multi-host mode enabled
    draw_process_list(f, app, area);
}

// Handle keyboard input for multi-host view
fn handle_multi_host_input(key: KeyEvent, app: &mut App) -> Result<bool, Box<dyn Error>> {
    // Redirect to process list handling
    handle_process_list_input(key, app)
}
