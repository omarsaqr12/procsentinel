//! Read-only process-inspection agent.
//!
//! The endpoint discloses process names, users and resource usage. It has no
//! application-level authentication or TLS, so it must not bind publicly.
//! For remote access, forward the loopback port through an authenticated SSH
//! connection rather than exposing it directly on the network.

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::process::{ProcessInfo, ProcessManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub parent_pid: Option<u32>,
    pub status: String,
    pub user: Option<String>,
    pub nice: i32,
    pub start_time_str: String,
    pub start_timestamp: u64,
}

impl From<ProcessInfo> for AgentProcessInfo {
    fn from(proc: ProcessInfo) -> Self {
        Self {
            pid: proc.pid,
            name: proc.name,
            cpu_usage: proc.cpu_usage,
            memory_usage: proc.memory_usage,
            parent_pid: proc.parent_pid,
            status: proc.status,
            user: proc.user,
            nice: proc.nice,
            start_time_str: proc.start_time_str,
            start_timestamp: proc.start_timestamp,
        }
    }
}

#[derive(Clone)]
pub struct AgentState {
    process_manager: Arc<RwLock<ProcessManager>>,
}

pub struct Agent {
    state: AgentState,
    port: u16,
}

// Centralize the listening policy so tests can catch accidental public binds.
fn agent_listen_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

impl Agent {
    pub fn new(port: u16) -> Self {
        let process_manager = Arc::new(RwLock::new(ProcessManager::new()));
        let state = AgentState { process_manager };

        Self { state, port }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let app = Router::new()
            .route("/api/health", get(health_check))
            .route("/api/processes", get(get_processes))
            .with_state(self.state.clone());

        let addr = agent_listen_addr(self.port);
        let listener = tokio::net::TcpListener::bind(addr).await?;

        println!("Agent server listening on {} (loopback only; use an SSH tunnel for remote access)", addr);

        axum::serve(listener, app).await?;

        Ok(())
    }
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

async fn get_processes(
    State(state): State<AgentState>,
) -> Result<Json<Vec<AgentProcessInfo>>, StatusCode> {
    let mut pm = state.process_manager.write().await;
    pm.refresh();

    let processes: Vec<AgentProcessInfo> = pm.get_processes()
        .iter()
        .map(|p| AgentProcessInfo::from(p.clone()))
        .collect();

    Ok(Json(processes))
}

#[cfg(test)]
mod tests {
    use super::agent_listen_addr;

    #[test]
    fn agent_never_binds_to_all_network_interfaces() {
        for port in [0, 3000, 8080, u16::MAX] {
            let addr = agent_listen_addr(port);
            assert!(addr.ip().is_loopback(), "unexpected public bind: {addr}");
            assert_eq!(addr.port(), port);
        }
    }
}
