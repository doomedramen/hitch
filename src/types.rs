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

    /// Validate the configuration for common issues
    /// Returns Ok(()) if valid, Err with description if invalid
    pub fn validate(&self) -> Result<(), String> {
        // Validate environment names
        for env_name in self.environments.keys() {
            if env_name.is_empty() {
                return Err("Environment name cannot be empty".to_string());
            }
            if env_name.contains(' ') {
                return Err(format!("Environment '{}' cannot contain spaces", env_name));
            }
            if env_name.to_lowercase() == "hitch-metadata" {
                return Err("Environment name 'hitch-metadata' is reserved".to_string());
            }
        }

        // Validate each environment
        for (env_name, env) in &self.environments {
            // Base branch validation
            if env.base.is_empty() {
                return Err(format!(
                    "Environment '{}' has an empty base branch",
                    env_name
                ));
            }

            // Check for circular dependencies
            if env.branches.contains(&env.base) {
                return Err(format!(
                    "Environment '{}' has its base branch '{}' in its promoted branches list",
                    env_name, env.base
                ));
            }

            // Validate branch names
            for branch in &env.branches {
                if branch.is_empty() {
                    return Err(format!(
                        "Environment '{}' has an empty branch in its list",
                        env_name
                    ));
                }
                if branch == env_name {
                    return Err(format!(
                        "Environment '{}' has itself in its promoted branches list",
                        env_name
                    ));
                }
            }

            // Check for duplicate branches in the same environment
            let mut seen_branches = std::collections::HashSet::new();
            for branch in &env.branches {
                if !seen_branches.insert(branch) {
                    return Err(format!(
                        "Environment '{}' has duplicate branch '{}' in its list",
                        env_name, branch
                    ));
                }
            }
        }

        // Cross-environment validation
        // Check for environments that share base branches (might be intentional, but warn)
        let mut base_branch_usage = std::collections::HashMap::new();
        for (env_name, env) in &self.environments {
            base_branch_usage
                .entry(env.base.clone())
                .or_insert_with(Vec::new)
                .push(env_name.clone());
        }

        // Note: Multiple environments can share base branches (this is allowed)
        // The base_branch_usage map could be used for validation or reporting if needed

        Ok(())
    }

    pub fn add_environment(
        &mut self,
        name: String,
        environment: Environment,
    ) -> Result<(), String> {
        // Validate the environment before adding
        let mut temp_config = self.clone();
        temp_config
            .environments
            .insert(name.clone(), environment.clone());
        temp_config.validate()?;

        // If validation passes, add to actual config
        self.environments.insert(name, environment);
        Ok(())
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

    pub fn get_environment_names(&self) -> Vec<String> {
        self.environments.keys().cloned().collect()
    }
}

impl Default for HitchConfig {
    fn default() -> Self {
        Self::new()
    }
}
