use crate::scripting_rules::RuleEngine;
use crate::filter_parser::{FilterParser, FilterExpression};
use sysinfo::{ProcessExt, System, SystemExt, PidExt, UserExt};
#[cfg(target_os = "linux")]
use procfs::process::Process as ProcfsProcess; // Import procfs for nice value
use std::convert::TryInto; // Import the try_into function
use chrono::{Local, TimeZone};
use libc::{self, c_int};
use std::collections::HashMap;

#[derive(Clone)] 
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub parent_pid: Option<u32>,
    pub status: String,
    pub user: Option<String>,
    pub nice: i32, 
    pub start_time_str: String,
    pub start_timestamp: u64, // Store actual start timestamp (seconds since boot) for uptime calculation
    pub cgroup: Option<String>,
    pub container_id: Option<String>,
    pub namespace_ids: std::collections::HashMap<String, u64>,
    pub host: Option<String>, // Host identifier for multi-host mode (None = local)
}

pub struct ProcessManager {
    system: System,
    filtered_processes: Vec<ProcessInfo>,// for the scripting
    processes: Vec<ProcessInfo>,
    sort_mode: Option<String>,
    sort_ascending: bool,
    filter_mode: Option<String>,
    filter_value: Option<String>,
    advanced_filter: Option<FilterExpression>,
    filter_parser: FilterParser,
    spawned_children: Vec<std::process::Child>,
}

impl ProcessManager {
    pub fn new() -> Self {
        let mut system = System::new_all(); 
        system.refresh_all(); 
        ProcessManager { 
            system,
            processes: Vec::new(),
            filtered_processes: Vec::new(),
            sort_mode: None,
            sort_ascending: true,
            filter_mode: None,
            filter_value: None,
            advanced_filter: None,
            filter_parser: FilterParser::new(),
            spawned_children: Vec::new(),
        }
    }

    pub fn refresh(&mut self) {
        // Reap zombie processes
        let mut i = 0;
        while i < self.spawned_children.len() {
            if let Some(child) = self.spawned_children.get_mut(i) {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        // Process finished, remove it
                        self.spawned_children.remove(i);
                    }
                    Ok(None) => {
                        // Process still running
                        i += 1;
                    }
                    Err(_) => {
                        // Error checking, remove it
                        self.spawned_children.remove(i);
                    }
                }
            } else {
                break;
            }
        }

        self.system.refresh_all();
        self.update_processes();
        // Re-apply sort if there is an active sort mode
        if let Some(mode) = self.sort_mode.clone() {
            self.sort_processes(&mode);
        }
    }

    pub fn set_filter(&mut self, mode: Option<String>, value: Option<String>) {
        self.filter_mode = mode;
        self.filter_value = value;
        self.advanced_filter = None; // Clear advanced filter when using simple filter
        self.update_processes(); // Refresh to apply filter
    }

    /// Set advanced filter expression
    pub fn set_advanced_filter(&mut self, filter_expr: Option<FilterExpression>) {
        self.advanced_filter = filter_expr;
        self.filter_mode = None; // Clear simple filter when using advanced filter
        self.filter_value = None;
        self.update_processes();
    }

    /// Parse and set advanced filter from string
    pub fn set_advanced_filter_string(&mut self, filter_str: &str) -> Result<(), String> {
        if filter_str.trim().is_empty() {
            self.set_advanced_filter(None);
            return Ok(());
        }
        
        let expr = self.filter_parser.parse(filter_str)?;
        self.set_advanced_filter(Some(expr));
        Ok(())
    }

    pub fn get_advanced_filter_string(&self) -> Option<String> {
        // For now, we don't serialize back to string
        // This could be enhanced later
        None
    }

    fn update_processes(&mut self) {
        let mut processes = Vec::new();
        
        for (pid, process) in self.system.processes() {
            // Retrieve nice value using procfs (Linux only)
            #[cfg(target_os = "linux")]
            let nice_value = {
                let pid_i32: i32 = pid.as_u32().try_into().unwrap_or(0);
                ProcfsProcess::new(pid_i32)
                    .and_then(|p| p.stat().map(|stat| stat.nice))
                    .unwrap_or(0)
            };
            #[cfg(not(target_os = "linux"))]
            let nice_value = {
                // Use libc to get nice value on macOS
                unsafe {
                    // Clear errno before call
                    *libc::__error() = 0;
                    let prio = libc::getpriority(libc::PRIO_PROCESS, pid.as_u32());
                    // getpriority returns -1 on error AND on valid priority -1.
                    // Must check errno.
                    if prio == -1 && *libc::__error() != 0 {
                        0 // Error, default to 0
                    } else {
                        prio
                    }
                }
            };
            // Format the start time
            let formatted_time = format_timestamp(process.start_time());
            let pid_u32 = pid.as_u32();
            
            // Get cgroup, container, and namespace info
            let cgroup = get_cgroup(pid_u32);
            let container_id = cgroup.as_ref().and_then(|cg| get_container_id(cg));
            let namespace_ids = get_namespace_ids(pid_u32);
            
            // Determine status - prefer procfs on Linux for accuracy
            #[cfg(target_os = "linux")]
            let raw_status = {
                let pid_i32: i32 = pid.as_u32().try_into().unwrap_or(0);
                ProcfsProcess::new(pid_i32)
                    .and_then(|p| p.stat().map(|stat| match stat.state {
                        'R' => "Running".to_string(),
                        'S' => "Sleeping".to_string(),
                        'D' => "Disk Sleep".to_string(),
                        'Z' => "Zombie".to_string(),
                        'T' => "Stopped".to_string(),
                        't' => "Tracing Stop".to_string(),
                        'X' | 'x' => "Dead".to_string(),
                        'K' => "Wakekill".to_string(),
                        'W' => "Waking".to_string(),
                        'P' => "Parked".to_string(),
                        'I' => "Idle".to_string(),
                        _ => format!("Unknown({})", stat.state),
                    }))
                    .unwrap_or_else(|_| process.status().to_string())
            };
            #[cfg(not(target_os = "linux"))]
            let raw_status = process.status().to_string();

            // Check for both "Sleep" and "Sleeping" as sysinfo output varies
            // If CPU usage > 0, consider it Running regardless of reported state (often transient)
            let status = if process.cpu_usage() > 0.0 && (raw_status == "Sleep" || raw_status == "Sleeping" || raw_status == "Idle") {
                "Run".to_string()
            } else {
                raw_status
            };

            let proc_info = ProcessInfo {
                pid: pid_u32,
                name: process.name().to_string(),
                cpu_usage: process.cpu_usage() / self.system.cpus().len() as f32,
                memory_usage: process.memory(),
                parent_pid: process.parent().map(|p| p.as_u32()),
                status,
                user: process.user_id()
                    .and_then(|id| self.system.get_user_by_id(id)
                    .map(|user| user.name().to_string())),
                nice: nice_value as i32,
                start_time_str: formatted_time,
                start_timestamp: process.start_time(), // Store actual start timestamp (seconds since boot)
                cgroup,
                container_id,
                namespace_ids,
                host: None, // Local processes have no host
            };

            // Apply advanced filter if set
            if let Some(ref filter_expr) = self.advanced_filter {
                if !self.filter_parser.evaluate(&proc_info, filter_expr) {
                    continue;
                }
            }
            // Apply simple filter if set (and no advanced filter)
            else if let (Some(mode), Some(value)) = (&self.filter_mode, &self.filter_value) {
                let should_include = match mode.as_str() {
                    "user" => proc_info.user.as_ref().map_or(false, |u| u.contains(value)),
                    "name" => proc_info.name.to_lowercase().contains(&value.to_lowercase()),
                    "pid" => proc_info.pid.to_string().contains(value),
                    "ppid" => proc_info.parent_pid.map_or(false, |p| p.to_string().contains(value)),
                    _ => true,
                };
                if !should_include {
                    continue;
                }
            }

            processes.push(proc_info);
        }
        
        self.processes = processes;

        // Re-apply sort if there is an active sort mode
        if let Some(mode) = self.sort_mode.clone() {
            self.sort_processes(&mode);
        }
    }

    pub fn get_processes(&self) -> &Vec<ProcessInfo> {
        &self.processes
    }

    pub fn get_filtered_processes(&self) -> &Vec<ProcessInfo> {
        &self.filtered_processes
    }

    pub fn set_sort(&mut self, mode: &str, ascending: bool) {
        self.sort_mode = Some(mode.to_string());
        self.sort_ascending = ascending;
        self.sort_processes(mode);
    }

    fn sort_processes(&mut self, mode: &str) {
        match mode {
            "pid" => {
                if self.sort_ascending {
                    self.processes.sort_by_key(|p| p.pid);
                } else {
                    self.processes.sort_by_key(|p| std::cmp::Reverse(p.pid));
                }
            }
            "mem" => {
                if self.sort_ascending {
                    self.processes.sort_by_key(|p| p.memory_usage);
                } else {
                    self.processes.sort_by_key(|p| std::cmp::Reverse(p.memory_usage));
                }
            }
            "ppid" => {
                if self.sort_ascending {
                    self.processes.sort_by_key(|p| p.parent_pid.unwrap_or(0));
                } else {
                    self.processes.sort_by_key(|p| std::cmp::Reverse(p.parent_pid.unwrap_or(0)));
                }
            }
            "start" => {
                if self.sort_ascending {
                    self.processes.sort_by(|a, b| a.start_time_str.cmp(&b.start_time_str));
                } else {
                    self.processes.sort_by(|a, b| b.start_time_str.cmp(&a.start_time_str));
                }
            }
            "nice" => {
                if self.sort_ascending {
                    self.processes.sort_by_key(|p| p.nice);
                } else {
                    self.processes.sort_by_key(|p| std::cmp::Reverse(p.nice));
                }
            }
            "cpu" => {
                if self.sort_ascending {
                    self.processes.sort_by(|a, b| a.cpu_usage.partial_cmp(&b.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
                } else {
                    self.processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
                }
            }
            "name" => {
                if self.sort_ascending {
                    self.processes.sort_by(|a, b| a.name.cmp(&b.name));
                } else {
                    self.processes.sort_by(|a, b| b.name.cmp(&a.name));
                }
            }
            "user" => {
                if self.sort_ascending {
                    self.processes.sort_by(|a, b| {
                        let a_user = a.user.as_ref().map(|s| s.as_str()).unwrap_or("");
                        let b_user = b.user.as_ref().map(|s| s.as_str()).unwrap_or("");
                        a_user.cmp(b_user)
                    });
                } else {
                    self.processes.sort_by(|a, b| {
                        let a_user = a.user.as_ref().map(|s| s.as_str()).unwrap_or("");
                        let b_user = b.user.as_ref().map(|s| s.as_str()).unwrap_or("");
                        b_user.cmp(a_user)
                    });
                }
            }
            "status" => {
                if self.sort_ascending {
                    self.processes.sort_by(|a, b| a.status.cmp(&b.status));
                } else {
                    self.processes.sort_by(|a, b| b.status.cmp(&a.status));
                }
            }
            _ => {}
        }
    }

    /// Apply profile-based prioritization to move prioritized processes to the top
    /// This should be called after sort_processes() to maintain sort order within groups
    pub fn apply_prioritization<F>(&mut self, is_prioritized: F) 
    where
        F: Fn(&str) -> bool
    {
        // Stable partition: prioritized first, others second
        // This maintains the relative order within each group (preserving the sort)
        self.processes.sort_by_key(|p| !is_prioritized(&p.name));
    }


    pub fn set_niceness(&self, pid: u32, nice: i32) -> std::io::Result<()> {
        // Validate niceness range
        if nice < -20 || nice > 19 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Nice value must be between -20 and 19"
            ));
        }

        // Check privileges if setting negative nice
        if nice < 0 && unsafe { libc::geteuid() } != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Root privileges required for negative nice values (use sudo)"
            ));
        }
        let temp_pid: libc::id_t = pid;

        // SAFETY: This is safe because we're passing valid arguments
        let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, temp_pid, nice as c_int) };
        
        if result != 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("Failed to set nice for PID {}: {}", pid, err);
            return Err(err);
        }

        Ok(())
    }

    pub fn apply_nice_adjustments<F>(&self, get_nice_adjustment: F) -> (usize, usize)
    where
        F: Fn(&str) -> Option<i32>
    {
        let mut success_count = 0;
        let mut fail_count = 0;

        for process in &self.processes {
            if let Some(nice_value) = get_nice_adjustment(&process.name) {
                // Only apply if the nice value is different from current
                if process.nice != nice_value {
                    match self.set_niceness(process.pid, nice_value) {
                        Ok(_) => success_count += 1,
                        Err(_) => fail_count += 1, 
                    }
                }
            }
        }

        (success_count, fail_count)
    }


    pub fn stop_process(&self, pid: u32) -> std::io::Result<()> {
        use libc::{kill, pid_t, SIGSTOP};
        
        let temp_pid: pid_t = pid as pid_t;
        
        // SAFETY: This is safe because we're passing valid arguments
        let result = unsafe { kill(temp_pid, SIGSTOP) };
        
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        
        Ok(())
    }
    

    pub fn kill_process(&self, pid: u32) -> std::io::Result<()> {
        use libc::{kill, pid_t, SIGKILL};
        
        let temp_pid: pid_t = pid as pid_t;
        
        // SAFETY: This is safe because we're passing valid arguments
        let result = unsafe { kill(temp_pid, SIGKILL) };
        
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        
        Ok(())
    }

    pub fn continue_process(&self, pid: u32) -> std::io::Result<()> {
        use libc::{kill, pid_t, SIGCONT};
        
        let temp_pid: pid_t = pid as pid_t;
        
        // SAFETY: This is safe because we're passing valid arguments
        let result = unsafe { kill(temp_pid, SIGCONT) };
        
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        
        Ok(())
    }

    pub fn terminate_process(&self, pid: u32) -> std::io::Result<()> {
        use libc::{kill, pid_t, SIGTERM};
        
        let temp_pid: pid_t = pid as pid_t;
        
        // SAFETY: This is safe because we're passing valid arguments
        let result = unsafe { kill(temp_pid, SIGTERM) };
        
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        
        Ok(())
    }
    
    pub fn apply_rules(&mut self, rule_engine: &mut RuleEngine) {
        self.filtered_processes = self.processes
            .iter()
            .cloned()
            .filter(|p| rule_engine.evaluate_for(p))
            .collect();
    }

    /// Kill processes by name pattern
    /// Restart processes matching the pattern by killing them and respawning with the same command/args
    pub fn restart_process_by_pattern(&mut self, pattern: &str) -> std::io::Result<Vec<u32>> {
        let mut restarted_pids = Vec::new();
        let mut processes_to_restart: Vec<(u32, String, Vec<String>)> = Vec::new();
        
        // First, collect all matching processes and read their command lines
        for process in &self.processes {
            if process.name.contains(pattern) {
                // Try to read the command line before killing
                if let Some((program, args)) = read_process_cmdline(process.pid) {
                    processes_to_restart.push((process.pid, program, args));
                } else {
                    // If we can't read cmdline, just kill it (fallback behavior)
                    if let Err(e) = self.kill_process(process.pid) {
                        return Err(e);
                    }
                    restarted_pids.push(process.pid);
                }
            }
        }
        
        // Now kill and restart each process
        for (pid, program, args) in processes_to_restart {
            // Kill the process first
            if let Err(e) = self.kill_process(pid) {
                // Log error but continue with other processes
                eprintln!("Error killing process {}: {}", pid, e);
                continue;
            }
            
            // Wait a brief moment for the process to fully terminate
            std::thread::sleep(std::time::Duration::from_millis(100));
            
            // Convert Vec<String> to Vec<&str> for start_process
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            
            // Restart the process with the same command and arguments
            match self.start_process(&program, &args_refs, None, &[]) {
                Ok(new_pid) => {
                    restarted_pids.push(new_pid);
                }
                Err(e) => {
                    // Log error but continue with other processes
                    eprintln!("Error restarting process '{}': {}", program, e);
                }
            }
        }
        
        Ok(restarted_pids)
    }

    /// Cleanup idle processes based on criteria
    pub fn cleanup_idle_processes(
        &self,
        cpu_threshold: f32,
        memory_threshold: u64,
        action: &str,
    ) -> std::io::Result<Vec<u32>> {
        let mut cleaned_pids = Vec::new();
        for process in &self.processes {
            if process.cpu_usage < cpu_threshold && process.memory_usage > memory_threshold {
                match action {
                    "kill" => {
                        if let Err(e) = self.kill_process(process.pid) {
                            return Err(e);
                        }
                    }
                    "stop" => {
                        if let Err(e) = self.stop_process(process.pid) {
                            return Err(e);
                        }
                    }
                    "lower_priority" => {
                        // Increase nice value (lower priority)
                        let new_nice = (process.nice + 5).min(19);
                        if let Err(e) = self.set_niceness(process.pid, new_nice) {
                            return Err(e);
                        }
                    }
                    _ => continue,
                }
                cleaned_pids.push(process.pid);
            }
        }
        Ok(cleaned_pids)
    }

    /// Get all child processes of a given parent PID
    pub fn get_child_processes(&self, parent_pid: u32) -> Vec<ProcessInfo> {
        self.processes.iter()
            .filter(|p| p.parent_pid == Some(parent_pid))
            .cloned()
            .collect()
    }

    /// Kill a process and all its children recursively
    pub fn kill_process_and_children(&self, pid: u32) -> std::io::Result<Vec<u32>> {
        let mut killed_pids = Vec::new();
        let children = self.get_child_processes(pid);
        
        // First kill all children recursively
        for child in &children {
            let child_children = self.get_child_processes(child.pid);
            if !child_children.is_empty() {
                // Recursively kill children of children
                let _ = self.kill_process_and_children(child.pid);
            }
            // Kill the child
            if let Err(e) = self.kill_process(child.pid) {
                return Err(e);
            }
            killed_pids.push(child.pid);
        }
        
        // Then kill the parent
        if let Err(e) = self.kill_process(pid) {
            return Err(e);
        }
        killed_pids.push(pid);
        
        Ok(killed_pids)
    }

    /// Start a new process with the given parameters
    pub fn start_process(
        &mut self,
        program: &str,
        args: &[&str],
        working_dir: Option<&str>,
        env_vars: &[(String, String)],
    ) -> std::io::Result<u32> {
        use std::process::Command;
        
        let mut command = Command::new(program);
        
        // Set arguments
        if !args.is_empty() {
            command.args(args);
        }
        
        // Log the command execution
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("lpm_debug.log") {
            writeln!(file, "Starting process: '{}' with args: {:?}", program, args).ok();
        }
        
        // Set working directory
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }
        
        // Set environment variables
        for (key, value) in env_vars {
            command.env(key, value);
        }
        
        // Redirect child process stdout/stderr to /dev/null to prevent output from interfering with TUI
        let child = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()?;
        let pid = child.id();
        
        // Store child handle to prevent zombies
        self.spawned_children.push(child);
        
        Ok(pid)
    }
}
// Helper function to read cgroup from /proc/<pid>/cgroup (Linux only)
#[cfg(target_os = "linux")]
fn get_cgroup(pid: u32) -> Option<String> {
    let cgroup_path = format!("/proc/{}/cgroup", pid);
    if let Ok(content) = std::fs::read_to_string(&cgroup_path) {
        // Parse cgroup file format: hierarchy:controller:path
        // We're interested in the path part
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                let path = parts[2].trim();
                if !path.is_empty() && path != "/" {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn get_cgroup(_pid: u32) -> Option<String> {
    None // Not supported on non-Linux systems
}

// Helper function to extract container ID from cgroup path (Linux only)
#[cfg(target_os = "linux")]
fn get_container_id(cgroup: &str) -> Option<String> {
    // Docker format: /docker/<container_id>
    if let Some(id) = cgroup.strip_prefix("/docker/") {
        if !id.is_empty() {
            // Take first 12 characters (short container ID)
            return Some(id.chars().take(12).collect());
        }
    }
    
    // Docker cgroup v2 format: /system.slice/docker-<container_id>.scope
    // Format: 0::/system.slice/docker-<64-char-hex-id>.scope
    if cgroup.contains("/system.slice/docker-") {
        // Extract the part after "docker-" and before ".scope"
        if let Some(start) = cgroup.find("/system.slice/docker-") {
            let after_docker = &cgroup[start + "/system.slice/docker-".len()..];
            if let Some(end) = after_docker.find(".scope") {
                let container_id = &after_docker[..end];
                // Container ID is 64 hex characters, take first 12 for short ID
                if container_id.len() >= 12 && container_id.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(container_id.chars().take(12).collect());
                }
            }
        }
    }
    
    // Docker cgroup v2 format (alternative): /user.slice/.../docker-<container_id>.scope
    if cgroup.contains("/docker-") && cgroup.contains(".scope") {
        // Extract container ID from docker-<id>.scope pattern
        if let Some(start) = cgroup.find("/docker-") {
            let after_docker = &cgroup[start + "/docker-".len()..];
            if let Some(end) = after_docker.find(".scope") {
                let container_id = &after_docker[..end];
                // Container ID is typically 64 hex characters, take first 12 for short ID
                if container_id.len() >= 12 && container_id.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(container_id.chars().take(12).collect());
                }
            }
        }
    }
    
    // Kubernetes format: /kubepods/.../pod<uuid>/<container_id>
    if cgroup.contains("/kubepods/") {
        // Try to extract container ID from various patterns
        for part in cgroup.split('/') {
            // Container IDs are typically 64 hex chars (full) or 12+ (short)
            if part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(part.chars().take(12).collect());
            } else if part.len() >= 12 && part.len() < 64 && part.chars().all(|c| c.is_alphanumeric()) {
                // Short container ID
                return Some(part.chars().take(12).collect());
            }
        }
    }
    
    // Podman format: /machine.slice/... or /libpod/...
    if cgroup.contains("machine.slice") || cgroup.contains("libpod") || cgroup.contains("user.slice") {
        // Extract from path - look for long alphanumeric IDs
        for part in cgroup.split('/') {
            // Container IDs in Podman are often long hex strings
            if part.len() >= 12 && part.chars().all(|c| c.is_alphanumeric() || c == '-') {
                // Filter out common non-container parts
                if !part.contains("slice") && !part.contains("scope") && part.len() >= 12 {
                    return Some(part.chars().take(12).collect());
                }
            }
        }
    }
    
    // Generic: Look for any long alphanumeric string that might be a container ID
    // This catches other container runtimes
    for part in cgroup.split('/') {
        if part.len() >= 12 && part.len() <= 64 && 
           part.chars().all(|c| c.is_alphanumeric() || c == '-') &&
           !part.contains("slice") && !part.contains("scope") && 
           !part.contains("systemd") && !part.contains("user") {
            return Some(part.chars().take(12).collect());
        }
    }
    
    None
}

#[cfg(not(target_os = "linux"))]
fn get_container_id(_cgroup: &str) -> Option<String> {
    None // Not supported on non-Linux systems
}

/// Read the command line of a process from /proc/<pid>/cmdline
/// Returns (program, args) if successful, None otherwise
#[cfg(target_os = "linux")]
fn read_process_cmdline(pid: u32) -> Option<(String, Vec<String>)> {
    use std::fs;
    use std::io::Read;
    
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    
    // Try to read the cmdline file
    if let Ok(mut file) = fs::File::open(&cmdline_path) {
        let mut contents = Vec::new();
        if file.read_to_end(&mut contents).is_ok() {
            // cmdline is null-separated, with a final null
            // Split by null bytes and filter out empty strings
            let parts: Vec<String> = contents
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|bytes| {
                    String::from_utf8_lossy(bytes).to_string()
                })
                .collect();
            
            if !parts.is_empty() {
                let program = parts[0].clone();
                let args = parts[1..].to_vec();
                return Some((program, args));
            }
        }
    }
    
    None
}

/// Read the command line of a process (non-Linux fallback)
/// On non-Linux systems, we can't easily read cmdline, so return None
#[cfg(not(target_os = "linux"))]
fn read_process_cmdline(_pid: u32) -> Option<(String, Vec<String>)> {
    None // Not supported on non-Linux systems
}

// Helper function to read namespace IDs from /proc/<pid>/ns/* (Linux only)
// 
// Returns a HashMap mapping namespace type names (e.g., "pid", "net", "mnt") to their inode IDs.
// In Linux, every process should have namespace IDs for all namespace types.
// If a process doesn't have namespace IDs, it's likely:
// 1. The process has exited
// 2. Permission denied reading /proc/<pid>/ns/*
// 3. The process is in a different mount namespace
#[cfg(target_os = "linux")]
fn get_namespace_ids(pid: u32) -> HashMap<String, u64> {
    let mut namespace_ids = HashMap::new();
    let ns_dir = format!("/proc/{}/ns", pid);
    
    // Try to read the namespace directory
    if let Ok(entries) = std::fs::read_dir(&ns_dir) {
        for entry in entries.flatten() {
            // Read the symlink target to get the inode
            // Format is typically: "pid:[4026531836]" or "[4026531836]"
            // The inode number uniquely identifies the namespace
            if let Ok(target) = std::fs::read_link(entry.path()) {
                let target_str = target.to_string_lossy();
                // Extract inode from format: [<inode>] or type:[<inode>]
                // Look for bracket pattern which is consistent across all namespace types
                if let Some(bracket_start) = target_str.find('[') {
                    if let Some(bracket_end) = target_str[bracket_start..].find(']') {
                        let inode_str = &target_str[bracket_start + 1..bracket_start + bracket_end];
                        if let Ok(inode) = inode_str.parse::<u64>() {
                            // Validate: namespace IDs should be non-zero (though 0 is technically valid)
                            // We accept all values including 0, as it's a valid namespace ID
                            let ns_type = entry.file_name().to_string_lossy().to_string();
                            namespace_ids.insert(ns_type, inode);
                        }
                        // Silently skip invalid inode formats (shouldn't happen in practice)
                    }
                }
                // Silently skip symlinks without bracket format (shouldn't happen in practice)
            }
            // Silently skip unreadable symlinks (permission issues, etc.)
        }
    }
    // Silently return empty HashMap if directory doesn't exist or can't be read
    // This is expected for processes that have exited or permission issues
    
    namespace_ids
}

#[cfg(not(target_os = "linux"))]
fn get_namespace_ids(_pid: u32) -> HashMap<String, u64> {
    HashMap::new() // Not supported on non-Linux systems
}

// Function to format the timestamp
fn format_timestamp(timestamp: u64) -> String {
    // The timestamp from sysinfo is usually in seconds since boot
    // We need to convert it to a DateTime object
    match Local.timestamp_opt(timestamp as i64, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%H:%M:%S").to_string(),
        _ => "00:00:00".to_string() // Fallback if conversion fails
    }
}
