//! Process grouping module for cgroups, containers, and namespaces

use crate::process::ProcessInfo;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub enum GroupType {
    Cgroup,
    Container,
    Namespace(String), // namespace type (e.g., "pid", "net", "mnt")
    Username,          // Group by actual username (e.g., "mohab", "root")
}

#[derive(Clone)]
pub struct ProcessGroup {
    pub group_type: GroupType,
    pub group_id: String,
    pub processes: Vec<ProcessInfo>,
    pub total_cpu: f32,
    pub total_memory: u64,
}

impl ProcessGroup {
    pub fn new(group_type: GroupType, group_id: String) -> Self {
        Self {
            group_type,
            group_id,
            processes: Vec::new(),
            total_cpu: 0.0,
            total_memory: 0,
        }
    }

    pub fn add_process(&mut self, process: ProcessInfo) {
        self.total_cpu += process.cpu_usage;
        self.total_memory += process.memory_usage;
        self.processes.push(process);
    }

    pub fn process_count(&self) -> usize {
        self.processes.len()
    }
}

pub struct ProcessGroupManager;

impl ProcessGroupManager {
    /// Group processes by cgroup
    pub fn group_by_cgroup(processes: &[ProcessInfo]) -> Vec<ProcessGroup> {
        let mut groups: HashMap<String, ProcessGroup> = HashMap::new();

        for process in processes {
            if let Some(cgroup) = &process.cgroup {
                let group_id = cgroup.clone();
                let group = groups
                    .entry(group_id.clone())
                    .or_insert_with(|| ProcessGroup::new(GroupType::Cgroup, group_id));
                group.add_process(process.clone());
            } else {
                // Processes without cgroup go to "No cgroup"
                let group_id = "No cgroup".to_string();
                let group = groups
                    .entry(group_id.clone())
                    .or_insert_with(|| ProcessGroup::new(GroupType::Cgroup, group_id));
                group.add_process(process.clone());
            }
        }

        groups.into_values().collect()
    }

    /// Group processes by container ID
    pub fn group_by_container(processes: &[ProcessInfo]) -> Vec<ProcessGroup> {
        let mut groups: HashMap<String, ProcessGroup> = HashMap::new();

        for process in processes {
            if let Some(container_id) = &process.container_id {
                let group_id = container_id.clone();
                let group = groups
                    .entry(group_id.clone())
                    .or_insert_with(|| ProcessGroup::new(GroupType::Container, group_id));
                group.add_process(process.clone());
            } else {
                // Processes not in containers go to "No container"
                let group_id = "No container".to_string();
                let group = groups
                    .entry(group_id.clone())
                    .or_insert_with(|| ProcessGroup::new(GroupType::Container, group_id));
                group.add_process(process.clone());
            }
        }

        groups.into_values().collect()
    }

    /// Group processes by namespace type
    ///
    /// Note: In Linux, every process should have namespace IDs for all namespace types.
    /// If a process doesn't have a namespace type, it's likely an error reading /proc/<pid>/ns/*.
    /// We exclude such processes from grouping rather than creating a "None" group that could
    /// collide with valid namespace ID 0.
    pub fn group_by_namespace(
        processes: &[ProcessInfo],
        namespace_type: &str,
    ) -> Vec<ProcessGroup> {
        let mut groups: HashMap<u64, ProcessGroup> = HashMap::new();

        for process in processes {
            // Only group processes that have the namespace type
            // Processes without namespace IDs are excluded (likely read errors)
            if let Some(&namespace_id) = process.namespace_ids.get(namespace_type) {
                let group_id = format!("{}:{}", namespace_type, namespace_id);
                let group = groups.entry(namespace_id).or_insert_with(|| {
                    ProcessGroup::new(GroupType::Namespace(namespace_type.to_string()), group_id)
                });
                group.add_process(process.clone());
            }
            // Explicitly exclude processes without this namespace type
            // This is consistent with get_namespace_groups() behavior
        }

        groups.into_values().collect()
    }

    /// Get all available namespace types from processes
    pub fn get_available_namespace_types(processes: &[ProcessInfo]) -> Vec<String> {
        let mut types = std::collections::HashSet::new();
        for process in processes {
            for ns_type in process.namespace_ids.keys() {
                types.insert(ns_type.clone());
            }
        }
        let mut result: Vec<String> = types.into_iter().collect();
        result.sort();
        result
    }

    /// Group processes by actual username (e.g., "mohab", "root")
    pub fn group_by_username(processes: &[ProcessInfo]) -> Vec<ProcessGroup> {
        let mut groups: HashMap<String, ProcessGroup> = HashMap::new();

        for process in processes {
            let username = process
                .user
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let group = groups
                .entry(username.clone())
                .or_insert_with(|| ProcessGroup::new(GroupType::Username, username));
            group.add_process(process.clone());
        }

        groups.into_values().collect()
    }
}
