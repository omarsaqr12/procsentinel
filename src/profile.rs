//! Focus Mode Profiles for prioritizing and hiding processes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub prioritize_processes: Vec<String>, // Process name patterns
    pub hide_processes: Vec<String>,        // Process name patterns to hide
    pub nice_adjustments: HashMap<String, i32>, // Process name -> nice value
}

impl Profile {
    pub fn new(name: String) -> Self {
        Self {
            name,
            prioritize_processes: Vec::new(),
            hide_processes: Vec::new(),
            nice_adjustments: HashMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileConfig {
    profiles: Vec<Profile>,
}

pub struct ProfileManager {
    profiles: Vec<Profile>,
    active_profile: Option<String>,
    config_path: PathBuf,
}

impl ProfileManager {
    pub fn new() -> Self {
        let config_dir = dirs::home_dir()
            .map(|mut p| {
                p.push(".lpm");
                p
            })
            .unwrap_or_else(|| PathBuf::from("."));
        
        let config_path = config_dir.join("profiles.toml");
        
        let mut manager = Self {
            profiles: Vec::new(),
            active_profile: None,
            config_path,
        };
        
        // Load profiles from file
        let _ = manager.load_profiles();
        
        manager
    }

    pub fn get_profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn get_profile_mut(&mut self, name: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    pub fn add_profile(&mut self, profile: Profile) {
        // Remove existing profile with same name
        self.profiles.retain(|p| p.name != profile.name);
        self.profiles.push(profile);
        let _ = self.save_profiles();
    }

    pub fn remove_profile(&mut self, name: &str) -> bool {
        let len_before = self.profiles.len();
        self.profiles.retain(|p| p.name != name);
        let removed = self.profiles.len() < len_before;
        if removed {
            if self.active_profile.as_ref() == Some(&name.to_string()) {
                self.active_profile = None;
            }
            let _ = self.save_profiles();
        }
        removed
    }

    pub fn set_active_profile(&mut self, name: Option<String>) {
        self.active_profile = name;
    }

    pub fn get_active_profile(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    pub fn is_process_prioritized(&self, process_name: &str) -> bool {
        if let Some(profile_name) = &self.active_profile {
            if let Some(profile) = self.get_profile(profile_name) {
                return profile.prioritize_processes.iter()
                    .any(|pattern| process_name.contains(pattern) || 
                         pattern == "*" || 
                         process_name.matches(pattern).next().is_some());
            }
        }
        false
    }

    pub fn should_hide_process(&self, process_name: &str) -> bool {
        if let Some(profile_name) = &self.active_profile {
            if let Some(profile) = self.get_profile(profile_name) {
                return profile.hide_processes.iter()
                    .any(|pattern| process_name.contains(pattern) || 
                         pattern == "*" || 
                         process_name.matches(pattern).next().is_some());
            }
        }
        false
    }

    pub fn get_nice_adjustment(&self, process_name: &str) -> Option<i32> {
        if let Some(profile_name) = &self.active_profile {
            if let Some(profile) = self.get_profile(profile_name) {
                // Check exact match first
                if let Some(&nice) = profile.nice_adjustments.get(process_name) {
                    return Some(nice);
                }
                // Check pattern matches
                for (pattern, &nice) in &profile.nice_adjustments {
                    if process_name.contains(pattern) || pattern == "*" {
                        return Some(nice);
                    }
                }
            }
        }
        None
    }

    fn load_profiles(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.config_path.exists() {
            return Ok(()); // No config file yet
        }

        let content = fs::read_to_string(&self.config_path)?;
        let config: ProfileConfig = toml::from_str(&content)?;
        self.profiles = config.profiles;
        Ok(())
    }

    fn save_profiles(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create config directory if it doesn't exist
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let config = ProfileConfig {
            profiles: self.profiles.clone(),
        };

        let content = toml::to_string_pretty(&config)?;
        fs::write(&self.config_path, content)?;
        Ok(())
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self::new()
    }
}

