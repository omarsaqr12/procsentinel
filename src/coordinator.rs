//! Multi-host coordination - Coordinator side (main LPM instance)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;
use crate::process::ProcessInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHost {
    pub address: String,  // IP:port or hostname:port
    pub name: String,
    pub connected: bool,
    pub last_update: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub parent_pid: Option<u32>,
    pub status: String,
    pub user: Option<String>,
    pub nice: i32,
    pub start_time_str: String,
    pub start_timestamp: u64, // Store actual start timestamp (seconds since boot)
    pub host: String,  // Host identifier
}

impl From<RemoteProcessInfo> for ProcessInfo {
    fn from(rp: RemoteProcessInfo) -> Self {
        Self {
            pid: rp.pid,
            name: rp.name,
            cpu_usage: rp.cpu_usage,
            memory_usage: rp.memory_usage,
            parent_pid: rp.parent_pid,
            status: rp.status,
            user: rp.user,
            nice: rp.nice,
            start_time_str: rp.start_time_str,
            start_timestamp: rp.start_timestamp, // Use remote process start timestamp
            cgroup: None,
            container_id: None,
            namespace_ids: std::collections::HashMap::new(),
            host: Some(rp.host),
        }
    }
}

pub struct Coordinator {
    hosts: Vec<RemoteHost>,
    remote_processes: HashMap<String, Vec<RemoteProcessInfo>>, // host -> processes
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            hosts: Vec::new(),
            remote_processes: HashMap::new(),
        }
    }

    pub fn add_host(&mut self, address: String, name: String) {
        // Check if host already exists
        if !self.hosts.iter().any(|h| h.address == address) {
            self.hosts.push(RemoteHost {
                address,
                name,
                connected: false,
                last_update: None,
            });
        }
    }

    pub fn remove_host(&mut self, address: &str) {
        self.hosts.retain(|h| h.address != address);
        self.remote_processes.remove(address);
    }

    pub fn get_hosts(&self) -> &[RemoteHost] {
        &self.hosts
    }

    pub fn get_remote_processes(&self) -> Vec<RemoteProcessInfo> {
        self.remote_processes.values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn update_host_data(&mut self, host_address: &str, processes: Vec<RemoteProcessInfo>) {
        // Update host connection status
        if let Some(host) = self.hosts.iter_mut().find(|h| h.address == host_address) {
            host.connected = true;
            host.last_update = Some(std::time::SystemTime::now());
        }
        
        self.remote_processes.insert(host_address.to_string(), processes);
    }

    pub fn mark_host_disconnected(&mut self, host_address: &str) {
        if let Some(host) = self.hosts.iter_mut().find(|h| h.address == host_address) {
            host.connected = false;
        }
    }
}

// Standalone async function to fetch data
pub async fn fetch_host_data(host_address: String, host_name: String) -> Result<Vec<RemoteProcessInfo>, String> {
    let url = format!("http://{}/api/processes", host_address);
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    
    let response = timeout(Duration::from_secs(5), client.get(&url).send())
        .await
        .map_err(|_| "Request timeout".to_string())?
        .map_err(|e| format!("HTTP request failed: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }
    
    #[derive(Deserialize)]
    struct AgentProcessInfo {
        pid: u32,
        name: String,
        cpu_usage: f32,
        memory_usage: u64,
        parent_pid: Option<u32>,
        status: String,
        user: Option<String>,
        nice: i32,
        start_time_str: String,
        #[serde(default)]
        start_timestamp: u64,
    }
    
    let agent_processes: Vec<AgentProcessInfo> = response.json()
        .await
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    
    let processes: Vec<RemoteProcessInfo> = agent_processes.into_iter()
        .map(|ap| RemoteProcessInfo {
            pid: ap.pid,
            name: ap.name,
            cpu_usage: ap.cpu_usage,
            memory_usage: ap.memory_usage,
            parent_pid: ap.parent_pid,
            status: ap.status,
            user: ap.user,
            nice: ap.nice,
            start_time_str: ap.start_time_str,
            start_timestamp: ap.start_timestamp,
            host: host_name.clone(),
        })
        .collect();
    
    Ok(processes)
}

impl Coordinator {

    pub async fn test_connection(&self, host_address: &str) -> bool {
        let url = format!("http://{}/api/health", host_address);
        
        if let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            if let Ok(response) = timeout(Duration::from_secs(2), client.get(&url).send()).await {
                if let Ok(resp) = response {
                    return resp.status().is_success();
                }
            }
        }
        false
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

