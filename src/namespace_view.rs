//! Namespace view module for detailed namespace information and drill-down

use crate::process::ProcessInfo;

#[derive(Clone)]
pub struct NamespaceGroup {
    pub namespace_type: String,
    pub namespace_id: u64,
    pub processes: Vec<ProcessInfo>,
    pub cpu_usage: f32,
    pub memory_usage: u64,
}

impl NamespaceGroup {
    pub fn new(namespace_type: String, namespace_id: u64) -> Self {
        Self {
            namespace_type,
            namespace_id,
            processes: Vec::new(),
            cpu_usage: 0.0,
            memory_usage: 0,
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

/// Get all namespace groups for a specific namespace type
pub fn get_namespace_groups(
    processes: &[ProcessInfo],
    namespace_type: &str,
) -> Vec<NamespaceGroup> {
    let mut groups: std::collections::HashMap<u64, NamespaceGroup> =
        std::collections::HashMap::new();

    for process in processes {
        if let Some(&namespace_id) = process.namespace_ids.get(namespace_type) {
            let group = groups
                .entry(namespace_id)
                .or_insert_with(|| NamespaceGroup::new(namespace_type.to_string(), namespace_id));
            group.add_process(process.clone());
        }
    }

    groups.into_values().collect()
}

/// Get namespace group details for a specific namespace ID
pub fn get_namespace_group_details(
    processes: &[ProcessInfo],
    namespace_type: &str,
    namespace_id: u64,
) -> Option<NamespaceGroup> {
    let mut group = NamespaceGroup::new(namespace_type.to_string(), namespace_id);

    for process in processes {
        if process
            .namespace_ids
            .get(namespace_type)
            .map(|&id| id == namespace_id)
            .unwrap_or(false)
        {
            group.add_process(process.clone());
        }
    }

    if group.process_count() > 0 {
        Some(group)
    } else {
        None
    }
}
