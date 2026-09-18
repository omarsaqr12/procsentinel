#!/usr/bin/env python3
"""One-use, SHA-guarded transform; remove this script after CI has committed the repair."""
import hashlib
from pathlib import Path

path = Path('src/criu_manager.rs')
original = path.read_bytes()
sha = hashlib.sha1(b'blob ' + str(len(original)).encode() + b'\0' + original).hexdigest()
assert sha == '79f4722fa7ec5de00ebd694b8e68936e712ff2a5', f'Unexpected baseline: {sha}'
s = original.decode('utf-8')

def change(old, new, label):
    global s
    matches = s.count(old)
    assert matches == 1, f'{label}: wanted one anchor; found {matches}'
    s = s.replace(old, new, 1)

change('use serde::{Deserialize, Serialize};\n', '''use serde::{Deserialize, Serialize};

// A checkpoint ID is a single, limited ASCII path component, never a path.
fn valid_checkpoint_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn read_restore_pid(path: &Path) -> Result<u32, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("CRIU did not create a readable PID file: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("CRIU PID file is not a regular file".to_string());
    }
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read CRIU PID file: {e}"))?;
    match data.trim().parse::<u32>() {
        Ok(pid) if pid > 0 => Ok(pid),
        _ => Err("CRIU returned a missing, invalid, or zero restored PID".to_string()),
    }
}
''', 'imports and guards')

change('''                let created_at = UNIX_EPOCH + Duration::from_secs(created_at_secs);

                Ok(CheckpointInfo {''', '''                if !valid_checkpoint_id(&checkpoint_id) {
                    return Err(de::Error::custom("invalid checkpoint_id"));
                }
                let created_at = UNIX_EPOCH + Duration::from_secs(created_at_secs);

                Ok(CheckpointInfo {''', 'deserialized metadata')

change('''    pub fn is_available(&self) -> bool {
        self.available
    }

    fn find_criu()''', '''    pub fn is_available(&self) -> bool {
        self.available
    }

    // Non-following checks prevent ordinary path traversal and symlink escapes.
    // This assumes a private user-owned checkpoint root: hostile concurrent renames
    // of this directory or its ancestors would require directory-handle confinement.
    fn checkpoint_path(&self, id: &str, must_exist: bool) -> Result<PathBuf, String> {
        if !valid_checkpoint_id(id) {
            return Err("Invalid checkpoint ID: expected 1-128 ASCII letters, digits, _ or -".to_string());
        }
        let root = std::fs::symlink_metadata(&self.checkpoint_base_dir)
            .map_err(|e| format!("Cannot inspect checkpoint root: {e}"))?;
        if root.file_type().is_symlink() || !root.is_dir() {
            return Err("Checkpoint root must be a real directory".to_string());
        }
        let base = self.checkpoint_base_dir.canonicalize()
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

    fn find_criu()''', 'safe path helper')

change('''        let checkpoint_dir = self.checkpoint_base_dir.join(&checkpoint_id);
        
        // Create checkpoint directory
        std::fs::create_dir_all(&checkpoint_dir)''', '''        let checkpoint_dir = self.checkpoint_path(&checkpoint_id, false)?;

        // Create a new validated direct child; never reuse an existing checkpoint.
        std::fs::create_dir(&checkpoint_dir)''', 'checkpoint creation')

change('''        let checkpoint_dir = self.checkpoint_base_dir.join(checkpoint_id);
        
        if !checkpoint_dir.exists() {
            return Err(format!("Checkpoint directory not found: {:?}", checkpoint_dir));
        }
        
        // Run CRIU restore command''', '''        let checkpoint_dir = self.checkpoint_path(checkpoint_id, true)?;
        // A unique pidfile prevents stale results from previous restores being reused.
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| format!("System clock error: {e}"))?.as_nanos();
        let pid_file = checkpoint_dir.join(format!("restore-{}-{nonce}.pid", std::process::id()));
        if std::fs::symlink_metadata(&pid_file).is_ok() {
            return Err("CRIU PID file already exists".to_string());
        }

        // Run CRIU restore command''', 'restore path')

change('''            .arg(&checkpoint_dir)
            .arg("-d") // Detach from terminal
            .output()''', '''            .arg(&checkpoint_dir)
            .arg("--pidfile")
            .arg(&pid_file)
            .arg("-d") // Detach from terminal
            .output()''', 'CRIU PID argument')

change('''        // Try to read PID from checkpoint directory
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
        Ok(0) // Placeholder - actual implementation would track restored PID''', '''        // A successful CRIU exit without a valid PID must never be reported as success.
        let pid = read_restore_pid(&pid_file)?;
        let _ = std::fs::remove_file(&pid_file);
        Ok(pid)''', 'restore PID')

change('''        if !self.checkpoint_base_dir.exists() {
            return checkpoints;
        }
        
        // Load from metadata file''', '''        match std::fs::symlink_metadata(&self.checkpoint_base_dir) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {},
            _ => return checkpoints,
        }

        // Load from metadata file''', 'listing checkpoint root')

change('''                    checkpoint.created_at = UNIX_EPOCH + std::time::Duration::from_secs(checkpoint.created_at_secs);
                    checkpoint.checkpoint_dir = self.checkpoint_base_dir.join(&checkpoint.checkpoint_id);
                    
                    // Filter out checkpoints that no longer exist
                    if checkpoint.checkpoint_dir.exists() {
                        checkpoints.push(checkpoint.clone());
                    }''', '''                    checkpoint.created_at = UNIX_EPOCH + std::time::Duration::from_secs(checkpoint.created_at_secs);
                    if let Ok(path) = self.checkpoint_path(&checkpoint.checkpoint_id, true) {
                        checkpoint.checkpoint_dir = path;
                        checkpoints.push(checkpoint.clone());
                    }''', 'untrusted listing metadata')

change('''                if path.is_dir() && path.file_name().and_then(|n| n.to_str()).map(|s| s.starts_with("checkpoint_")).unwrap_or(false) {
                    let checkpoint_id = path.file_name().unwrap().to_string_lossy().to_string();''', '''                // DirEntry::file_type does not follow symlinks.
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && path.file_name().and_then(|n| n.to_str())
                        .map(|s| s.starts_with("checkpoint_") && valid_checkpoint_id(s)).unwrap_or(false)
                {
                    let checkpoint_id = path.file_name().unwrap().to_string_lossy().to_string();
                    let path = match self.checkpoint_path(&checkpoint_id, true) {
                        Ok(path) => path,
                        Err(_) => continue,
                    };''', 'scan directory')

change('''    pub fn delete_checkpoint(&self, checkpoint_id: &str) -> Result<(), String> {
        let checkpoint_dir = self.checkpoint_base_dir.join(checkpoint_id);
        
        if !checkpoint_dir.exists() {
            return Err(format!("Checkpoint not found: {}", checkpoint_id));
        }''', '''    pub fn delete_checkpoint(&self, checkpoint_id: &str) -> Result<(), String> {
        let checkpoint_dir = self.checkpoint_path(checkpoint_id, true)?;''', 'bounded checkpoint deletion')

s += '''
#[cfg(test)]
mod checkpoint_security_tests {
    use super::*;

    fn disposable_manager() -> CriuManager {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let checkpoint_base_dir = std::env::temp_dir()
            .join(format!("procsentinel-criu-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&checkpoint_base_dir).unwrap();
        CriuManager { criu_path: None, available: false, checkpoint_base_dir }
    }

    #[test]
    fn rejects_traversal_absolute_and_invalid_names() {
        assert!(valid_checkpoint_id("checkpoint_123-abc"));
        for bad in ["", ".", "..", "../outside", "/tmp/outside", "a/b", "a\\\\b", "é", "x x"] {
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
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&outside, manager.checkpoint_base_dir.join("checkpoint_link")).unwrap();
            assert!(manager.delete_checkpoint("checkpoint_link").is_err());
            assert!(!manager.list_checkpoints().iter().any(|i| i.checkpoint_id == "checkpoint_link"));
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
    fn pidfile_requires_a_positive_pid_in_regular_file() {
        let manager = disposable_manager();
        let file = manager.checkpoint_base_dir.join("test.pid");
        for invalid in ["", "0", "nope", "4294967296"] {
            std::fs::write(&file, invalid).unwrap();
            assert!(read_restore_pid(&file).is_err());
        }
        std::fs::write(&file, "1234\\n").unwrap();
        assert_eq!(read_restore_pid(&file).unwrap(), 1234);
        std::fs::remove_dir_all(manager.checkpoint_base_dir).unwrap();
    }
}
'''
path.write_text(s)
print('Applied CRIU safety fix; unit tests are compiled with src/criu_manager.rs')
