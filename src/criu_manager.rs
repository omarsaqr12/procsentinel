//! CRIU (Checkpoint/Restore in Userspace) integration for fault tolerance

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointInfo {
    pub checkpoint_id: String,
    pub pid: u32,
    pub process_name: String,
    #[serde(skip)]
    pub checkpoint_dir: PathBuf,
    #[serde(skip)]
    pub created_at: SystemTime,
    pub created_at_secs: u64, // Serializable timestamp
    pub metadata: Option<String>,
}

// Custom Deserialize implementation because SystemTime doesn't implement Default
impl<'de> Deserialize<'de> for CheckpointInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct CheckpointInfoVisitor;

        impl<'de> Visitor<'de> for CheckpointInfoVisitor {
            type Value = CheckpointInfo;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct CheckpointInfo")
            }

            fn visit_map<V>(self, mut map: V) -> Result<CheckpointInfo, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut checkpoint_id = None;
                let mut pid = None;
                let mut process_name = None;
                let mut created_at_secs = None;
                let mut metadata = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "checkpoint_id" => {
                            if checkpoint_id.is_some() {
                                return Err(de::Error::duplicate_field("checkpoint_id"));
                            }
                            checkpoint_id = Some(map.next_value()?);
                        }
                        "pid" => {
                            if pid.is_some() {
                                return Err(de::Error::duplicate_field("pid"));
                            }
                            pid = Some(map.next_value()?);
                        }
                        "process_name" => {
                            if process_name.is_some() {
                                return Err(de::Error::duplicate_field("process_name"));
                            }
                            process_name = Some(map.next_value()?);
                        }
                        "created_at_secs" => {
                            if created_at_secs.is_some() {
                                return Err(de::Error::duplicate_field("created_at_secs"));
                            }
                            created_at_secs = Some(map.next_value()?);
                        }
                        "metadata" => {
                            if metadata.is_some() {
                                return Err(de::Error::duplicate_field("metadata"));
                            }
                            metadata = Some(map.next_value()?);
                        }
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let checkpoint_id =
                    checkpoint_id.ok_or_else(|| de::Error::missing_field("checkpoint_id"))?;
                let pid = pid.ok_or_else(|| de::Error::missing_field("pid"))?;
                let process_name =
                    process_name.ok_or_else(|| de::Error::missing_field("process_name"))?;
                let created_at_secs = created_at_secs.unwrap_or(0);
                let metadata = metadata;

                let created_at = UNIX_EPOCH + Duration::from_secs(created_at_secs);

                Ok(CheckpointInfo {
                    checkpoint_id,
                    pid,
                    process_name,
                    checkpoint_dir: PathBuf::new(),
                    created_at,
                    created_at_secs,
                    metadata,
                })
            }
        }

        deserializer.deserialize_map(CheckpointInfoVisitor)
    }
}

impl Default for CheckpointInfo {
    fn default() -> Self {
        Self {
            checkpoint_id: String::new(),
            pid: 0,
            process_name: String::new(),
            checkpoint_dir: PathBuf::new(),
            created_at: SystemTime::now(),
            created_at_secs: 0,
            metadata: None,
        }
    }
}

pub struct CriuManager {
    criu_path: Option<PathBuf>,
    available: bool,
    checkpoint_base_dir: PathBuf,
}

impl CriuManager {
    pub fn new() -> Self {
        // Check if CRIU is available
        let criu_path = Self::find_criu();
        let available = criu_path.is_some();

        // Default checkpoint directory
        let checkpoint_base_dir = dirs::home_dir()
            .map(|mut p| {
                p.push(".lpm");
                p.push("checkpoints");
                p
            })
            .unwrap_or_else(|| PathBuf::from("./checkpoints"));

        // Create checkpoint directory if it doesn't exist
        if let Some(parent) = checkpoint_base_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::create_dir_all(&checkpoint_base_dir);

        Self {
            criu_path,
            available,
            checkpoint_base_dir,
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    fn find_criu() -> Option<PathBuf> {
        // Check common CRIU locations
        let possible_paths = vec![
            PathBuf::from("/usr/bin/criu"),
            PathBuf::from("/usr/local/bin/criu"),
            PathBuf::from("/sbin/criu"),
        ];

        for path in possible_paths {
            if path.exists() {
                // Verify it's actually CRIU
                if let Ok(output) = Command::new(&path).arg("--version").output() {
                    if output.status.success() {
                        return Some(path);
                    }
                }
            }
        }

        // Try to find in PATH
        if let Ok(output) = Command::new("which").arg("criu").output() {
            if output.status.success() {
                if let Ok(path_str) = String::from_utf8(output.stdout) {
                    let path = PathBuf::from(path_str.trim());
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }

        None
    }

    pub fn checkpoint_process(
        &self,
        pid: u32,
        process_name: &str,
        checkpoint_id: Option<String>,
    ) -> Result<CheckpointInfo, String> {
        if !self.available {
            return Err("CRIU is not available on this system. Please install CRIU to use checkpoint functionality.".to_string());
        }

        let criu_path = self.criu_path.as_ref().ok_or("CRIU path not found")?;

        // Generate checkpoint ID if not provided
        let checkpoint_id = checkpoint_id.unwrap_or_else(|| {
            format!(
                "checkpoint_{}_{}",
                pid,
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            )
        });

        let checkpoint_dir = self.checkpoint_base_dir.join(&checkpoint_id);

        // Create checkpoint directory
        std::fs::create_dir_all(&checkpoint_dir)
            .map_err(|e| format!("Failed to create checkpoint directory: {}", e))?;

        // Run CRIU dump command
        let output = Command::new(criu_path)
            .arg("dump")
            .arg("-t")
            .arg(pid.to_string())
            .arg("-D")
            .arg(&checkpoint_dir)
            .arg("--leave-running") // Keep process running after checkpoint
            .output()
            .map_err(|e| format!("Failed to execute CRIU: {}", e))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!("CRIU checkpoint failed: {}", error_msg));
        }

        let now = SystemTime::now();
        let created_at_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        let checkpoint_info = CheckpointInfo {
            checkpoint_id: checkpoint_id.clone(),
            pid,
            process_name: process_name.to_string(),
            checkpoint_dir,
            created_at: now,
            created_at_secs,
            metadata: Some(format!("PID: {}, Process: {}", pid, process_name)),
        };

        // Save checkpoint metadata
        self.save_checkpoint_metadata(&checkpoint_info)?;

        Ok(checkpoint_info)
    }

    pub fn restore_process(&self, checkpoint_id: &str) -> Result<u32, String> {
        if !self.available {
            return Err("CRIU is not available on this system.".to_string());
        }

        let criu_path = self.criu_path.as_ref().ok_or("CRIU path not found")?;
        let checkpoint_dir = self.checkpoint_base_dir.join(checkpoint_id);

        if !checkpoint_dir.exists() {
            return Err(format!(
                "Checkpoint directory not found: {:?}",
                checkpoint_dir
            ));
        }

        // Run CRIU restore command
        // Note: CRIU restore typically requires root privileges and specific setup
        // This is a simplified implementation
        let output = Command::new(criu_path)
            .arg("restore")
            .arg("-D")
            .arg(&checkpoint_dir)
            .arg("-d") // Detach from terminal
            .output()
            .map_err(|e| format!("Failed to execute CRIU restore: {}", e))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "CRIU restore failed: {}. Note: CRIU restore typically requires root privileges and proper setup.",
                error_msg
            ));
        }

        // Try to read PID from checkpoint directory
        // CRIU stores the PID in various files, this is a simplified approach
        // In a real implementation, you'd parse the CRIU image files
        let pid_file = checkpoint_dir.join("pidfile");
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                return Ok(pid);
            }
        }

        // If we can't get PID from file, return a placeholder
        // In practice, CRIU restore would give us the PID
        Ok(0) // Placeholder - actual implementation would track restored PID
    }

    pub fn list_checkpoints(&self) -> Vec<CheckpointInfo> {
        let mut checkpoints = Vec::new();

        if !self.checkpoint_base_dir.exists() {
            return checkpoints;
        }

        // Load from metadata file
        let metadata_file = self.checkpoint_base_dir.join("checkpoints.toml");
        if let Ok(content) = std::fs::read_to_string(&metadata_file) {
            if let Ok(mut metadata_list) = toml::from_str::<Vec<CheckpointInfo>>(&content) {
                // Restore SystemTime and PathBuf from serialized data
                for checkpoint in &mut metadata_list {
                    checkpoint.created_at =
                        UNIX_EPOCH + std::time::Duration::from_secs(checkpoint.created_at_secs);
                    checkpoint.checkpoint_dir =
                        self.checkpoint_base_dir.join(&checkpoint.checkpoint_id);

                    // Filter out checkpoints that no longer exist
                    if checkpoint.checkpoint_dir.exists() {
                        checkpoints.push(checkpoint.clone());
                    }
                }
            }
        }

        // Also scan directory for checkpoints without metadata
        if let Ok(entries) = std::fs::read_dir(&self.checkpoint_base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.starts_with("checkpoint_"))
                        .unwrap_or(false)
                {
                    let checkpoint_id = path.file_name().unwrap().to_string_lossy().to_string();

                    // Check if already in list
                    if !checkpoints.iter().any(|c| c.checkpoint_id == checkpoint_id) {
                        // Try to load metadata or create basic info
                        let created_at = entry
                            .metadata()
                            .ok()
                            .and_then(|m| m.created().ok())
                            .unwrap_or_else(|| SystemTime::now());
                        let created_at_secs = created_at
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        let checkpoint_info = CheckpointInfo {
                            checkpoint_id: checkpoint_id.clone(),
                            pid: 0,
                            process_name: "Unknown".to_string(),
                            checkpoint_dir: path.clone(),
                            created_at,
                            created_at_secs,
                            metadata: None,
                        };
                        checkpoints.push(checkpoint_info);
                    }
                }
            }
        }

        // Sort by creation time (newest first)
        checkpoints.sort_by(|a, b| b.created_at_secs.cmp(&a.created_at_secs));

        // Restore SystemTime from serialized timestamp
        for checkpoint in &mut checkpoints {
            if checkpoint.created_at == SystemTime::UNIX_EPOCH {
                checkpoint.created_at =
                    UNIX_EPOCH + std::time::Duration::from_secs(checkpoint.created_at_secs);
            }
        }

        checkpoints
    }

    pub fn delete_checkpoint(&self, checkpoint_id: &str) -> Result<(), String> {
        let checkpoint_dir = self.checkpoint_base_dir.join(checkpoint_id);

        if !checkpoint_dir.exists() {
            return Err(format!("Checkpoint not found: {}", checkpoint_id));
        }

        std::fs::remove_dir_all(&checkpoint_dir)
            .map_err(|e| format!("Failed to delete checkpoint: {}", e))?;

        // Update metadata file
        let mut checkpoints = self.list_checkpoints();
        checkpoints.retain(|c| c.checkpoint_id != checkpoint_id);
        self.save_all_checkpoints_metadata(&checkpoints)?;

        Ok(())
    }

    fn save_checkpoint_metadata(&self, checkpoint: &CheckpointInfo) -> Result<(), String> {
        let mut checkpoints = self.list_checkpoints();

        // Remove existing checkpoint with same ID
        checkpoints.retain(|c| c.checkpoint_id != checkpoint.checkpoint_id);
        checkpoints.push(checkpoint.clone());

        self.save_all_checkpoints_metadata(&checkpoints)
    }

    fn save_all_checkpoints_metadata(&self, checkpoints: &[CheckpointInfo]) -> Result<(), String> {
        let metadata_file = self.checkpoint_base_dir.join("checkpoints.toml");

        // Convert SystemTime to a serializable format
        // For simplicity, we'll use a simplified serialization
        let content = toml::to_string_pretty(checkpoints)
            .map_err(|e| format!("Failed to serialize checkpoints: {}", e))?;

        std::fs::write(&metadata_file, content)
            .map_err(|e| format!("Failed to write checkpoint metadata: {}", e))?;

        Ok(())
    }

    pub fn get_checkpoint_base_dir(&self) -> &Path {
        &self.checkpoint_base_dir
    }
}

impl Default for CriuManager {
    fn default() -> Self {
        Self::new()
    }
}
