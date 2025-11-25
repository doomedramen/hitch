use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Environment configuration as defined in hitch.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    /// Source branch used for rebuilding this environment (renamed from "source" to "base")
    pub base: String,

    /// List of branches promoted to this environment
    pub branches: Vec<String>,

    /// Whether the environment is currently locked
    pub locked: bool,

    /// Email of user who locked the environment (null if unlocked)
    pub locked_by: Option<String>,

    /// Timestamp when environment was locked (null if unlocked)
    pub locked_at: Option<DateTime<Utc>>,

    /// Timestamp when environment was last rebuilt (null if never rebuilt)
    pub rebuilt_at: Option<DateTime<Utc>>,

    /// Timestamp when environment was last released (null if never released)
    pub released_at: Option<DateTime<Utc>>,
}

impl Environment {
    pub fn new(base: String) -> Self {
        Self {
            base,
            branches: Vec::new(),
            locked: false,
            locked_by: None,
            locked_at: None,
            rebuilt_at: None,
            released_at: None,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn lock(&mut self, user_email: String) {
        self.locked = true;
        self.locked_by = Some(user_email);
        self.locked_at = Some(Utc::now());
    }

    pub fn unlock(&mut self) {
        self.locked = false;
        self.locked_by = None;
        self.locked_at = None;
    }

    pub fn update_rebuilt_timestamp(&mut self) {
        self.rebuilt_at = Some(Utc::now());
    }

    pub fn update_released_timestamp(&mut self) {
        self.released_at = Some(Utc::now());
    }

    pub fn add_branch(&mut self, branch: String) {
        if !self.branches.contains(&branch) {
            self.branches.push(branch);
        }
    }

    pub fn remove_branch(&mut self, branch: &str) {
        self.branches.retain(|b| b != branch);
    }

    #[allow(dead_code)]
    pub fn has_branch(&self, branch: &str) -> bool {
        self.branches.contains(&branch.to_string())
    }
}

/// Main Hitch configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitchConfig {
    /// Version of the configuration format
    pub version: String,

    /// Map of environment names to environment configurations
    pub environments: std::collections::HashMap<String, Environment>,
}

impl HitchConfig {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            environments: std::collections::HashMap::new(),
        }
    }

    pub fn add_environment(&mut self, name: String, environment: Environment) {
        self.environments.insert(name, environment);
    }

    pub fn remove_environment(&mut self, name: &str) {
        self.environments.remove(name);
    }

    pub fn get_environment(&self, name: &str) -> Option<&Environment> {
        self.environments.get(name)
    }

    pub fn get_environment_mut(&mut self, name: &str) -> Option<&mut Environment> {
        self.environments.get_mut(name)
    }

    #[allow(dead_code)]
    pub fn environment_exists(&self, name: &str) -> bool {
        self.environments.contains_key(name)
    }

    pub fn get_environment_names(&self) -> Vec<String> {
        self.environments.keys().cloned().collect()
    }
}

impl Default for HitchConfig {
    fn default() -> Self {
        Self::new()
    }
}
