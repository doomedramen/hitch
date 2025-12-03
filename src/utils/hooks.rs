//! Hook system for extending Hitch functionality
//!
//! This module provides the infrastructure for plugins to hook into
//! Hitch's lifecycle events.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Types of hooks that can be registered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookType {
    /// Before a branch is promoted to an environment
    BeforePromote,
    /// After a branch is promoted to an environment
    AfterPromote,
    /// Before a branch is demoted from an environment
    BeforeDemote,
    /// After a branch is demoted from an environment
    AfterDemote,
    /// Before an environment is rebuilt
    BeforeRebuild,
    /// After an environment is rebuilt
    AfterRebuild,
    /// Before an environment is locked
    BeforeLock,
    /// After an environment is locked
    AfterLock,
    /// Before an environment is unlocked
    BeforeUnlock,
    /// After an environment is unlocked
    AfterUnlock,
    /// Before the guard is installed
    BeforeGuardInstall,
    /// After the guard is installed
    AfterGuardInstall,
}

impl std::fmt::Display for HookType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            HookType::BeforePromote => "before-promote",
            HookType::AfterPromote => "after-promote",
            HookType::BeforeDemote => "before-demote",
            HookType::AfterDemote => "after-demote",
            HookType::BeforeRebuild => "before-rebuild",
            HookType::AfterRebuild => "after-rebuild",
            HookType::BeforeLock => "before-lock",
            HookType::AfterLock => "after-lock",
            HookType::BeforeUnlock => "before-unlock",
            HookType::AfterUnlock => "after-unlock",
            HookType::BeforeGuardInstall => "before-guard-install",
            HookType::AfterGuardInstall => "after-guard-install",
        };
        write!(f, "{}", name)
    }
}

/// Context passed to hooks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    /// The command being executed
    pub command: String,
    /// The environment being operated on (if applicable)
    pub environment: Option<String>,
    /// The branch being operated on (if applicable)
    pub branch: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// User email who triggered the operation
    pub user_email: Option<String>,
    /// Timestamp when the operation started
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl HookContext {
    /// Create a new hook context
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            environment: None,
            branch: None,
            metadata: HashMap::new(),
            user_email: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Set the environment
    pub fn with_environment(mut self, env: impl Into<String>) -> Self {
        self.environment = Some(env.into());
        self
    }

    /// Set the branch
    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Add metadata
    pub fn with_metadata<K: Into<String>, V: Into<serde_json::Value>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the user email
    pub fn with_user_email(mut self, email: impl Into<String>) -> Self {
        self.user_email = Some(email.into());
        self
    }
}

/// Result of hook execution
#[derive(Debug, Clone)]
pub enum HookResult {
    /// Continue with the operation
    Continue,
    /// Stop the operation with an error
    Stop(String),
    /// Skip the operation with a message
    Skip(String),
    /// Modify the context (for validation/modification hooks)
    Modify(HookContext),
}

/// Trait for implementing Hitch plugins
pub trait HitchPlugin: Send + Sync {
    /// Get the plugin name
    fn name(&self) -> &str;

    /// Get the plugin version
    fn version(&self) -> &str;

    /// Get the hooks this plugin registers for
    fn registered_hooks(&self) -> Vec<HookType>;

    /// Execute the plugin for a given hook
    fn execute(&self, hook_type: HookType, context: &HookContext) -> HookResult;
}

/// Registry for managing plugins
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn HitchPlugin>>,
    hook_map: HashMap<HookType, Vec<Arc<dyn HitchPlugin>>>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            hook_map: HashMap::new(),
        }
    }

    /// Register a plugin
    pub fn register<P: HitchPlugin + 'static>(&mut self, plugin: P) {
        let plugin = Arc::new(plugin);
        let hooks = plugin.registered_hooks();

        for hook_type in hooks {
            self.hook_map
                .entry(hook_type)
                .or_default()
                .push(plugin.clone());
        }

        self.plugins.push(plugin);
    }

    /// Execute all plugins registered for a hook type
    pub fn execute_hooks(&self, hook_type: HookType, context: &HookContext) -> Vec<HookResult> {
        let mut results = Vec::new();

        if let Some(plugins) = self.hook_map.get(&hook_type) {
            for plugin in plugins {
                let result = plugin.execute(hook_type, context);
                results.push(result);
            }
        }

        results
    }

    /// Get all registered plugins
    pub fn plugins(&self) -> &[Arc<dyn HitchPlugin>] {
        &self.plugins
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        name: String,
        hooks: Vec<HookType>,
    }

    impl TestPlugin {
        fn new(name: &str, hooks: Vec<HookType>) -> Self {
            Self {
                name: name.to_string(),
                hooks,
            }
        }
    }

    impl HitchPlugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        fn registered_hooks(&self) -> Vec<HookType> {
            self.hooks.clone()
        }

        fn execute(&self, _hook_type: HookType, _context: &HookContext) -> HookResult {
            HookResult::Continue
        }
    }

    #[test]
    fn test_plugin_registry() {
        let mut registry = PluginRegistry::new();

        // Register plugins
        registry.register(TestPlugin::new(
            "test-plugin-1",
            vec![HookType::BeforePromote, HookType::AfterPromote],
        ));
        registry.register(TestPlugin::new(
            "test-plugin-2",
            vec![HookType::BeforePromote],
        ));

        // Check plugins
        assert_eq!(registry.plugins().len(), 2);

        // Check hook mapping
        let context = HookContext::new("test");

        let before_results = registry.execute_hooks(HookType::BeforePromote, &context);
        assert_eq!(before_results.len(), 2); // Both plugins registered

        let after_results = registry.execute_hooks(HookType::AfterPromote, &context);
        assert_eq!(after_results.len(), 1); // Only first plugin registered

        let lock_results = registry.execute_hooks(HookType::BeforeLock, &context);
        assert_eq!(lock_results.len(), 0); // No plugins registered
    }
}
