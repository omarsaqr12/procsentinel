//! CRIU (Checkpoint/Restore in Userspace) integration for fault tolerance

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// A checkpoint ID is a single, limited ASCII path component, never a path.
fn valid_checkpoint_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn read_restore_pid(path: &Path) -> Result<u32, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("CRIU did not create a readable PID file: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("CRIU PID file is not a regular file".to_string());
    }
    let data =
        std::fs::read_to_string(path).map_err(|e| format!("Could not read CRIU PID file: {e}"))?;
    match data.trim().parse::<u32>() {
        Ok(pid) if pid > 0 => Ok(pid),
        _ => Err("CRIU returned a missing, invalid, or zero restored PID".to_string()),
    }
}

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
                let mut checkpoint_id: Option<String> = None;
                let mut pid = None;
                let mut process_name = None;
                let mut created_at_secs = None;
                let mut metadata = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
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

                if !valid_checkpoint_id(&checkpoint_id) {
                    return Err(de::Error::custom("invalid checkpoint_id"));
                }
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

#[derive(Serialize, Deserialize)]
struct CheckpointListFile {
    checkpoints: Vec<CheckpointInfo>,
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

    // Non-following checks prevent ordinary path traversal and symlink escapes.
    // This assumes a private user-owned checkpoint root: hostile concurrent renames
    // of this directory or its ancestors would require directory-handle confinement.
    fn checkpoint_path(&self, id: &str, must_exist: bool) -> Result<PathBuf, String> {
        if !valid_checkpoint_id(id) {
            return Err(
                "Invalid checkpoint ID: expected 1-128 ASCII letters, digits, _ or -".to_string(),
            );
        }
        let root = std::fs::symlink_metadata(&self.checkpoint_base_dir)
            .map_err(|e| format!("Cannot inspect checkpoint root: {e}"))?;
        if root.file_type().is_symlink() || !root.is_dir() {
            return Err("Checkpoint root must be a real directory".to_string());
        }
        let base = self
            .checkpoint_base_dir
            .canonicalize()
            .map_err(|e| format!("Cannot resolve checkpoint root: {e}"))?;
        let candidate = base.join(id);
        match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                Err("Checkpoint entry is a symlink or not a directory".to_string())
            }
            Ok(_) if !must_exist => Err("Checkpoint ID already exists".to_string()),
            Ok(_) => Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !must_exist => Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err("Checkpoint directory not found".to_string())
            }
            Err(e) => Err(format!("Cannot inspect checkpoint directory: {e}")),
        }
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

        let checkpoint_dir = self.checkpoint_path(&checkpoint_id, false)?;

        // Create a new validated direct child; never reuse an existing checkpoint.
        std::fs::create_dir(&checkpoint_dir)
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
        let checkpoint_dir = self.checkpoint_path(checkpoint_id, true)?;
        // A unique pidfile prevents stale results from previous restores being reused.
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("System clock error: {e}"))?
            .as_nanos();
        let pid_file = checkpoint_dir.join(format!("restore-{}-{nonce}.pid", std::process::id()));
        if std::fs::symlink_metadata(&pid_file).is_ok() {
            return Err("CRIU PID file already exists".to_string());
        }

        // Run CRIU restore command
        // Note: CRIU restore typically requires root privileges and specific setup
        // This is a simplified implementation
        let output = Command::new(criu_path)
            .arg("restore")
            .arg("-D")
            .arg(&checkpoint_dir)
            .arg("--pidfile")
            .arg(&pid_file)
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

        // A successful CRIU exit without a valid PID must never be reported as success.
        let pid = read_restore_pid(&pid_file)?;
        let _ = std::fs::remove_file(&pid_file);
        Ok(pid)
    }

    pub fn list_checkpoints(&self) -> Vec<CheckpointInfo> {
        let mut checkpoints = Vec::new();

        match std::fs::symlink_metadata(&self.checkpoint_base_dir) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            _ => return checkpoints,
        }

        // Load from metadata file
        let metadata_file = self.checkpoint_base_dir.join("checkpoints.toml");
        if let Ok(content) = std::fs::read_to_string(&metadata_file) {
            if let Ok(mut metadata_list) = toml::from_str::<CheckpointListFile>(&content)
                .map(|file| file.checkpoints)
                .or_else(|_| toml::from_str::<Vec<CheckpointInfo>>(&content))
            {
                // Restore SystemTime and PathBuf from serialized data
                for checkpoint in &mut metadata_list {
                    checkpoint.created_at =
                        UNIX_EPOCH + std::time::Duration::from_secs(checkpoint.created_at_secs);
                    if let Ok(path) = self.checkpoint_path(&checkpoint.checkpoint_id, true) {
                        checkpoint.checkpoint_dir = path;
                        checkpoints.push(checkpoint.clone());
                    }
                }
            }
        }

        // Also scan directory for checkpoints without metadata
        if let Ok(entries) = std::fs::read_dir(&self.checkpoint_base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                // DirEntry::file_type does not follow symlinks.
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.starts_with("checkpoint_") && valid_checkpoint_id(s))
                        .unwrap_or(false)
                {
                    let checkpoint_id = path.file_name().unwrap().to_string_lossy().to_string();
                    let path = match self.checkpoint_path(&checkpoint_id, true) {
                        Ok(path) => path,
                        Err(_) => continue,
                    };

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
        let checkpoint_dir = self.checkpoint_path(checkpoint_id, true)?;

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
        let content = toml::to_string_pretty(&CheckpointListFile {
            checkpoints: checkpoints.to_vec(),
        })
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

#[cfg(test)]
mod checkpoint_security_tests {
    use super::*;

    fn disposable_manager() -> CriuManager {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let checkpoint_base_dir = std::env::temp_dir().join(format!(
            "procsentinel-criu-test-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&checkpoint_base_dir).unwrap();
        CriuManager {
            criu_path: None,
            available: false,
            checkpoint_base_dir,
        }
    }

    #[test]
    fn rejects_traversal_absolute_and_invalid_names() {
        assert!(valid_checkpoint_id("checkpoint_123-abc"));
        for bad in [
            "",
            ".",
            "..",
            "../outside",
            "/tmp/outside",
            "a/b",
            "a\\b",
            "é",
            "x x",
        ] {
            assert!(!valid_checkpoint_id(bad), "accepted: {bad}");
        }
        assert!(!valid_checkpoint_id(&"a".repeat(129)));
        let manager = disposable_manager();
        for bad in ["../outside", "/tmp/outside", "a/b", ".."] {
            assert!(manager.delete_checkpoint(bad).is_err());
        }
        std::fs::remove_dir_all(manager.checkpoint_base_dir).unwrap();
    }

    #[test]
    fn deletion_stays_inside_private_checkpoint_root() {
        let manager = disposable_manager();
        let outside = manager.checkpoint_base_dir.with_extension("outside");
        std::fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("KEEP");
        std::fs::write(&sentinel, "do not remove").unwrap();
        assert!(manager.delete_checkpoint("../outside").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                &outside,
                manager.checkpoint_base_dir.join("checkpoint_link"),
            )
            .unwrap();
            assert!(manager.delete_checkpoint("checkpoint_link").is_err());
            assert!(
                !manager
                    .list_checkpoints()
                    .iter()
                    .any(|i| i.checkpoint_id == "checkpoint_link")
            );
        }
        let good = manager.checkpoint_base_dir.join("checkpoint_good");
        std::fs::create_dir(&good).unwrap();
        std::fs::write(good.join("artifact"), "ok").unwrap();
        assert!(manager.delete_checkpoint("checkpoint_good").is_ok());
        assert!(!good.exists());
        assert!(sentinel.exists());
        std::fs::remove_dir_all(manager.checkpoint_base_dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn rejects_malicious_deserialized_metadata() {
        let malicious = serde_json::json!({
            "checkpoint_id": "../escape", "pid": 42,
            "process_name": "test", "created_at_secs": 1
        });
        assert!(serde_json::from_value::<CheckpointInfo>(malicious).is_err());
    }

    #[test]
    fn metadata_roundtrip_and_delete_persist_consistently() {
        let manager = disposable_manager();
        let dir = manager.checkpoint_base_dir.join("checkpoint_metadata");
        std::fs::create_dir(&dir).unwrap();
        let now = SystemTime::now();
        let secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let info = CheckpointInfo {
            checkpoint_id: "checkpoint_metadata".to_string(),
            pid: 123,
            process_name: "disposable".to_string(),
            checkpoint_dir: dir.clone(),
            created_at: now,
            created_at_secs: secs,
            metadata: None,
        };
        manager.save_all_checkpoints_metadata(&[info]).unwrap();
        let raw =
            std::fs::read_to_string(manager.checkpoint_base_dir.join("checkpoints.toml")).unwrap();
        let parsed = toml::from_str::<CheckpointListFile>(&raw).unwrap();
        assert_eq!(parsed.checkpoints[0].pid, 123);
        let restored = manager.list_checkpoints();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].pid, 123);
        manager.delete_checkpoint("checkpoint_metadata").unwrap();
        assert!(!dir.exists());
        assert!(manager.list_checkpoints().is_empty());
        std::fs::remove_dir_all(manager.checkpoint_base_dir).unwrap();
    }

    #[test]
    fn pidfile_requires_a_positive_pid_in_regular_file() {
        let manager = disposable_manager();
        let file = manager.checkpoint_base_dir.join("test.pid");
        for invalid in ["", "0", "nope", "4294967296"] {
            std::fs::write(&file, invalid).unwrap();
            assert!(read_restore_pid(&file).is_err());
        }
        std::fs::write(&file, "1234\n").unwrap();
        assert_eq!(read_restore_pid(&file).unwrap(), 1234);
        std::fs::remove_dir_all(manager.checkpoint_base_dir).unwrap();
    }
}
