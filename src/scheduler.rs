//! Job scheduling and automation module

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub enum ScheduleType {
    Cron(String),     // Cron expression like "0 * * * *"
    Interval(u64),    // Interval in seconds
    Once(SystemTime), // Run once at specific time
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScheduleAction {
    RestartProcess {
        pattern: String,
    },
    StartProcess {
        program: String,   // Program path or command (e.g., "firefox" or "/usr/bin/firefox")
        args: Vec<String>, // Command arguments (empty vec if none)
    },
    CleanupIdle {
        cpu_threshold: f32,    // CPU < threshold
        memory_threshold: u64, // Memory > threshold (bytes)
        duration_seconds: u64, // For Y minutes
        action: String,        // "kill", "stop", or "lower_priority"
    },
    ApplyRule {
        rule: String,
    },
    KillProcess {
        pid: u32,
    },
    StopProcess {
        pid: u32,
    },
    ContinueProcess {
        pid: u32,
    },
    ReniceProcess {
        pid: u32,
        nice: i32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub name: String,
    #[serde(with = "schedule_type_serde")]
    pub schedule: ScheduleType,
    pub action: ScheduleAction,
    pub enabled: bool,
    #[serde(skip)] // Don't serialize runtime state
    pub last_run: Option<SystemTime>,
    #[serde(skip)] // Don't serialize runtime state
    pub next_run: Option<SystemTime>,
}

// Helper module for ScheduleType serialization
mod schedule_type_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(schedule: &ScheduleType, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match schedule {
            ScheduleType::Cron(expr) => serializer.serialize_str(&format!("cron:{}", expr)),
            ScheduleType::Interval(secs) => serializer.serialize_str(&format!("interval:{}", secs)),
            ScheduleType::Once(time) => {
                let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
                serializer.serialize_str(&format!("once:{}", duration.as_secs()))
            }
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ScheduleType, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(expr) = s.strip_prefix("cron:") {
            Ok(ScheduleType::Cron(expr.to_string()))
        } else if let Some(secs_str) = s.strip_prefix("interval:") {
            let secs = secs_str.parse::<u64>().map_err(serde::de::Error::custom)?;
            Ok(ScheduleType::Interval(secs))
        } else if let Some(secs_str) = s.strip_prefix("once:") {
            let secs = secs_str.parse::<u64>().map_err(serde::de::Error::custom)?;
            Ok(ScheduleType::Once(UNIX_EPOCH + Duration::from_secs(secs)))
        } else {
            Err(serde::de::Error::custom("Invalid schedule type"))
        }
    }
}

impl ScheduledTask {
    pub fn new(name: String, schedule: ScheduleType, action: ScheduleAction) -> Self {
        Self {
            name,
            schedule,
            action,
            enabled: true,
            last_run: None,
            next_run: None,
        }
    }
}

pub struct Scheduler {
    tasks: Vec<ScheduledTask>,
    task_log: Vec<(String, SystemTime, String)>, // (task_name, time, result)
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            task_log: Vec::new(),
        }
    }

    pub fn add_task(&mut self, task: ScheduledTask) {
        self.tasks.push(task);
    }

    pub fn get_tasks(&self) -> &[ScheduledTask] {
        &self.tasks
    }

    pub fn get_tasks_mut(&mut self) -> &mut Vec<ScheduledTask> {
        &mut self.tasks
    }

    pub fn remove_task(&mut self, index: usize) -> Option<ScheduledTask> {
        if index < self.tasks.len() {
            Some(self.tasks.remove(index))
        } else {
            None
        }
    }

    pub fn toggle_task(&mut self, index: usize) -> bool {
        if let Some(task) = self.tasks.get_mut(index) {
            task.enabled = !task.enabled;
            true
        } else {
            false
        }
    }

    pub fn get_task_log(&self) -> &[(String, SystemTime, String)] {
        &self.task_log
    }

    pub fn add_log_entry(&mut self, task_name: String, result: String) {
        self.task_log.push((task_name, SystemTime::now(), result));
        // Keep only last 100 log entries
        if self.task_log.len() > 100 {
            self.task_log.remove(0);
        }
    }

    /// Check which tasks should run now and return their indices
    pub fn check_due_tasks(&mut self) -> Vec<usize> {
        let now = SystemTime::now();
        let mut due_tasks = Vec::new();

        for (i, task) in self.tasks.iter_mut().enumerate() {
            if !task.enabled {
                continue;
            }

            let should_run = match &task.schedule {
                ScheduleType::Interval(seconds) => {
                    // Check if enough time has passed since last run
                    if let Some(last) = task.last_run {
                        if let Ok(elapsed) = now.duration_since(last) {
                            elapsed.as_secs() >= *seconds
                        } else {
                            false
                        }
                    } else {
                        // First run
                        true
                    }
                }
                ScheduleType::Once(time) => {
                    // Run if time has passed and not run yet
                    now >= *time && task.last_run.is_none()
                }
                ScheduleType::Cron(expr) => {
                    // Simple cron parsing for common patterns
                    // Full cron parsing would require a library, but we can handle basic cases
                    let parts: Vec<&str> = expr.trim().split_whitespace().collect();
                    if parts.len() >= 5 {
                        // Parse: minute hour day month weekday
                        // For now, check if we're at the specified minute (basic implementation)
                        // This is a simplified version - full cron would need proper parsing
                        let minute_str = parts[0];
                        let hour_str = parts[1];

                        // Get current time components
                        use std::time::UNIX_EPOCH;
                        if let Ok(duration) = now.duration_since(UNIX_EPOCH) {
                            let total_seconds = duration.as_secs();
                            let current_minute = (total_seconds / 60) % 60;
                            let current_hour = (total_seconds / 3600) % 24;

                            // Check if minute matches (if not "*")
                            let minute_matches = minute_str == "*"
                                || minute_str
                                    .parse::<u64>()
                                    .map(|m| m == current_minute)
                                    .unwrap_or(false);

                            // Check if hour matches (if not "*")
                            let hour_matches = hour_str == "*"
                                || hour_str
                                    .parse::<u64>()
                                    .map(|h| h == current_hour)
                                    .unwrap_or(false);

                            // For simplicity, if both minute and hour are "*", run every minute
                            // Otherwise, check if we match the specified time
                            if minute_str == "*" && hour_str == "*" {
                                // Run every minute - check if at least 60 seconds passed
                                if let Some(last) = task.last_run {
                                    if let Ok(elapsed) = now.duration_since(last) {
                                        elapsed.as_secs() >= 60
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else if minute_matches && hour_matches {
                                // Matches cron expression - check if we haven't run in this minute
                                if let Some(last) = task.last_run {
                                    if let Ok(elapsed) = now.duration_since(last) {
                                        elapsed.as_secs() >= 60 // At least 1 minute since last run
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        // Invalid cron format - fallback to every minute
                        if let Some(last) = task.last_run {
                            if let Ok(elapsed) = now.duration_since(last) {
                                elapsed.as_secs() >= 60
                            } else {
                                false
                            }
                        } else {
                            true
                        }
                    }
                }
            };

            if should_run {
                due_tasks.push(i);
                task.last_run = Some(now);
                // Calculate next run time
                task.next_run = match &task.schedule {
                    ScheduleType::Interval(seconds) => {
                        now.checked_add(Duration::from_secs(*seconds))
                    }
                    ScheduleType::Once(_) => None, // Won't run again
                    ScheduleType::Cron(_) => {
                        now.checked_add(Duration::from_secs(60)) // Next minute
                    }
                };
            }
        }

        due_tasks
    }
}

/// Load scheduler tasks from config file
pub fn load_tasks() -> Vec<ScheduledTask> {
    let config_path =
        std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
            .join(".lpm")
            .join("scheduled_tasks.toml");

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(tasks) = toml::from_str::<Vec<ScheduledTask>>(&content) {
            return tasks;
        }
    }
    Vec::new()
}

/// Save scheduler tasks to config file
pub fn save_tasks(tasks: &[ScheduledTask]) -> std::io::Result<()> {
    let config_dir =
        std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
            .join(".lpm");

    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("scheduled_tasks.toml");
    let toml_string = toml::to_string_pretty(tasks)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(config_path, toml_string)
}
