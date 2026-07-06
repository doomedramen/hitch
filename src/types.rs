use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

    /// Whether this environment requires approval for changes
    #[serde(default)]
    pub requires_approval: bool,

    /// List of email addresses authorized to approve changes
    #[serde(default)]
    pub approvers: Vec<String>,

    /// Minimum number of approvals required
    #[serde(default = "default_min_approvals")]
    pub min_approvals: usize,
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
            requires_approval: false,
            approvers: Vec::new(),
            min_approvals: 1,
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

    /// Check if approval is required for this environment
    pub fn requires_approval_check(&self) -> bool {
        self.requires_approval
    }

    /// Check if a user is authorized to approve changes to this environment
    pub fn is_approver(&self, email: &str) -> bool {
        self.approvers.contains(&email.to_string())
    }

    /// Validate the approval configuration for this environment
    pub fn validate_approval_config(&self) -> Result<(), String> {
        if self.requires_approval {
            if self.approvers.is_empty() {
                return Err(
                    "Environment requires approval but no approvers are specified".to_string(),
                );
            }
            if self.min_approvals == 0 {
                return Err("Minimum approvals must be at least 1".to_string());
            }
            if self.min_approvals > self.approvers.len() {
                return Err(format!(
                    "Minimum approvals ({}) cannot be greater than number of approvers ({})",
                    self.min_approvals,
                    self.approvers.len()
                ));
            }
            // Validate email format
            for approver in &self.approvers {
                if !approver.contains('@') || !approver.contains('.') {
                    return Err(format!("Invalid email format for approver: {}", approver));
                }
            }
        }
        Ok(())
    }
}

/// Default function for min_approvals field
fn default_min_approvals() -> usize {
    1
}

/// Types of operations that can be rolled back
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollbackOperation {
    Promote,
    Demote,
}

/// Information needed to rollback a failed operation
#[derive(Debug, Clone)]
pub struct RollbackInfo {
    /// The type of operation being performed
    pub operation: RollbackOperation,
    /// The target environment name
    pub env_name: String,
    /// The branch being promoted/demoted
    pub branch: String,
    /// The environment state before the operation (single-environment fallback)
    pub previous_state: Option<Environment>,
    /// The FULL configuration before the operation. Rollback restores this whole
    /// snapshot so that changes beyond the target environment (e.g. an approval
    /// request appended to `approval_requests`, or edits to other environments)
    /// are also reverted, not just the one `Environment`.
    pub previous_config: Option<HitchConfig>,
    /// When the operation was started
    #[allow(dead_code)]
    pub timestamp: DateTime<Utc>,
}

impl RollbackInfo {
    pub fn new(operation: RollbackOperation, env_name: String, branch: String) -> Self {
        Self {
            operation,
            env_name,
            branch,
            previous_state: None,
            previous_config: None,
            timestamp: Utc::now(),
        }
    }
}

/// Type of operation that requires approval
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Operation {
    Promote,
    Demote,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operation::Promote => write!(f, "Promote"),
            Operation::Demote => write!(f, "Demote"),
        }
    }
}

/// Status of an approval request
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Applied,
    Rejected,
    Cancelled,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalStatus::Pending => write!(f, "Pending"),
            ApprovalStatus::Approved => write!(f, "Approved"),
            ApprovalStatus::Applied => write!(f, "Applied"),
            ApprovalStatus::Rejected => write!(f, "Rejected"),
            ApprovalStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Individual approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    /// Email of user who gave approval
    pub approved_by: String,
    /// When approval was given
    pub approved_at: DateTime<Utc>,
    /// Optional approval comment
    pub comment: Option<String>,
}

/// Rejection record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    /// Email of user who rejected
    pub rejected_by: String,
    /// When rejection occurred
    pub rejected_at: DateTime<Utc>,
    /// Required reason for rejection
    pub reason: String,
}

/// Snapshot of branch states at request time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildSnapshot {
    /// Name of the base branch
    pub base_branch: String,
    /// Commit SHA of base branch
    pub base_sha: String,
    /// Map of branch name to commit SHA
    pub branch_shas: std::collections::HashMap<String, String>,
    /// Whether merge conflicts were detected at request time
    pub merge_conflicts: bool,
}

/// Approval request for environment change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique identifier for this request
    pub id: String,
    /// Target environment name
    pub environment: String,
    /// Branch being promoted/demoted
    pub branch: String,
    /// Type of operation
    pub operation: Operation,
    /// Email of user who requested the change
    pub requested_by: String,
    /// When request was created
    pub requested_at: DateTime<Utc>,
    /// Current status of request
    pub status: ApprovalStatus,
    /// List of approvals received
    pub approvals: Vec<Approval>,
    /// Rejection details (if rejected)
    pub rejection: Option<Rejection>,
    /// Snapshot of branch states at request time
    pub rebuild_snapshot: RebuildSnapshot,
    /// The approval threshold frozen at request-creation time. Evaluating the
    /// threshold against this (rather than the environment's live `min_approvals`)
    /// prevents a later `hitch set --min-approvals` from retroactively satisfying
    /// or invalidating an in-flight request. `None` for requests created before
    /// this field existed, in which case the environment's current value is used.
    #[serde(default)]
    pub snapshot_min_approvals: Option<usize>,
}

impl ApprovalRequest {
    /// Create a new approval request
    pub fn new(
        environment: String,
        branch: String,
        operation: Operation,
        requested_by: String,
        rebuild_snapshot: RebuildSnapshot,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            environment,
            branch,
            operation,
            requested_by,
            requested_at: Utc::now(),
            status: ApprovalStatus::Pending,
            approvals: Vec::new(),
            rejection: None,
            rebuild_snapshot,
            snapshot_min_approvals: None,
        }
    }

    /// The approval threshold governing this request.
    ///
    /// Uses the value frozen at creation time when present, falling back to the
    /// environment's current `min_approvals` for older requests. This is what
    /// keeps threshold evaluation stable across `hitch set --min-approvals`.
    pub fn required_approvals(&self, environment: &Environment) -> usize {
        self.snapshot_min_approvals
            .unwrap_or(environment.min_approvals)
    }

    /// Check if user has already approved this request
    pub fn has_approved(&self, email: &str) -> bool {
        self.approvals.iter().any(|a| a.approved_by == email)
    }

    /// Get the current number of approvals
    pub fn approval_count(&self) -> usize {
        self.approvals.len()
    }

    /// Add an approval to this request
    pub fn add_approval(&mut self, approved_by: String, comment: Option<String>) {
        self.approvals.push(Approval {
            approved_by,
            approved_at: Utc::now(),
            comment,
        });
    }

    /// Check if approval threshold is met
    pub fn threshold_met(&self, min_approvals: usize) -> bool {
        self.approval_count() >= min_approvals
    }

    /// Mark request as approved (threshold met)
    pub fn mark_approved(&mut self) {
        self.status = ApprovalStatus::Approved;
    }

    /// Mark request as applied (operation executed)
    pub fn mark_applied(&mut self) {
        self.status = ApprovalStatus::Applied;
    }

    /// Reject the request
    pub fn reject(&mut self, rejected_by: String, reason: String) {
        self.status = ApprovalStatus::Rejected;
        self.rejection = Some(Rejection {
            rejected_by,
            rejected_at: Utc::now(),
            reason,
        });
    }

    /// Cancel the request
    pub fn cancel(&mut self) {
        self.status = ApprovalStatus::Cancelled;
    }
}

/// The maximum config schema MAJOR version this binary knows how to safely
/// rewrite. Reading a newer config is tolerated, but rewriting one is refused
/// (see [`HitchConfig::check_write_compatibility`]) because serde silently drops
/// fields this binary doesn't model, which would corrupt a newer config on save.
pub const SUPPORTED_CONFIG_MAJOR: u64 = 1;

/// Main Hitch configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitchConfig {
    /// Version of the configuration format
    pub version: String,

    /// Map of environment names to environment configurations
    pub environments: std::collections::HashMap<String, Environment>,

    /// List of approval requests
    #[serde(default)]
    pub approval_requests: Vec<ApprovalRequest>,
}

impl HitchConfig {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            environments: std::collections::HashMap::new(),
            approval_requests: Vec::new(),
        }
    }

    /// Parse the MAJOR component of `version` (e.g. "1.0" -> 1, "2.3" -> 2).
    /// Returns `None` if the version string is missing or unparseable.
    fn major_version(&self) -> Option<u64> {
        self.version
            .split('.')
            .next()
            .and_then(|s| s.trim().parse().ok())
    }

    /// Refuse to rewrite a config that was written by a newer schema than this
    /// binary supports. Because the config is deserialized without capturing
    /// unknown fields, serializing it back would silently drop any configuration
    /// this (older) binary doesn't understand — so we block the write instead.
    ///
    /// A missing/unparseable version is treated as compatible (legacy/hand-edited
    /// configs) to avoid spurious blocks.
    pub fn check_write_compatibility(&self) -> Result<(), String> {
        match self.major_version() {
            Some(major) if major > SUPPORTED_CONFIG_MAJOR => Err(format!(
                "hitch.json is schema version '{}', which is newer than this hitch binary \
                 supports (up to major version {}). Rewriting it could drop configuration this \
                 version does not understand. Please upgrade hitch before modifying it \
                 (e.g. `cargo install hitch` or `brew upgrade hitch`).",
                self.version, SUPPORTED_CONFIG_MAJOR
            )),
            _ => Ok(()),
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

            // Validate approval configuration
            if let Err(e) = env.validate_approval_config() {
                return Err(format!(
                    "Environment '{}' approval configuration invalid: {}",
                    env_name, e
                ));
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

    // Approval request management methods

    /// Add an approval request
    pub fn add_approval_request(&mut self, request: ApprovalRequest) {
        self.approval_requests.push(request);
    }

    /// Find an approval request by ID
    pub fn get_approval_request(&self, id: &str) -> Option<&ApprovalRequest> {
        self.approval_requests.iter().find(|r| r.id == id)
    }

    /// Find an approval request by ID (mutable)
    pub fn get_approval_request_mut(&mut self, id: &str) -> Option<&mut ApprovalRequest> {
        self.approval_requests.iter_mut().find(|r| r.id == id)
    }

    /// Get all approval requests
    pub fn get_approval_requests(&self) -> &[ApprovalRequest] {
        &self.approval_requests
    }

    /// Get approval requests for a specific environment
    pub fn get_approval_requests_for_env(&self, env_name: &str) -> Vec<&ApprovalRequest> {
        self.approval_requests
            .iter()
            .filter(|r| r.environment == env_name)
            .collect()
    }
}

impl Default for HitchConfig {
    fn default() -> Self {
        Self::new()
    }
}
