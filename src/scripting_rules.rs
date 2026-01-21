use rhai::{Engine, Scope};
use crate::process::ProcessInfo;

/// A lightweight snapshot of a process used for rule testing.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProcessSnapshot {
    pub pid: i32,
    pub name: String,
    pub memory_mb: u64,
    pub cpu_usage: f64,
    pub runtime_secs: u64,
}

#[allow(dead_code)]
pub struct RuleEngine {
    pub engine: Engine,
    pub scope: Scope<'static>,
    pub active_rule: Option<String>, // This holds the current rule
}


impl RuleEngine {
    pub fn new() -> Self {
        Self {
            engine: Engine::new(),
            scope: Scope::new(),
            active_rule: None,
        }
    }

    pub fn set_rule(&mut self, rule: String) {
        self.active_rule = Some(rule.clone());
        println!("Setting rule: {}", rule);

    }

    // Evaluate and return a boolean result for testing
    pub fn evaluate_for(&mut self, process: &ProcessInfo) -> bool {
        match &self.active_rule {
            Some(rule) if !rule.trim().is_empty() => {
                let mut scope = Scope::new();
                scope.push("cpu", process.cpu_usage as f64);
                scope.push("mem", process.memory_usage as f64 / 1024.0 / 1024.0);
                scope.push("pid", process.pid as i64);
                scope.push("name", process.name.clone() as String);
    
                let result = self.engine.eval_with_scope::<bool>(&mut scope, rule);
    
                match result {
                    Ok(val) => val,
                    Err(_) => false, // ignore errors
                }
            }
            _ => true, // No rule or empty string = allow all
        }
    }
    
    
}
