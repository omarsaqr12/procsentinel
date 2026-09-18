//! Container view module for detailed container information and drill-down

use crate::process::ProcessInfo;

#[derive(Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub processes: Vec<ProcessInfo>,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub start_time: Option<String>, // Container start time if available
}

impl ContainerInfo {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            processes: Vec::new(),
            cpu_usage: 0.0,
            memory_usage: 0,
            start_time: None,
        }
    }

    pub fn add_process(&mut self, process: ProcessInfo) {
        self.cpu_usage += process.cpu_usage;
        self.memory_usage += process.memory_usage;
        self.processes.push(process);
    }

    pub fn process_count(&self) -> usize {
        self.processes.len()
    }
}

/// Get all containers from processes
pub fn get_containers(processes: &[ProcessInfo]) -> Vec<ContainerInfo> {
    let mut containers: std::collections::HashMap<String, ContainerInfo> =
        std::collections::HashMap::new();

    for process in processes {
        if let Some(container_id) = &process.container_id {
            let container = containers.entry(container_id.clone()).or_insert_with(|| {
                // Try to get container name (could be improved with Docker API)
                let name = format!("container_{}", &container_id[..12.min(container_id.len())]);
                ContainerInfo::new(container_id.clone(), name)
            });
            container.add_process(process.clone());
        }
    }

    containers.into_values().collect()
}

/// Get container details for a specific container ID
pub fn get_container_details(
    processes: &[ProcessInfo],
    container_id: &str,
) -> Option<ContainerInfo> {
    // Normalize container_id to short form (first 12 chars) for matching
    let short_id = if container_id.len() > 12 {
        &container_id[..12]
    } else {
        container_id
    };

    // Get container name using the shared function
    let container_name = get_container_name(container_id);

    let mut container = ContainerInfo::new(short_id.to_string(), container_name);

    // Match processes by container_id (comparing short IDs)
    for process in processes {
        if let Some(proc_container_id) = &process.container_id {
            // Compare short IDs (first 12 characters)
            let proc_short_id = if proc_container_id.len() > 12 {
                &proc_container_id[..12]
            } else {
                proc_container_id.as_str()
            };

            if proc_short_id == short_id {
                container.add_process(process.clone());
            }
        }
    }

    if container.process_count() > 0 {
        Some(container)
    } else {
        None
    }
}

/// Get container name from Docker by container ID (short or full)
/// Returns the container name if found, otherwise returns the ID
pub fn get_container_name(container_id: &str) -> String {
    // Normalize to short ID (first 12 chars)
    let short_id = if container_id.len() > 12 {
        &container_id[..12]
    } else {
        container_id
    };

    // Try to get container name from Docker
    // Try regular docker first, then sudo if needed (but sudo will prompt for password)
    let commands = vec![
        (
            false,
            vec![
                "docker",
                "ps",
                "--format",
                "{{.ID}} {{.Names}}",
                "--no-trunc",
            ],
        ),
        (
            true,
            vec![
                "sudo",
                "docker",
                "ps",
                "--format",
                "{{.ID}} {{.Names}}",
                "--no-trunc",
            ],
        ),
    ];

    for (use_sudo, cmd_args) in commands {
        // Skip sudo if we're on the first iteration and want to avoid password prompts
        // Only try sudo if regular docker fails
        if use_sudo {
            // Only try sudo if the first attempt failed - but this will prompt for password
            // For now, we'll skip sudo to avoid password prompts
            continue;
        }

        if let Ok(output) = std::process::Command::new(cmd_args[0])
            .args(&cmd_args[1..])
            .output()
        {
            if output.status.success() {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    for line in output_str.lines() {
                        if let Some((id, name)) = line.split_once(' ') {
                            // Normalize the ID from Docker output to short form
                            let id_short = if id.len() > 12 { &id[..12] } else { id };
                            // Match if short IDs are equal (most reliable)
                            if id_short == short_id {
                                return name.to_string();
                            }
                            // Also try matching if one starts with the other (for full vs short ID)
                            if id.starts_with(short_id) || short_id.starts_with(id_short) {
                                return name.to_string();
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: return formatted ID
    format!("container_{}", short_id)
}
