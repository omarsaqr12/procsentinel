from pathlib import Path
p = Path('src/criu_manager.rs')
s = p.read_text()
old = '''impl Default for CheckpointInfo {
    fn default() -> Self {
'''
new = '''#[derive(Debug, Deserialize)]
struct CheckpointMetadataFile {
    checkpoints: Vec<CheckpointInfo>,
}

impl Default for CheckpointInfo {
    fn default() -> Self {
'''
if s.count(old) != 1:
    raise SystemExit(f'expected one CheckpointInfo Default anchor, found {s.count(old)}')
s = s.replace(old, new, 1)
old = '''            if let Ok(mut metadata_list) = toml::from_str::<Vec<CheckpointInfo>>(&content) {
                // Restore SystemTime and PathBuf from serialized data
                for checkpoint in &mut metadata_list {
'''
new = '''            if let Ok(file) = toml::from_str::<CheckpointMetadataFile>(&content) {
                let mut metadata_list = file.checkpoints;
                // Restore SystemTime and PathBuf from serialized data
                for checkpoint in &mut metadata_list {
'''
if s.count(old) != 1:
    raise SystemExit(f'expected one metadata read anchor, found {s.count(old)}')
s = s.replace(old, new, 1)
old = '''        let content = toml::to_string_pretty(checkpoints)
            .map_err(|e| format!("Failed to serialize checkpoints: {}", e))?;
'''
new = '''        #[derive(Serialize)]
        struct CheckpointMetadata<'a> {
            checkpoints: &'a [CheckpointInfo],
        }
        let content = toml::to_string_pretty(&CheckpointMetadata { checkpoints })
            .map_err(|e| format!("Failed to serialize checkpoints: {}", e))?;
'''
if s.count(old) != 1:
    raise SystemExit(f'expected one metadata write anchor, found {s.count(old)}')
s = s.replace(old, new, 1)
p.write_text(s)
