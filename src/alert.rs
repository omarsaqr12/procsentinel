//! Alerts and notifications for process thresholds

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertTarget {
    All,
    Pattern(String), // Process name pattern
    Pid(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    CpuGreaterThan {
        threshold: f32,
        duration_secs: u64,
    },
    MemoryGreaterThan {
        threshold_mb: u64,
        duration_secs: u64,
    },
    IoGreaterThan {
        threshold_mb_per_sec: f64,
        duration_secs: u64,
    },
    ProcessDied {
        pattern: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub name: String,
    pub condition: AlertCondition,
    pub target: AlertTarget,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveAlert {
    pub alert_name: String,
    pub triggered_at: SystemTime,
    pub process_pid: Option<u32>,
    pub process_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlertConfig {
    alerts: Vec<Alert>,
}

pub struct AlertManager {
    alerts: Vec<Alert>,
    active_alerts: Vec<ActiveAlert>,
    condition_tracking: HashMap<String, (SystemTime, u32)>, // (alert_name, process_pid) -> (start_time, count)
    config_path: PathBuf,
}

impl AlertManager {
    pub fn new() -> Self {
        let config_dir = dirs::home_dir()
            .map(|mut p| {
                p.push(".lpm");
                p
            })
            .unwrap_or_else(|| PathBuf::from("."));

        let config_path = config_dir.join("alerts.toml");

        let mut manager = Self {
            alerts: Vec::new(),
            active_alerts: Vec::new(),
            condition_tracking: HashMap::new(),
            config_path,
        };

        // Load alerts from file
        let _ = manager.load_alerts();

        manager
    }

    pub fn get_alerts(&self) -> &[Alert] {
        &self.alerts
    }

    pub fn get_alerts_mut(&mut self) -> &mut Vec<Alert> {
        &mut self.alerts
    }

    pub fn add_alert(&mut self, alert: Alert) {
        self.alerts.push(alert);
        let _ = self.save_alerts();
    }

    pub fn remove_alert(&mut self, index: usize) -> Option<Alert> {
        if index < self.alerts.len() {
            let removed = self.alerts.remove(index);
            let _ = self.save_alerts();
            Some(removed)
        } else {
            None
        }
    }

    pub fn toggle_alert(&mut self, index: usize) -> bool {
        if let Some(alert) = self.alerts.get_mut(index) {
            alert.enabled = !alert.enabled;
            let _ = self.save_alerts();
            true
        } else {
            false
        }
    }

    pub fn get_active_alerts(&self) -> &[ActiveAlert] {
        &self.active_alerts
    }

    pub fn clear_active_alert(&mut self, index: usize) {
        if index < self.active_alerts.len() {
            self.active_alerts.remove(index);
        }
    }

    pub fn clear_all_active_alerts(&mut self) {
        self.active_alerts.clear();
    }

    /// Check alert conditions against process data
    pub fn check_alerts(
        &mut self,
        processes: &[crate::process::ProcessInfo],
        prev_processes: &std::collections::HashMap<u32, String>,
    ) {
        let now = SystemTime::now();
        let current_pids: std::collections::HashSet<u32> =
            processes.iter().map(|p| p.pid).collect();

        // Check for process death alerts
        for alert in &self.alerts {
            if !alert.enabled {
                continue;
            }

            if let AlertCondition::ProcessDied { pattern } = &alert.condition {
                for (pid, name) in prev_processes {
                    if !current_pids.contains(pid) {
                        // Process died - check if it matches pattern
                        let matches = if pattern == "*" {
                            true
                        } else {
                            name.contains(pattern)
                        };

                        if matches {
                            // Check if we already have an active alert for this death
                            // We use a unique key for the death event based on alert name and PID
                            if !self
                                .active_alerts
                                .iter()
                                .any(|a| a.alert_name == alert.name && a.process_pid == Some(*pid))
                            {
                                self.active_alerts.push(ActiveAlert {
                                    alert_name: alert.name.clone(),
                                    triggered_at: now,
                                    process_pid: Some(*pid),
                                    process_name: Some(name.clone()),
                                    message: format!("Process {} ({}) died", name, pid),
                                });
                            }
                        }
                    }
                }
            }
        }

        // Check threshold-based alerts
        for alert in &self.alerts {
            if !alert.enabled {
                continue;
            }

            for process in processes {
                // Check if process matches target
                let matches_target = match &alert.target {
                    AlertTarget::All => true,
                    AlertTarget::Pattern(pattern) => process.name.contains(pattern),
                    AlertTarget::Pid(pid) => process.pid == *pid,
                };

                if !matches_target {
                    continue;
                }

                let key = format!("{}:{}", alert.name, process.pid);
                let should_trigger = match &alert.condition {
                    AlertCondition::CpuGreaterThan {
                        threshold,
                        duration_secs,
                    } => {
                        if process.cpu_usage > *threshold {
                            let entry = self
                                .condition_tracking
                                .entry(key.clone())
                                .or_insert_with(|| (now, 0));
                            entry.1 += 1;

                            if let Ok(elapsed) = now.duration_since(entry.0) {
                                elapsed.as_secs() >= *duration_secs
                            } else {
                                false
                            }
                        } else {
                            // Condition no longer met - clear tracking
                            self.condition_tracking.remove(&key);
                            false
                        }
                    }
                    AlertCondition::MemoryGreaterThan {
                        threshold_mb,
                        duration_secs,
                    } => {
                        let memory_mb = process.memory_usage / (1024 * 1024);
                        if memory_mb > *threshold_mb {
                            let entry = self
                                .condition_tracking
                                .entry(key.clone())
                                .or_insert_with(|| (now, 0));
                            entry.1 += 1;

                            if let Ok(elapsed) = now.duration_since(entry.0) {
                                elapsed.as_secs() >= *duration_secs
                            } else {
                                false
                            }
                        } else {
                            self.condition_tracking.remove(&key);
                            false
                        }
                    }
                    AlertCondition::IoGreaterThan { .. } => {
                        // I/O monitoring would require additional tracking
                        false
                    }
                    AlertCondition::ProcessDied { .. } => false, // Handled above
                };

                if should_trigger {
                    // Check if alert already active for this process
                    if !self
                        .active_alerts
                        .iter()
                        .any(|a| a.alert_name == alert.name && a.process_pid == Some(process.pid))
                    {
                        let message = match &alert.condition {
                            AlertCondition::CpuGreaterThan { threshold, .. } => {
                                format!(
                                    "{}: Process {} (PID: {}) CPU > {}% for threshold duration",
                                    alert.name, process.name, process.pid, threshold
                                )
                            }
                            AlertCondition::MemoryGreaterThan { threshold_mb, .. } => {
                                format!(
                                    "{}: Process {} (PID: {}) Memory > {}MB for threshold duration",
                                    alert.name, process.name, process.pid, threshold_mb
                                )
                            }
                            _ => format!("{}: Alert triggered", alert.name),
                        };

                        self.active_alerts.push(ActiveAlert {
                            alert_name: alert.name.clone(),
                            triggered_at: now,
                            process_pid: Some(process.pid),
                            process_name: Some(process.name.clone()),
                            message,
                        });
                    }
                }
            }
        }

        // Clean up old active alerts (older than 5 minutes)
        let five_minutes_ago = now - Duration::from_secs(300);
        self.active_alerts
            .retain(|a| a.triggered_at > five_minutes_ago);
    }

    fn load_alerts(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.config_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.config_path)?;
        let config: AlertConfig = toml::from_str(&content)?;
        self.alerts = config.alerts;
        Ok(())
    }

    fn save_alerts(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let config = AlertConfig {
            alerts: self.alerts.clone(),
        };

        let content = toml::to_string_pretty(&config)?;
        fs::write(&self.config_path, content)?;
        Ok(())
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}
