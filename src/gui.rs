//! Desktop GUI interface for Linux Process Manager

use crate::alert::AlertManager;
use crate::coordinator::Coordinator;
use crate::criu_manager::CriuManager;
use crate::graph::GraphData;
use crate::process::ProcessManager;
use crate::process_log::ProcessExitLogEntry;
use crate::profile::ProfileManager;
use crate::scheduler::{ScheduleAction, ScheduleType, ScheduledTask, Scheduler};
use crate::scripting_rules::RuleEngine;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct GuiApp {
    process_manager: Arc<Mutex<ProcessManager>>,
    graph_data: Arc<Mutex<GraphData>>,
    profile_manager: Arc<Mutex<ProfileManager>>,
    alert_manager: Arc<Mutex<AlertManager>>,
    coordinator: Arc<Mutex<Coordinator>>,
    criu_manager: Arc<Mutex<CriuManager>>,
    scheduler: Arc<Mutex<Scheduler>>,
    rule_engine: Arc<Mutex<RuleEngine>>,
    process_exit_log: Vec<ProcessExitLogEntry>,
    known_pids: HashMap<u32, String>, // To track process exits

    // UI State
    selected_tab: Tab,
    selected_process_index: usize,
    selected_process_pid: Option<u32>, // Track selected PID for filtering compatibility
    scroll_offset: f32,
    sort_column: Option<String>,
    sort_ascending: bool,
    filter_text: String,
    host_input: String, // For adding hosts
    multi_host_mode: bool,
    last_refresh: Instant,
    refresh_interval: f32, // seconds
    // Start Process dialog state
    show_start_process_dialog: bool,
    start_process_program: String,
    start_process_args: String,
    start_process_working_dir: String,

    // Nice Dialog
    show_nice_dialog: bool,
    nice_input: String,

    // Task Dialog
    show_task_dialog: bool,
    task_name_input: String,
    task_schedule_type_index: usize, // 0: Interval, 1: Cron, 2: OneShot
    task_interval_input: String,
    task_cron_input: String,
    task_oneshot_input: String,       // RFC3339 or similar
    task_action_index: usize,         // 0: Kill, 1: Stop, 2: LowerPriority, 3: ApplyRule
    task_action_target_input: String, // PID or Rule

    // Rule Dialog
    show_rule_dialog: bool,
    rule_input: String,

    // Confirmation/Warning Dialog
    show_confirmation_dialog: bool,
    confirmation_message: String,
    pending_action: Option<PendingAction>,
    show_kill_tree_option: bool, // New field for kill tree option

    // Profile Dialog
    show_profile_dialog: bool,
    profile_edit_mode: bool, // true for edit, false for create
    profile_edit_name: String,
    profile_name_input: String,
    profile_prioritize_input: String,
    profile_hide_input: String,
    profile_nice_pattern_input: String,
    profile_nice_value_input: String,

    // Alert Dialog
    show_alert_dialog: bool,
    alert_name_input: String,
    alert_condition_index: usize, // 0: CPU, 1: Memory, 2: ProcessDied
    alert_threshold_input: String,
    alert_duration_input: String,
    alert_target_index: usize, // 0: All, 1: Pattern
    alert_target_pattern_input: String,

    // Error feedback
    nice_error_message: Option<String>,
    last_error: Option<String>, // General error message for operations
}

#[derive(Clone)]
enum PendingAction {
    Kill(u32),
    KillTree(u32), // New variant
    Stop(u32),
    Terminate(u32),
    Continue(u32),
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    ProcessList,
    Statistics,
    Profiles,
    Alerts,
    Checkpoints,
    Hosts,
    PerProcessGraph,
    Logs,
    Schedule,
    Rules,
}

impl Default for GuiApp {
    fn default() -> Self {
        Self {
            process_manager: Arc::new(Mutex::new(ProcessManager::new())),
            graph_data: Arc::new(Mutex::new(GraphData::new(60, 500))),
            profile_manager: Arc::new(Mutex::new(ProfileManager::new())),
            alert_manager: Arc::new(Mutex::new(AlertManager::new())),
            coordinator: Arc::new(Mutex::new(Coordinator::new())),
            criu_manager: Arc::new(Mutex::new(CriuManager::new())),
            scheduler: Arc::new(Mutex::new(Scheduler::new())),
            rule_engine: Arc::new(Mutex::new(RuleEngine::new())),
            process_exit_log: Vec::new(),
            known_pids: HashMap::new(),
            selected_tab: Tab::ProcessList,
            selected_process_index: 0,
            selected_process_pid: None,
            scroll_offset: 0.0,
            sort_column: None,
            sort_ascending: true,
            filter_text: String::new(),
            host_input: String::new(),
            multi_host_mode: false,
            last_refresh: Instant::now(),
            refresh_interval: 1.0,
            show_start_process_dialog: false,
            start_process_program: String::new(),
            start_process_args: String::new(),
            start_process_working_dir: String::new(),

            show_nice_dialog: false,
            nice_input: String::new(),
            nice_error_message: None,
            last_error: None,

            show_task_dialog: false,
            task_name_input: String::new(),
            task_schedule_type_index: 0,
            task_interval_input: String::new(),
            task_cron_input: String::new(),
            task_oneshot_input: String::new(),
            task_action_index: 0,
            task_action_target_input: String::new(),

            show_rule_dialog: false,
            rule_input: String::new(),

            show_confirmation_dialog: false,
            confirmation_message: String::new(),
            pending_action: None,
            show_kill_tree_option: false, // Initialize new field

            show_profile_dialog: false,
            profile_edit_mode: false,
            profile_edit_name: String::new(),
            profile_name_input: String::new(),
            profile_prioritize_input: String::new(),
            profile_hide_input: String::new(),
            profile_nice_pattern_input: String::new(),
            profile_nice_value_input: String::new(),

            show_alert_dialog: false,
            alert_name_input: String::new(),
            alert_condition_index: 0,
            alert_threshold_input: String::new(),
            alert_duration_input: String::new(),
            alert_target_index: 0,
            alert_target_pattern_input: String::new(),
        }
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-refresh
        if self.last_refresh.elapsed().as_secs_f32() >= self.refresh_interval {
            self.refresh();
            self.last_refresh = Instant::now();
        }

        // Request repaint for smooth updates
        ctx.request_repaint();

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Exit").clicked() {
                        std::process::exit(0);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui
                        .selectable_label(self.selected_tab == Tab::ProcessList, "Process List")
                        .clicked()
                    {
                        self.selected_tab = Tab::ProcessList;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Statistics, "Statistics")
                        .clicked()
                    {
                        self.selected_tab = Tab::Statistics;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Profiles, "Profiles")
                        .clicked()
                    {
                        self.selected_tab = Tab::Profiles;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Alerts, "Alerts")
                        .clicked()
                    {
                        self.selected_tab = Tab::Alerts;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Checkpoints, "Checkpoints")
                        .clicked()
                    {
                        self.selected_tab = Tab::Checkpoints;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Hosts, "Hosts")
                        .clicked()
                    {
                        self.selected_tab = Tab::Hosts;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::PerProcessGraph, "Graph")
                        .clicked()
                    {
                        self.selected_tab = Tab::PerProcessGraph;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Logs, "Logs")
                        .clicked()
                    {
                        self.selected_tab = Tab::Logs;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Schedule, "Schedule")
                        .clicked()
                    {
                        self.selected_tab = Tab::Schedule;
                    }
                    if ui
                        .selectable_label(self.selected_tab == Tab::Rules, "Rules")
                        .clicked()
                    {
                        self.selected_tab = Tab::Rules;
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        // Show about dialog
                    }
                });
            });
        });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| match self.selected_tab {
            Tab::ProcessList => self.draw_process_list(ui, ctx),
            Tab::Statistics => self.draw_statistics(ui),
            Tab::Profiles => self.draw_profiles(ui),
            Tab::Alerts => self.draw_alerts(ui),
            Tab::Checkpoints => self.draw_checkpoints(ui),
            Tab::Hosts => self.draw_hosts(ui),
            Tab::PerProcessGraph => self.draw_per_process_graph(ui),
            Tab::Logs => self.draw_logs(ui),
            Tab::Schedule => self.draw_schedule(ui),
            Tab::Rules => self.draw_rules(ui),
        });

        // Render Dialogs
        self.draw_nice_dialog(ctx);
        self.draw_task_dialog(ctx);
        self.draw_rule_dialog(ctx);
        self.draw_confirmation_dialog(ctx);
        self.draw_profile_dialog(ctx);
        self.draw_alert_dialog(ctx);
    }
}

impl GuiApp {
    fn refresh(&mut self) {
        if let Ok(mut pm) = self.process_manager.lock() {
            pm.refresh();
        }
        if let Ok(pm) = self.process_manager.lock() {
            if let Ok(mut gd) = self.graph_data.lock() {
                gd.update(&pm);
            }
        }

        // Multi-host fetch (if enabled)
        if self.multi_host_mode {
            let coordinator = self.coordinator.clone();

            // Get hosts to fetch from (brief lock)
            let hosts_to_fetch: Vec<(String, String)> = if let Ok(coord) = coordinator.lock() {
                coord
                    .get_hosts()
                    .iter()
                    .map(|h| (h.address.clone(), h.name.clone()))
                    .collect()
            } else {
                Vec::new()
            };

            if !hosts_to_fetch.is_empty() {
                // Spawn async task to fetch data
                tokio::spawn(async move {
                    for (address, name) in hosts_to_fetch {
                        match crate::coordinator::fetch_host_data(address.clone(), name).await {
                            Ok(processes) => {
                                if let Ok(mut coord) = coordinator.lock() {
                                    coord.update_host_data(&address, processes);
                                }
                            }
                            Err(_) => {
                                if let Ok(mut coord) = coordinator.lock() {
                                    coord.mark_host_disconnected(&address);
                                }
                            }
                        }
                    }
                });
            }
        }

        // Check alerts
        if let Ok(pm) = self.process_manager.lock() {
            if let Ok(mut am) = self.alert_manager.lock() {
                let processes = pm.get_processes().clone();
                // Use known_pids which maps PID -> Name from previous refresh
                am.check_alerts(&processes, &self.known_pids);
            }
        }

        // Apply profile rules (prioritization and nice adjustments)
        if let Ok(pm) = self.profile_manager.lock() {
            if let Ok(process_manager) = self.process_manager.lock() {
                // Apply nice adjustments
                let mut adjustments = Vec::new();
                for process in process_manager.get_processes() {
                    if let Some(target_nice) = pm.get_nice_adjustment(&process.name) {
                        if process.nice != target_nice {
                            adjustments.push((process.pid, target_nice));
                        }
                    }
                }

                // Apply adjustments
                for (pid, nice) in adjustments {
                    let _ = process_manager.set_niceness(pid, nice);
                }
            }
        }

        // Update process log
        self.update_process_log();
    }

    fn update_process_log(&mut self) {
        if let Ok(pm) = self.process_manager.lock() {
            let current_processes = pm.get_processes();
            let current_pids_map: HashMap<u32, String> = current_processes
                .iter()
                .map(|p| (p.pid, p.name.clone()))
                .collect();

            // Check for exited processes
            let known_pids_set: HashSet<u32> = self.known_pids.keys().cloned().collect();
            let current_pids_set: HashSet<u32> = current_pids_map.keys().cloned().collect();

            let exited_pids: Vec<u32> = known_pids_set
                .difference(&current_pids_set)
                .cloned()
                .collect();

            for pid in exited_pids {
                if let Some(name) = self.known_pids.get(&pid) {
                    self.process_exit_log.push(ProcessExitLogEntry {
                        pid,
                        name: name.clone(),
                        user: None,
                        start_time: "Unknown".to_string(),
                        exit_time: chrono::Local::now(),
                        uptime_secs: 0,
                    });
                }
            }

            // Update known pids
            self.known_pids = current_pids_map;
        }
    }

    fn draw_process_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Process List");

        // Filter and controls
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.filter_text);
            ui.checkbox(&mut self.multi_host_mode, "Multi-Host Mode");
            if ui.button("New Process").clicked() {
                self.show_start_process_dialog = true;
            }
            if ui.button("Refresh").clicked() {
                self.refresh();
            }

            ui.separator();

            // Sort controls
            ui.label("Sort by:");
            let current_sort = self
                .sort_column
                .clone()
                .unwrap_or_else(|| "pid".to_string());
            egui::ComboBox::from_id_source("sort_column")
                .selected_text(&current_sort)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current_sort == "pid", "PID").clicked() {
                        self.sort_by("pid");
                    }
                    if ui
                        .selectable_label(current_sort == "name", "Name")
                        .clicked()
                    {
                        self.sort_by("name");
                    }
                    if ui.selectable_label(current_sort == "cpu", "CPU").clicked() {
                        self.sort_by("cpu");
                    }
                    if ui
                        .selectable_label(current_sort == "memory", "Memory")
                        .clicked()
                    {
                        self.sort_by("mem");
                    }
                });

            let sort_icon = if self.sort_ascending { "⬆" } else { "⬇" };
            if ui.button(sort_icon).clicked() {
                self.sort_ascending = !self.sort_ascending;
                if let Some(col) = &self.sort_column {
                    if let Ok(mut pm) = self.process_manager.lock() {
                        pm.set_sort(col, self.sort_ascending);
                        pm.refresh();
                    }
                }
            }
        });

        ui.separator();

        // Error Banner
        let error_msg = self.last_error.clone();
        if let Some(error) = error_msg {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                if ui.button("Dismiss").clicked() {
                    self.last_error = None;
                }
            });
            ui.separator();
        }

        // Process table - get processes
        let mut processes = if let Ok(pm) = self.process_manager.lock() {
            pm.get_processes().clone()
        } else {
            Vec::new()
        };

        // Add remote processes if in multi-host mode
        if self.multi_host_mode {
            if let Ok(coord) = self.coordinator.lock() {
                let remote_procs = coord.get_remote_processes();
                for rp in remote_procs {
                    processes.push(crate::process::ProcessInfo::from(rp));
                }
            }
        }

        // Build filtered list for display
        let mut filtered_processes: Vec<_> = processes
            .iter()
            .filter(|p| {
                // Check if process should be hidden by profile
                if let Ok(pm) = self.profile_manager.lock() {
                    if pm.should_hide_process(&p.name) {
                        return false;
                    }
                }

                if !self.filter_text.is_empty() {
                    p.name
                        .to_lowercase()
                        .contains(&self.filter_text.to_lowercase())
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        // Sort processes: Prioritized first, then by selected column
        if let Ok(pm) = self.profile_manager.lock() {
            filtered_processes.sort_by(|a, b| {
                let a_prio = pm.is_process_prioritized(&a.name);
                let b_prio = pm.is_process_prioritized(&b.name);

                if a_prio != b_prio {
                    return b_prio.cmp(&a_prio); // True (prioritized) comes first
                }

                // Secondary sort by column if selected
                if let Some(col) = &self.sort_column {
                    let order = match col.as_str() {
                        "pid" => a.pid.cmp(&b.pid),
                        "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        "cpu" => a
                            .cpu_usage
                            .partial_cmp(&b.cpu_usage)
                            .unwrap_or(std::cmp::Ordering::Equal),
                        "mem" => a.memory_usage.cmp(&b.memory_usage),
                        "ppid" => a.parent_pid.cmp(&b.parent_pid),
                        "user" => a.user.cmp(&b.user),
                        "nice" => a.nice.cmp(&b.nice),
                        "status" => a.status.cmp(&b.status),
                        _ => std::cmp::Ordering::Equal,
                    };

                    if self.sort_ascending {
                        order
                    } else {
                        order.reverse()
                    }
                } else {
                    // Default sort by PID
                    a.pid.cmp(&b.pid)
                }
            });
        }

        // If selected process is filtered out, clear selection or select first available
        if let Some(selected_pid) = self.selected_process_pid {
            if !filtered_processes.iter().any(|p| p.pid == selected_pid) {
                // Selected process is not in filtered list, select first available
                if !filtered_processes.is_empty() {
                    self.selected_process_pid = Some(filtered_processes[0].pid);
                    self.selected_process_index = 0;
                } else {
                    self.selected_process_pid = None;
                    self.selected_process_index = 0;
                }
            }
        } else {
            // Auto-select first process if none selected and list is not empty
            if !filtered_processes.is_empty() {
                self.selected_process_pid = Some(filtered_processes[0].pid);
                self.selected_process_index = 0;
            }
        }

        // Find selected process by PID (works with filtering) - do this before layout
        let selected_process = if let Some(pid) = self.selected_process_pid {
            processes.iter().find(|p| p.pid == pid).cloned()
        } else if !filtered_processes.is_empty()
            && self.selected_process_index < filtered_processes.len()
        {
            Some(filtered_processes[self.selected_process_index].clone())
        } else {
            None
        };

        // Use Grid with fixed column widths for proper alignment
        let available_height = ui.available_height();
        let button_area_height = 100.0; // Reserve space for buttons
        let scroll_height = (available_height - button_area_height).max(200.0);

        // Capture sort state before closures
        let current_sort_column = self.sort_column.clone();
        let current_sort_ascending = self.sort_ascending;
        let multi_host_mode = self.multi_host_mode;

        // Use RefCell to allow mutation inside closures
        use std::cell::RefCell;
        let clicked_column: RefCell<Option<String>> = RefCell::new(None);
        let clicked_column_ref = &clicked_column;

        // Helper function to create sortable header (without borrowing self)
        let make_header = |ui: &mut egui::Ui, label: &str, column: &str, width: f32| {
            let sort_indicator = if current_sort_column.as_ref() == Some(&column.to_string()) {
                if current_sort_ascending {
                    " ↑"
                } else {
                    " ↓"
                }
            } else {
                ""
            };
            let header_text = format!("{}{}", label, sort_indicator);
            let response = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                    ui.set_width(width);
                    ui.selectable_label(false, &header_text)
                })
                .response;
            if response.clicked() {
                *clicked_column_ref.borrow_mut() = Some(column.to_string());
            }
        };

        // Header row with fixed widths
        egui::Grid::new("process_table_header")
            .num_columns(9 + if multi_host_mode { 1 } else { 0 })
            .spacing([2.0, 4.0])
            .min_col_width(60.0)
            .show(ui, |ui| {
                make_header(ui, "PID", "pid", 80.0);
                make_header(ui, "Name", "name", 200.0);
                if multi_host_mode {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                        ui.set_width(120.0);
                        ui.strong("Host");
                    });
                }
                make_header(ui, "CPU %", "cpu", 80.0);
                make_header(ui, "Memory", "mem", 100.0);
                make_header(ui, "PPID", "ppid", 80.0);
                make_header(ui, "User", "user", 100.0);
                make_header(ui, "Nice", "nice", 60.0);
                make_header(ui, "Status", "status", 80.0);
                ui.end_row();
            });

        // Handle sorting after the closure - this will trigger a repaint
        if let Some(column) = clicked_column.into_inner() {
            self.sort_by(&column);
            ctx.request_repaint(); // Request repaint to show sorted results
        }

        egui::ScrollArea::vertical()
            .max_height(scroll_height)
            .show(ui, |ui| {
                egui::Grid::new("process_table")
                    .num_columns(9 + if self.multi_host_mode { 1 } else { 0 })
                    .spacing([2.0, 2.0])
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        for (i, process) in filtered_processes.iter().enumerate() {
                            let is_selected = self.selected_process_pid == Some(process.pid);

                            // PID
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(80.0);
                                if ui
                                    .selectable_label(is_selected, process.pid.to_string())
                                    .clicked()
                                {
                                    self.selected_process_index = i;
                                    self.selected_process_pid = Some(process.pid);
                                }
                            });

                            // Name
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(200.0);
                                ui.label(&process.name);
                            });

                            // Host (if multi-host mode)
                            if self.multi_host_mode {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::LEFT),
                                    |ui| {
                                        ui.set_width(120.0);
                                        let host = process
                                            .host
                                            .as_ref()
                                            .map(|h| h.as_str())
                                            .unwrap_or("local");
                                        ui.label(host);
                                    },
                                );
                            }

                            // CPU %
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(80.0);
                                let cpu_color = if process.cpu_usage > 50.0 {
                                    egui::Color32::RED
                                } else if process.cpu_usage > 25.0 {
                                    egui::Color32::YELLOW
                                } else {
                                    egui::Color32::GREEN
                                };
                                ui.colored_label(cpu_color, format!("{:.2}%", process.cpu_usage));
                            });

                            // Memory
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(100.0);
                                ui.label(format!("{}", process.memory_usage / (1024 * 1024)));
                            });

                            // PPID
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(80.0);
                                ui.label(
                                    process
                                        .parent_pid
                                        .map(|p| p.to_string())
                                        .unwrap_or_default(),
                                );
                            });

                            // User
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(100.0);
                                ui.label(process.user.as_ref().map(|u| u.as_str()).unwrap_or(""));
                            });

                            // Nice
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(60.0);
                                ui.label(process.nice.to_string());
                            });

                            // Status
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
                                ui.set_width(80.0);
                                ui.label(&process.status);
                            });

                            ui.end_row();
                        }
                    });
            });

        // Process actions - always visible at bottom
        ui.separator();
        ui.add_space(5.0);

        if let Some(process) = selected_process {
            ui.horizontal(|ui| {
                ui.label(format!("Selected: {} (PID: {})", process.name, process.pid));
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if ui.button("Kill").clicked() {
                    // Check for children
                    let has_children = if let Ok(pm) = self.process_manager.lock() {
                        pm.get_processes().iter().any(|p| p.parent_pid == Some(process.pid))
                    } else {
                        false
                    };

                    if has_children {
                        self.confirmation_message = format!("Process {} (PID: {}) has child processes. Killing it might orphan them. Are you sure?", process.name, process.pid);
                        self.pending_action = Some(PendingAction::Kill(process.pid));
                        self.show_kill_tree_option = true; // Enable kill tree option
                        self.show_confirmation_dialog = true;
                    } else {
                        self.confirmation_message = format!("Are you sure you want to kill process {} (PID: {})?", process.name, process.pid);
                        self.pending_action = Some(PendingAction::Kill(process.pid));
                        self.show_kill_tree_option = false;
                        self.show_confirmation_dialog = true;
                    }
                }
                if ui.button("Stop").clicked() {
                    if let Ok(pm) = self.process_manager.lock() {
                        let _ = pm.stop_process(process.pid);
                    }
                    self.refresh();
                }
                if ui.button("Terminate").clicked() {
                    if let Ok(pm) = self.process_manager.lock() {
                        let _ = pm.terminate_process(process.pid);
                    }
                    self.refresh();
                }
                if ui.button("Continue").clicked() {
                    if let Ok(pm) = self.process_manager.lock() {
                        let _ = pm.continue_process(process.pid);
                    }
                    self.refresh();
                }
                if ui.button("Change Nice").clicked() {
                    self.show_nice_dialog = true;
                    self.nice_input = process.nice.to_string();
                }
                if ui.button("Graph").clicked() {
                    self.selected_tab = Tab::PerProcessGraph;
                }
            });
        } else {
            ui.label("No process selected. Click on a process to select it.");
        }

        // Start Process Dialog
        if self.show_start_process_dialog {
            egui::Window::new("Start New Process")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Program:");
                    ui.text_edit_singleline(&mut self.start_process_program);
                    ui.add_space(5.0);

                    ui.label("Arguments (space-separated):");
                    ui.text_edit_singleline(&mut self.start_process_args);
                    ui.add_space(5.0);

                    ui.label("Working Directory (optional):");
                    ui.text_edit_singleline(&mut self.start_process_working_dir);
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("Start").clicked() {
                            if !self.start_process_program.trim().is_empty() {
                                let program = self.start_process_program.clone();
                                let args_str = self.start_process_args.clone();
                                let working_dir_str = self.start_process_working_dir.clone();

                                // Execute start_process and drop lock before refresh
                                let result = if let Ok(mut pm_guard) = self.process_manager.lock() {
                                    let args: Vec<&str> = if !args_str.trim().is_empty() {
                                        args_str.split_whitespace().collect()
                                    } else {
                                        Vec::new()
                                    };
                                    let working_dir = if !working_dir_str.trim().is_empty() {
                                        Some(working_dir_str.trim())
                                    } else {
                                        None
                                    };

                                    // Call start_process and return result (lock will be dropped after this block)
                                    pm_guard.start_process(&program, &args, working_dir, &[])
                                } else {
                                    eprintln!("Failed to lock process manager");
                                    std::io::Result::Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "Failed to lock process manager",
                                    ))
                                };

                                match result {
                                    Ok(_pid) => {
                                        // Success - clear dialog
                                        self.show_start_process_dialog = false;
                                        self.start_process_program.clear();
                                        self.start_process_args.clear();
                                        self.start_process_working_dir.clear();
                                        self.refresh();
                                    }
                                    Err(e) => {
                                        // Show error (could use a message system)
                                        eprintln!("Failed to start process: {}", e);
                                    }
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_start_process_dialog = false;
                            self.start_process_program.clear();
                            self.start_process_args.clear();
                            self.start_process_working_dir.clear();
                        }
                    });
                });
        }
    }

    fn draw_statistics(&mut self, ui: &mut egui::Ui) {
        ui.heading("System Statistics");

        if let Ok(gd) = self.graph_data.lock() {
            // CPU Graph
            ui.label("CPU Usage");
            let cpu_history = gd.get_cpu_history();
            if !cpu_history.is_empty() {
                egui_plot::Plot::new("cpu_plot")
                    .height(200.0)
                    .show(ui, |plot_ui| {
                        let points: Vec<[f64; 2]> = cpu_history
                            .iter()
                            .enumerate()
                            .map(|(i, &val)| [i as f64, val as f64])
                            .collect();
                        plot_ui.line(egui_plot::Line::new(points));
                    });
            }

            ui.separator();

            // Memory Graph
            ui.label("Memory Usage");
            let mem_history = gd.get_memory_history();
            if !mem_history.is_empty() {
                egui_plot::Plot::new("mem_plot")
                    .height(200.0)
                    .show(ui, |plot_ui| {
                        let points: Vec<[f64; 2]> = mem_history
                            .iter()
                            .enumerate()
                            .map(|(i, val)| [i as f64, *val as f64])
                            .collect();
                        plot_ui.line(egui_plot::Line::new(points));
                    });
            }
        }
    }

    fn draw_profiles(&mut self, ui: &mut egui::Ui) {
        ui.heading("Focus Mode Profiles");

        // Clone profile data to avoid deadlock
        let (profiles, active_profile_name) = if let Ok(pm) = self.profile_manager.lock() {
            (
                pm.get_profiles().to_vec(),
                pm.get_active_profile().map(|s| s.to_string()),
            )
        } else {
            (Vec::new(), None)
        };

        // Track which profile to delete or toggle
        let mut profile_to_delete: Option<String> = None;
        let mut profile_to_toggle: Option<(String, bool)> = None;

        for profile in &profiles {
            ui.horizontal(|ui| {
                let is_active =
                    active_profile_name.as_ref().map(|s| s.as_str()) == Some(profile.name.as_str());
                if ui.selectable_label(is_active, &profile.name).clicked() {
                    profile_to_toggle = Some((profile.name.clone(), is_active));
                }
                if ui.button("Edit").clicked() {
                    // Open edit dialog
                    self.profile_edit_mode = true;
                    self.profile_edit_name = profile.name.clone();
                    self.profile_name_input = profile.name.clone();
                    self.profile_prioritize_input = profile.prioritize_processes.join(", ");
                    self.profile_hide_input = profile.hide_processes.join(", ");
                    // For nice adjustments, show as "pattern:value, pattern:value"
                    self.profile_nice_pattern_input = String::new();
                    self.profile_nice_value_input = String::new();
                    self.show_profile_dialog = true;
                }
                if ui.button("Delete").clicked() {
                    profile_to_delete = Some(profile.name.clone());
                }
            });
        }

        // Handle delete outside the iteration
        if let Some(name) = profile_to_delete {
            if let Ok(mut pm) = self.profile_manager.lock() {
                pm.remove_profile(&name);
            }
        }

        // Handle toggle outside the iteration
        if let Some((name, is_active)) = profile_to_toggle {
            if let Ok(mut pm) = self.profile_manager.lock() {
                if is_active {
                    pm.set_active_profile(None);
                } else {
                    pm.set_active_profile(Some(name));
                }
            }
        }

        if ui.button("Create New Profile").clicked() {
            // Open create dialog
            self.profile_edit_mode = false;
            self.profile_edit_name = String::new();
            self.profile_name_input = String::new();
            self.profile_prioritize_input = String::new();
            self.profile_hide_input = String::new();
            self.profile_nice_pattern_input = String::new();
            self.profile_nice_value_input = String::new();
            self.show_profile_dialog = true;
        }
    }

    fn draw_alerts(&mut self, ui: &mut egui::Ui) {
        ui.heading("Alerts and Notifications");

        // Clone alert data to avoid deadlock
        let (alerts, active_alerts_data) = if let Ok(am) = self.alert_manager.lock() {
            (am.get_alerts().to_vec(), am.get_active_alerts().to_vec())
        } else {
            (Vec::new(), Vec::new())
        };

        ui.label(format!("Active Alerts: {}", active_alerts_data.len()));

        ui.separator();

        // Track which alert to delete or toggle
        let mut alert_to_delete: Option<usize> = None;
        let mut alert_to_toggle: Option<usize> = None;

        ui.label("Alert Rules:");
        for (idx, alert) in alerts.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut enabled = alert.enabled;
                if ui.checkbox(&mut enabled, &alert.name).changed() {
                    alert_to_toggle = Some(idx);
                }

                // Format condition for display
                let condition_str = match &alert.condition {
                    crate::alert::AlertCondition::CpuGreaterThan {
                        threshold,
                        duration_secs,
                    } => {
                        format!("CPU > {}% for {}s", threshold, duration_secs)
                    }
                    crate::alert::AlertCondition::MemoryGreaterThan {
                        threshold_mb,
                        duration_secs,
                    } => {
                        format!("Memory > {}MB for {}s", threshold_mb, duration_secs)
                    }
                    crate::alert::AlertCondition::IoGreaterThan {
                        threshold_mb_per_sec,
                        duration_secs,
                    } => {
                        format!("I/O > {}MB/s for {}s", threshold_mb_per_sec, duration_secs)
                    }
                    crate::alert::AlertCondition::ProcessDied { pattern } => {
                        format!("Process died: {}", pattern)
                    }
                };
                ui.label(condition_str);

                if ui.button("Delete").clicked() {
                    alert_to_delete = Some(idx);
                }
            });
        }

        // Handle toggle outside the iteration
        if let Some(idx) = alert_to_toggle {
            if let Ok(mut am) = self.alert_manager.lock() {
                am.toggle_alert(idx);
            }
        }

        // Handle delete outside the iteration
        if let Some(idx) = alert_to_delete {
            if let Ok(mut am) = self.alert_manager.lock() {
                am.remove_alert(idx);
            }
        }

        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Create CPU Alert").clicked() {
                self.alert_name_input = format!("High CPU Alert {}", alerts.len() + 1);
                self.alert_condition_index = 0;
                self.alert_threshold_input = "80.0".to_string();
                self.alert_duration_input = "5".to_string();
                self.alert_target_index = 0;
                self.alert_target_pattern_input = String::new();
                self.show_alert_dialog = true;
            }
            if ui.button("Create Memory Alert").clicked() {
                self.alert_name_input = format!("High Memory Alert {}", alerts.len() + 1);
                self.alert_condition_index = 1;
                self.alert_threshold_input = "1024".to_string();
                self.alert_duration_input = "5".to_string();
                self.alert_target_index = 0;
                self.alert_target_pattern_input = String::new();
                self.show_alert_dialog = true;
            }
            if ui.button("Create Process Death Alert").clicked() {
                self.alert_name_input = format!("Process Death Alert {}", alerts.len() + 1);
                self.alert_condition_index = 2;
                self.alert_threshold_input = String::new();
                self.alert_duration_input = String::new();
                self.alert_target_index = 1;
                self.alert_target_pattern_input = "firefox".to_string();
                self.show_alert_dialog = true;
            }
        });

        ui.separator();

        // Active Alerts Section
        ui.heading("Active Alerts");
        if active_alerts_data.is_empty() {
            ui.label("No active alerts");
        } else {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for alert in active_alerts_data.iter().rev().take(20) {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::RED, "⚠");
                            ui.label(format!("{}: {}", alert.alert_name, alert.message));
                        });
                    }
                });

            if ui.button("Clear All Active Alerts").clicked() {
                if let Ok(mut am) = self.alert_manager.lock() {
                    am.clear_all_active_alerts();
                }
            }
        }
    }

    fn draw_checkpoints(&mut self, ui: &mut egui::Ui) {
        ui.heading("CRIU Checkpoints");

        if let Ok(cm) = self.criu_manager.lock() {
            let available = cm.is_available();

            if !available {
                ui.label("CRIU is not available on this system.");
                return;
            }

            let checkpoints = cm.list_checkpoints();

            for checkpoint in checkpoints {
                ui.horizontal(|ui| {
                    ui.label(&checkpoint.checkpoint_id);
                    ui.label(format!("PID: {}", checkpoint.pid));
                    ui.label(&checkpoint.process_name);
                    if ui.button("Restore").clicked() {
                        if let Ok(cm) = self.criu_manager.lock() {
                            match cm.restore_process(&checkpoint.checkpoint_id) {
                                Ok(_pid) => {
                                    // Show success message
                                }
                                Err(_e) => {
                                    // Show error message
                                }
                            }
                        }
                    }
                    if ui.button("Delete").clicked() {
                        if let Ok(cm) = self.criu_manager.lock() {
                            let _ = cm.delete_checkpoint(&checkpoint.checkpoint_id);
                        }
                    }
                });
            }

            if ui.button("Create Checkpoint").clicked() {
                // Create checkpoint for selected process
            }
        }
    }

    fn draw_hosts(&mut self, ui: &mut egui::Ui) {
        ui.heading("Multi-Host Management");

        // Get hosts list (drop lock before button handlers)
        let hosts: Vec<_> = if let Ok(coord) = self.coordinator.lock() {
            coord.get_hosts().to_vec()
        } else {
            Vec::new()
        };

        // Display hosts
        for host in &hosts {
            ui.horizontal(|ui| {
                let status_color = if host.connected {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::RED
                };
                ui.colored_label(status_color, &host.name);
                ui.label(&host.address);
                let address_to_remove = host.address.clone();
                if ui.button("Remove").clicked() {
                    if let Ok(mut coord) = self.coordinator.lock() {
                        coord.remove_host(&address_to_remove);
                    }
                }
            });
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Add Host:");
            ui.text_edit_singleline(&mut self.host_input);
            if ui.button("Add").clicked() && !self.host_input.trim().is_empty() {
                let address = self.host_input.trim().to_string();
                if let Ok(mut coord) = self.coordinator.lock() {
                    coord.add_host(address.clone(), address.clone());
                    self.host_input.clear();
                } else {
                    eprintln!("Failed to lock coordinator");
                }
            }
        });

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.multi_host_mode, "Enable Multi-Host Mode");
            if ui.button("Refresh All").clicked() {
                // Refresh processes from all hosts (would need async implementation)
            }
        });
    }

    fn sort_by(&mut self, column: &str) {
        if self.sort_column.as_ref() == Some(&column.to_string()) {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = Some(column.to_string());
            self.sort_ascending = true;
        }

        if let Ok(mut pm) = self.process_manager.lock() {
            pm.set_sort(column, self.sort_ascending);
            // Refresh to apply the sort
            pm.refresh();
        }
    }

    fn draw_per_process_graph(&mut self, ui: &mut egui::Ui) {
        ui.heading("Per-Process Graph");

        // Process selector
        ui.horizontal(|ui| {
            ui.label("Select Process:");
            if let Ok(pm) = self.process_manager.lock() {
                let processes = pm.get_processes();
                let current_selection = if let Some(pid) = self.selected_process_pid {
                    processes
                        .iter()
                        .find(|p| p.pid == pid)
                        .map(|p| format!("{} ({})", p.name, p.pid))
                        .unwrap_or_else(|| "Select Process".to_string())
                } else {
                    "Select Process".to_string()
                };

                egui::ComboBox::from_id_source("process_selector")
                    .selected_text(current_selection)
                    .show_ui(ui, |ui| {
                        for process in processes {
                            let label = format!("{} ({})", process.name, process.pid);
                            if ui
                                .selectable_value(
                                    &mut self.selected_process_pid,
                                    Some(process.pid),
                                    label,
                                )
                                .clicked()
                            {
                                // Selection changed
                            }
                        }
                    });
            }
        });

        ui.separator();

        if let Some(pid) = self.selected_process_pid {
            if let Ok(gd) = self.graph_data.lock() {
                if let Some((cpu_history, mem_history)) = gd.get_process_history(pid) {
                    // CPU Graph
                    ui.label("CPU Usage");
                    egui_plot::Plot::new("proc_cpu_plot")
                        .height(200.0)
                        .show(ui, |plot_ui| {
                            let points: Vec<[f64; 2]> = cpu_history
                                .iter()
                                .enumerate()
                                .map(|(i, &val)| [i as f64, val as f64])
                                .collect();
                            plot_ui.line(egui_plot::Line::new(points));
                        });

                    ui.add_space(10.0);

                    // Memory Graph
                    ui.label("Memory Usage (MB)");
                    egui_plot::Plot::new("proc_mem_plot")
                        .height(200.0)
                        .show(ui, |plot_ui| {
                            let points: Vec<[f64; 2]> = mem_history
                                .iter()
                                .enumerate()
                                .map(|(i, &val)| [i as f64, val as f64 / (1024.0 * 1024.0)])
                                .collect();
                            plot_ui.line(egui_plot::Line::new(points));
                        });
                } else {
                    ui.label("No history data available for this process.");
                }
            }
        } else {
            ui.label("Please select a process to view its history.");
        }
    }

    fn draw_logs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Process Exit Logs");

        use egui_extras::{Column, TableBuilder};

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(80.0).resizable(true)) // PID
            .column(Column::initial(200.0).resizable(true)) // Name
            .column(Column::initial(150.0).resizable(true)) // Exit Time
            .column(Column::remainder())
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("PID");
                });
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Exit Time");
                });
            })
            .body(|mut body| {
                for entry in self.process_exit_log.iter().rev() {
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label(entry.pid.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&entry.name);
                        });
                        row.col(|ui| {
                            ui.label(entry.exit_time.format("%Y-%m-%d %H:%M:%S").to_string());
                        });
                    });
                }
            });

        if self.process_exit_log.is_empty() {
            ui.label("No process exits recorded yet.");
        }
    }

    fn draw_schedule(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scheduled Tasks");

        if let Ok(mut scheduler) = self.scheduler.lock() {
            // Check for due tasks
            let due_indices = scheduler.check_due_tasks();

            // Collect tasks to execute to avoid borrowing issues
            let mut tasks_to_execute = Vec::new();
            for idx in &due_indices {
                if let Some(task) = scheduler.get_tasks().get(*idx) {
                    tasks_to_execute.push((task.name.clone(), task.action.clone()));
                }
            }

            // Execute due tasks

            for (name, action) in tasks_to_execute {
                println!("Executing task: {}", name);

                let result = if let Ok(pm) = self.process_manager.lock() {
                    match action {
                        ScheduleAction::KillProcess { pid } => match pm.kill_process(pid) {
                            Ok(_) => "Success".to_string(),
                            Err(e) => format!("Failed: {}", e),
                        },
                        ScheduleAction::StopProcess { pid } => match pm.stop_process(pid) {
                            Ok(_) => "Success".to_string(),
                            Err(e) => format!("Failed: {}", e),
                        },
                        ScheduleAction::ReniceProcess { pid, nice } => {
                            match pm.set_niceness(pid, nice) {
                                Ok(_) => "Success".to_string(),
                                Err(e) => format!("Failed: {}", e),
                            }
                        }
                        ScheduleAction::ApplyRule { rule } => {
                            if let Ok(mut re) = self.rule_engine.lock() {
                                re.set_rule(rule);
                                "Rule Applied".to_string()
                            } else {
                                "Failed to lock RuleEngine".to_string()
                            }
                        }
                        _ => "Not implemented".to_string(),
                    }
                } else {
                    "Failed to lock ProcessManager".to_string()
                };

                scheduler.add_log_entry(name, result);
            }

            // List tasks
            let tasks = scheduler.get_tasks_mut();
            let mut indices_to_remove = Vec::new();

            ui.horizontal(|ui| {
                if ui.button("Add New Task").clicked() {
                    self.show_task_dialog = true;
                }
            });

            ui.separator();

            for (i, task) in tasks.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let mut enabled = task.enabled;
                    if ui.checkbox(&mut enabled, &task.name).changed() {
                        task.enabled = enabled;
                    }

                    ui.label(format!("{:?}", task.schedule));
                    ui.label(format!("{:?}", task.action));

                    if ui.button("Delete").clicked() {
                        indices_to_remove.push(i);
                    }
                });
            }

            // Remove deleted tasks (in reverse order to maintain indices)
            for i in indices_to_remove.iter().rev() {
                scheduler.remove_task(*i);
            }

            // Task Log
            ui.separator();
            ui.heading("Task Log");
            let log = scheduler.get_task_log();
            for (name, time, result) in log.iter().rev().take(10) {
                ui.label(format!(
                    "{} - {}: {}",
                    time.duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    name,
                    result
                ));
            }
        }
    }

    fn draw_rules(&mut self, ui: &mut egui::Ui) {
        ui.heading("Automation Rules");

        ui.label("Define rules to automatically manage processes based on conditions.");
        ui.label("Example: cpu > 80.0");

        ui.horizontal(|ui| {
            ui.label("Active Rule:");
            if let Ok(re) = self.rule_engine.lock() {
                if let Some(rule) = &re.active_rule {
                    ui.label(rule);
                } else {
                    ui.label("None");
                }
            }
        });

        if ui.button("Set Rule").clicked() {
            self.show_rule_dialog = true;
        }

        if ui.button("Clear Rule").clicked() {
            if let Ok(mut re) = self.rule_engine.lock() {
                re.set_rule("".to_string());
            }
        }
    }

    fn draw_nice_dialog(&mut self, ctx: &egui::Context) {
        if self.show_nice_dialog {
            egui::Window::new("Change Nice Value")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Enter new nice value (-20 to 19):");
                    ui.text_edit_singleline(&mut self.nice_input);

                    if let Some(err) = &self.nice_error_message {
                        ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() {
                            if let Ok(nice) = self.nice_input.parse::<i32>() {
                                if let Some(pid) = self.selected_process_pid {
                                    if let Ok(pm) = self.process_manager.lock() {
                                        match pm.set_niceness(pid, nice) {
                                            Ok(_) => {
                                                self.show_nice_dialog = false;
                                                self.nice_input.clear();
                                                self.nice_error_message = None;
                                                // Refresh happens in next frame or we can force it
                                            }
                                            Err(e) => {
                                                self.nice_error_message = Some(e.to_string());
                                            }
                                        }
                                    }
                                    self.refresh();
                                }
                            } else {
                                self.nice_error_message = Some("Invalid integer".to_string());
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_nice_dialog = false;
                            self.nice_input.clear();
                            self.nice_error_message = None;
                        }
                    });
                });
        }
    }

    fn draw_task_dialog(&mut self, ctx: &egui::Context) {
        if self.show_task_dialog {
            egui::Window::new("Add Scheduled Task")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Task Name:");
                    ui.text_edit_singleline(&mut self.task_name_input);

                    ui.label("Schedule Type:");
                    egui::ComboBox::from_id_source("schedule_type")
                        .selected_text(match self.task_schedule_type_index {
                            0 => "Interval",
                            1 => "Cron",
                            2 => "OneShot",
                            _ => "Unknown",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.task_schedule_type_index, 0, "Interval");
                            ui.selectable_value(&mut self.task_schedule_type_index, 1, "Cron");
                            ui.selectable_value(&mut self.task_schedule_type_index, 2, "OneShot");
                        });

                    match self.task_schedule_type_index {
                        0 => {
                            ui.label("Interval (seconds):");
                            ui.text_edit_singleline(&mut self.task_interval_input);
                        }
                        1 => {
                            ui.label("Cron Expression:");
                            ui.text_edit_singleline(&mut self.task_cron_input);
                        }
                        2 => {
                            ui.label("Time (RFC3339):");
                            ui.text_edit_singleline(&mut self.task_oneshot_input);
                        }
                        _ => {}
                    }

                    ui.label("Action:");
                    egui::ComboBox::from_id_source("action_type")
                        .selected_text(match self.task_action_index {
                            0 => "Kill",
                            1 => "Stop",
                            2 => "Lower Priority",
                            3 => "Apply Rule",
                            _ => "Unknown",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.task_action_index, 0, "Kill");
                            ui.selectable_value(&mut self.task_action_index, 1, "Stop");
                            ui.selectable_value(&mut self.task_action_index, 2, "Lower Priority");
                            ui.selectable_value(&mut self.task_action_index, 3, "Apply Rule");
                        });

                    ui.label("Target (PID or Rule):");
                    ui.text_edit_singleline(&mut self.task_action_target_input);

                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            // Construct task and add to scheduler
                            // Simplified for now
                            if let Ok(mut scheduler) = self.scheduler.lock() {
                                let schedule = match self.task_schedule_type_index {
                                    0 => ScheduleType::Interval(
                                        self.task_interval_input.parse().unwrap_or(60),
                                    ),
                                    1 => ScheduleType::Cron(self.task_cron_input.clone()),
                                    // Simplified other types
                                    _ => ScheduleType::Interval(60),
                                };

                                let action = match self.task_action_index {
                                    0 => ScheduleAction::KillProcess {
                                        pid: self.task_action_target_input.parse().unwrap_or(0),
                                    },
                                    1 => ScheduleAction::StopProcess {
                                        pid: self.task_action_target_input.parse().unwrap_or(0),
                                    },
                                    2 => ScheduleAction::ReniceProcess {
                                        pid: self.task_action_target_input.parse().unwrap_or(0),
                                        nice: 10,
                                    },
                                    3 => ScheduleAction::ApplyRule {
                                        rule: self.task_action_target_input.clone(),
                                    },
                                    _ => ScheduleAction::KillProcess { pid: 0 },
                                };

                                let task = ScheduledTask::new(
                                    self.task_name_input.clone(),
                                    schedule,
                                    action,
                                );

                                scheduler.add_task(task);
                            }

                            self.show_task_dialog = false;
                            // Clear inputs
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_task_dialog = false;
                        }
                    });
                });
        }
    }

    fn draw_rule_dialog(&mut self, ctx: &egui::Context) {
        if self.show_rule_dialog {
            egui::Window::new("Set Automation Rule")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Enter rule expression (Rhia script):");
                    ui.label("Available variables: cpu, mem, pid, name");
                    ui.text_edit_multiline(&mut self.rule_input);

                    ui.horizontal(|ui| {
                        if ui.button("Set").clicked() {
                            if let Ok(mut re) = self.rule_engine.lock() {
                                re.set_rule(self.rule_input.clone());
                            }
                            self.show_rule_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_rule_dialog = false;
                        }
                    });
                });
        }
    }

    fn draw_confirmation_dialog(&mut self, ctx: &egui::Context) {
        if self.show_confirmation_dialog {
            egui::Window::new("Confirm Action")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(&self.confirmation_message);

                    ui.horizontal(|ui| {
                        if self.show_kill_tree_option {
                            if ui.button("Kill Parent Only").clicked() {
                                if let Some(action) = &self.pending_action {
                                    if let PendingAction::Kill(pid) = action {
                                        if let Ok(pm) = self.process_manager.lock() {
                                            if let Err(e) = pm.kill_process(*pid) {
                                                self.last_error =
                                                    Some(format!("Failed to kill process: {}", e));
                                            } else {
                                                self.last_error = None;
                                            }
                                        }
                                    }
                                }
                                self.refresh();
                                self.show_confirmation_dialog = false;
                                self.pending_action = None;
                                self.show_kill_tree_option = false;
                            }
                            if ui.button("Kill Tree (Parent + Children)").clicked() {
                                if let Some(action) = &self.pending_action {
                                    if let PendingAction::Kill(pid) = action {
                                        if let Ok(pm) = self.process_manager.lock() {
                                            if let Err(e) = pm.kill_process_and_children(*pid) {
                                                self.last_error = Some(format!(
                                                    "Failed to kill process tree: {}",
                                                    e
                                                ));
                                            } else {
                                                self.last_error = None;
                                            }
                                        }
                                    }
                                }
                                self.refresh();
                                self.show_confirmation_dialog = false;
                                self.pending_action = None;
                                self.show_kill_tree_option = false;
                            }
                        } else {
                            if ui.button("Yes").clicked() {
                                if let Some(action) = &self.pending_action {
                                    let result = if let Ok(pm) = self.process_manager.lock() {
                                        match action {
                                            PendingAction::Kill(pid) => pm.kill_process(*pid),
                                            PendingAction::KillTree(pid) => {
                                                pm.kill_process_and_children(*pid).map(|_| ())
                                            }
                                            PendingAction::Stop(pid) => pm.stop_process(*pid),
                                            PendingAction::Terminate(pid) => {
                                                pm.terminate_process(*pid)
                                            }
                                            PendingAction::Continue(pid) => {
                                                pm.continue_process(*pid)
                                            }
                                        }
                                    } else {
                                        Err(std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            "Failed to lock process manager",
                                        ))
                                    };

                                    if let Err(e) = result {
                                        self.last_error = Some(format!("Operation failed: {}", e));
                                    } else {
                                        self.last_error = None;
                                    }
                                }
                                self.refresh();
                                self.show_confirmation_dialog = false;
                                self.pending_action = None;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_confirmation_dialog = false;
                            self.pending_action = None;
                            self.show_kill_tree_option = false;
                        }
                    });
                });
        }
    }

    fn draw_profile_dialog(&mut self, ctx: &egui::Context) {
        if self.show_profile_dialog {
            let title = if self.profile_edit_mode {
                format!("Edit Profile: {}", self.profile_edit_name)
            } else {
                "Create New Profile".to_string()
            };

            egui::Window::new(title)
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Profile Name:");
                    ui.text_edit_singleline(&mut self.profile_name_input);
                    ui.add_space(5.0);

                    ui.label("Prioritize Processes (comma-separated patterns):");
                    ui.text_edit_singleline(&mut self.profile_prioritize_input);
                    ui.label("Example: firefox, chrome, code");
                    ui.add_space(5.0);

                    ui.label("Hide Processes (comma-separated patterns):");
                    ui.text_edit_singleline(&mut self.profile_hide_input);
                    ui.label("Example: systemd, kthreadd");
                    ui.add_space(5.0);

                    ui.label("Nice Adjustments:");
                    ui.horizontal(|ui| {
                        ui.label("Pattern:");
                        ui.text_edit_singleline(&mut self.profile_nice_pattern_input);
                        ui.label("Value:");
                        ui.text_edit_singleline(&mut self.profile_nice_value_input);
                    });
                    ui.label("Note: Nice adjustments are simplified in the GUI. Use TUI for advanced settings.");
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if !self.profile_name_input.trim().is_empty() {
                                if let Ok(mut pm) = self.profile_manager.lock() {
                                    let mut profile = crate::profile::Profile::new(self.profile_name_input.trim().to_string());

                                    // Parse prioritize processes
                                    profile.prioritize_processes = self.profile_prioritize_input
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                        .collect();

                                    // Parse hide processes
                                    profile.hide_processes = self.profile_hide_input
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                        .collect();

                                    // Parse nice adjustment
                                    if !self.profile_nice_pattern_input.trim().is_empty() {
                                        if let Ok(nice_value) = self.profile_nice_value_input.parse::<i32>() {
                                            profile.nice_adjustments.insert(
                                                self.profile_nice_pattern_input.trim().to_string(),
                                                nice_value
                                            );
                                        }
                                    }

                                    pm.add_profile(profile);
                                }
                                self.show_profile_dialog = false;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_profile_dialog = false;
                        }
                    });
                });
        }
    }

    fn draw_alert_dialog(&mut self, ctx: &egui::Context) {
        if self.show_alert_dialog {
            egui::Window::new("Create Alert")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Alert Name:");
                    ui.text_edit_singleline(&mut self.alert_name_input);
                    ui.add_space(5.0);

                    ui.label("Condition Type:");
                    egui::ComboBox::from_id_source("alert_condition")
                        .selected_text(match self.alert_condition_index {
                            0 => "CPU Greater Than",
                            1 => "Memory Greater Than",
                            2 => "Process Died",
                            _ => "Unknown",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.alert_condition_index,
                                0,
                                "CPU Greater Than",
                            );
                            ui.selectable_value(
                                &mut self.alert_condition_index,
                                1,
                                "Memory Greater Than",
                            );
                            ui.selectable_value(&mut self.alert_condition_index, 2, "Process Died");
                        });
                    ui.add_space(5.0);

                    if self.alert_condition_index != 2 {
                        ui.label("Threshold:");
                        ui.text_edit_singleline(&mut self.alert_threshold_input);
                        ui.label(match self.alert_condition_index {
                            0 => "CPU percentage (e.g., 80.0)",
                            1 => "Memory in MB (e.g., 1024)",
                            _ => "",
                        });
                        ui.add_space(5.0);

                        ui.label("Duration (seconds):");
                        ui.text_edit_singleline(&mut self.alert_duration_input);
                        ui.label("How long the condition must persist");
                        ui.add_space(5.0);
                    }

                    ui.label("Target:");
                    egui::ComboBox::from_id_source("alert_target")
                        .selected_text(match self.alert_target_index {
                            0 => "All Processes",
                            1 => "Pattern Match",
                            _ => "Unknown",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.alert_target_index, 0, "All Processes");
                            ui.selectable_value(&mut self.alert_target_index, 1, "Pattern Match");
                        });

                    if self.alert_target_index == 1 {
                        ui.label("Process Pattern:");
                        ui.text_edit_singleline(&mut self.alert_target_pattern_input);
                        ui.label("Process name pattern (e.g., firefox)");
                    }
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            if !self.alert_name_input.trim().is_empty() {
                                if let Ok(mut am) = self.alert_manager.lock() {
                                    let condition = match self.alert_condition_index {
                                        0 => {
                                            let threshold = self
                                                .alert_threshold_input
                                                .parse::<f32>()
                                                .unwrap_or(80.0);
                                            let duration = self
                                                .alert_duration_input
                                                .parse::<u64>()
                                                .unwrap_or(5);
                                            crate::alert::AlertCondition::CpuGreaterThan {
                                                threshold,
                                                duration_secs: duration,
                                            }
                                        }
                                        1 => {
                                            let threshold = self
                                                .alert_threshold_input
                                                .parse::<u64>()
                                                .unwrap_or(1024);
                                            let duration = self
                                                .alert_duration_input
                                                .parse::<u64>()
                                                .unwrap_or(5);
                                            crate::alert::AlertCondition::MemoryGreaterThan {
                                                threshold_mb: threshold,
                                                duration_secs: duration,
                                            }
                                        }
                                        2 => {
                                            // If target is "All Processes" (0), use wildcard pattern
                                            let pattern = if self.alert_target_index == 0 {
                                                "*".to_string()
                                            } else {
                                                self.alert_target_pattern_input.clone()
                                            };
                                            crate::alert::AlertCondition::ProcessDied { pattern }
                                        }
                                        _ => crate::alert::AlertCondition::CpuGreaterThan {
                                            threshold: 80.0,
                                            duration_secs: 5,
                                        },
                                    };

                                    let target = match self.alert_target_index {
                                        0 => crate::alert::AlertTarget::All,
                                        1 => crate::alert::AlertTarget::Pattern(
                                            self.alert_target_pattern_input.clone(),
                                        ),
                                        _ => crate::alert::AlertTarget::All,
                                    };

                                    let alert = crate::alert::Alert {
                                        name: self.alert_name_input.trim().to_string(),
                                        condition,
                                        target,
                                        enabled: true,
                                    };

                                    am.add_alert(alert);
                                }
                                self.show_alert_dialog = false;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_alert_dialog = false;
                        }
                    });
                });
        }
    }
}

pub fn run_gui() -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Linux Process Manager")
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Linux Process Manager",
        options,
        Box::new(|_cc| Box::new(GuiApp::default())),
    )
    .map_err(|e| format!("GUI error: {}", e).into())
}
