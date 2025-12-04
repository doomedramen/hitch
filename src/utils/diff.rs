//! Diff visualization utilities for Hitch
//!
//! Provides colored diff output for visualizing changes between environments,
//! branch promotions, and configuration updates.

use crate::types::{Environment, HitchConfig};
use colored::*;
use serde_json;
use std::collections::{HashMap, HashSet};

/// Represents a change in the configuration
#[derive(Debug, Clone, serde::Serialize)]
pub enum ConfigChange {
    /// Environment added
    EnvironmentAdded { name: String, env: Environment },
    /// Environment removed
    EnvironmentRemoved { name: String },
    /// Environment modified
    EnvironmentModified {
        name: String,
        field: String,
        old_value: String,
        new_value: String,
    },
    /// Branch added to environment
    BranchAdded { env: String, branch: String },
    /// Branch removed from environment
    BranchRemoved { env: String, branch: String },
}

/// Generates a visual diff between two configurations
pub fn diff_configs(old_config: &HitchConfig, new_config: &HitchConfig) -> Vec<ConfigChange> {
    let mut changes = Vec::new();

    // Find added environments
    for (name, env) in &new_config.environments {
        if !old_config.environments.contains_key(name) {
            changes.push(ConfigChange::EnvironmentAdded {
                name: name.clone(),
                env: env.clone(),
            });
        }
    }

    // Find removed environments
    for name in old_config.environments.keys() {
        if !new_config.environments.contains_key(name) {
            changes.push(ConfigChange::EnvironmentRemoved { name: name.clone() });
        }
    }

    // Find modified environments
    for (name, new_env) in &new_config.environments {
        if let Some(old_env) = old_config.environments.get(name) {
            // Check base branch
            if old_env.base != new_env.base {
                changes.push(ConfigChange::EnvironmentModified {
                    name: name.clone(),
                    field: "base".to_string(),
                    old_value: old_env.base.clone(),
                    new_value: new_env.base.clone(),
                });
            }

            // Check added branches
            let old_branches: HashSet<_> = old_env.branches.iter().collect();
            let new_branches: HashSet<_> = new_env.branches.iter().collect();

            for branch in new_branches.difference(&old_branches) {
                changes.push(ConfigChange::BranchAdded {
                    env: name.clone(),
                    branch: branch.to_string(),
                });
            }

            // Check removed branches
            for branch in old_branches.difference(&new_branches) {
                changes.push(ConfigChange::BranchRemoved {
                    env: name.clone(),
                    branch: branch.to_string(),
                });
            }

            // Check lock status
            if old_env.is_locked() != new_env.is_locked() {
                if new_env.is_locked() {
                    changes.push(ConfigChange::EnvironmentModified {
                        name: name.clone(),
                        field: "locked".to_string(),
                        old_value: "unlocked".to_string(),
                        new_value: format!(
                            "locked by {}",
                            new_env.locked_by.as_ref().unwrap_or(&"unknown".to_string())
                        ),
                    });
                } else {
                    changes.push(ConfigChange::EnvironmentModified {
                        name: name.clone(),
                        field: "locked".to_string(),
                        old_value: format!(
                            "locked by {}",
                            old_env.locked_by.as_ref().unwrap_or(&"unknown".to_string())
                        ),
                        new_value: "unlocked".to_string(),
                    });
                }
            }
        }
    }

    changes
}

/// Formats changes as a colored diff output
pub fn format_diff(changes: &[ConfigChange]) -> String {
    let mut output = String::new();

    // Group changes by type
    let mut additions = Vec::new();
    let mut removals = Vec::new();
    let mut modifications = Vec::new();

    for change in changes {
        match change {
            ConfigChange::EnvironmentAdded { .. } | ConfigChange::BranchAdded { .. } => {
                additions.push(change);
            }
            ConfigChange::EnvironmentRemoved { .. } | ConfigChange::BranchRemoved { .. } => {
                removals.push(change);
            }
            ConfigChange::EnvironmentModified { .. } => {
                modifications.push(change);
            }
        }
    }

    // Print additions
    if !additions.is_empty() {
        output.push_str(&format!(
            "\n{} {}\n",
            "✓".green(),
            "Additions:".green().bold()
        ));
        for change in &additions {
            match change {
                ConfigChange::EnvironmentAdded { name, env } => {
                    output.push_str(&format!(
                        "  {} Environment '{}' with base branch '{}'\n",
                        "+".green(),
                        name.cyan(),
                        env.base.yellow()
                    ));
                }
                ConfigChange::BranchAdded { env, branch } => {
                    output.push_str(&format!(
                        "  {} Branch '{}' to environment '{}'\n",
                        "+".green(),
                        branch.cyan(),
                        env.yellow()
                    ));
                }
                _ => {}
            }
        }
    }

    // Print removals
    if !removals.is_empty() {
        output.push_str(&format!("\n{} {}\n", "✗".red(), "Removals:".red().bold()));
        for change in &removals {
            match change {
                ConfigChange::EnvironmentRemoved { name } => {
                    output.push_str(&format!("  {} Environment '{}'\n", "-".red(), name.cyan()));
                }
                ConfigChange::BranchRemoved { env, branch } => {
                    output.push_str(&format!(
                        "  {} Branch '{}' from environment '{}'\n",
                        "-".red(),
                        branch.cyan(),
                        env.yellow()
                    ));
                }
                _ => {}
            }
        }
    }

    // Print modifications
    if !modifications.is_empty() {
        output.push_str(&format!(
            "\n{} {}\n",
            "~".yellow(),
            "Modifications:".yellow().bold()
        ));
        for change in &modifications {
            if let ConfigChange::EnvironmentModified {
                name,
                field,
                old_value,
                new_value,
            } = change
            {
                output.push_str(&format!(
                    "  {} {}.{}: {} → {}\n",
                    "~".yellow(),
                    name.cyan(),
                    field.yellow(),
                    old_value.red(),
                    new_value.green()
                ));
            }
        }
    }

    if changes.is_empty() {
        output.push_str(&format!("\n{} No changes detected\n", "✓".green()));
    }

    output
}

/// Creates a summary of changes
pub fn create_summary(changes: &[ConfigChange]) -> String {
    let mut counts = HashMap::new();

    for change in changes {
        let key = match change {
            ConfigChange::EnvironmentAdded { .. } => "Environments added",
            ConfigChange::EnvironmentRemoved { .. } => "Environments removed",
            ConfigChange::EnvironmentModified { .. } => "Environments modified",
            ConfigChange::BranchAdded { .. } => "Branches added",
            ConfigChange::BranchRemoved { .. } => "Branches removed",
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    if counts.is_empty() {
        "No changes".to_string()
    } else {
        counts
            .into_iter()
            .map(|(k, v)| format!("{} {}", v, k))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Generates a JSON diff for API responses
#[allow(dead_code)]
pub fn json_diff(old_config: &HitchConfig, new_config: &HitchConfig) -> serde_json::Value {
    let changes = diff_configs(old_config, new_config);
    serde_json::to_value(&changes).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_configs() {
        let mut old_config = HitchConfig::new();
        let mut new_config = HitchConfig::new();

        // Add an environment to old config
        let old_env = Environment::new("main".to_string());
        old_config
            .add_environment("dev".to_string(), old_env)
            .unwrap();

        // Add and modify in new config
        let mut new_env = Environment::new("develop".to_string());
        new_env.add_branch("feature-1".to_string());
        new_config
            .add_environment("dev".to_string(), new_env)
            .unwrap();
        new_config
            .add_environment("staging".to_string(), Environment::new("main".to_string()))
            .unwrap();

        let changes = diff_configs(&old_config, &new_config);
        assert_eq!(changes.len(), 3); // base change, branch added, staging added
    }

    #[test]
    fn test_format_diff() {
        let changes = vec![
            ConfigChange::EnvironmentAdded {
                name: "staging".to_string(),
                env: Environment::new("main".to_string()),
            },
            ConfigChange::BranchAdded {
                env: "dev".to_string(),
                branch: "feature-1".to_string(),
            },
        ];

        let output = format_diff(&changes);
        assert!(output.contains("Additions"));
        assert!(output.contains("staging"));
        assert!(output.contains("feature-1"));
    }

    #[test]
    fn test_empty_configs_diff() {
        let old = HitchConfig::default();
        let new = HitchConfig::default();
        let changes = diff_configs(&old, &new);
        assert!(
            changes.is_empty(),
            "Empty configs should have no differences"
        );
    }

    #[test]
    fn test_identical_configs_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add same environment to both
        let env = Environment::new("main".to_string());
        old.add_environment("test".to_string(), env.clone())
            .unwrap();
        new.add_environment("test".to_string(), env).unwrap();

        let changes = diff_configs(&old, &new);
        assert!(
            changes.is_empty(),
            "Identical configs should have no differences"
        );
    }

    #[test]
    fn test_lock_status_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add environment to both
        let env = Environment::new("main".to_string());
        old.add_environment("test".to_string(), env.clone())
            .unwrap();
        new.add_environment("test".to_string(), env).unwrap();

        // Lock in new config
        new.environments
            .get_mut("test")
            .unwrap()
            .lock("user".to_string());

        let changes = diff_configs(&old, &new);
        assert!(!changes.is_empty());

        // Find the lock status change
        let lock_change = changes.iter().find(|c| {
            matches!(c,
                ConfigChange::EnvironmentModified { name, field, .. }
                if name == "test" && field == "locked"
            )
        });
        assert!(lock_change.is_some(), "Should detect lock status change");
    }

    #[test]
    fn test_unlock_status_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add and lock environment in old config
        let env = Environment::new("main".to_string());
        old.add_environment("test".to_string(), env.clone())
            .unwrap();
        old.environments
            .get_mut("test")
            .unwrap()
            .lock("user".to_string());

        // Add same environment to new but keep unlocked
        new.add_environment("test".to_string(), env).unwrap();

        let changes = diff_configs(&old, &new);
        assert!(!changes.is_empty());

        // Find the unlock status change
        let unlock_change = changes.iter().find(|c| {
            matches!(c,
                ConfigChange::EnvironmentModified { name, field, .. }
                if name == "test" && field == "locked"
            )
        });
        assert!(
            unlock_change.is_some(),
            "Should detect unlock status change"
        );
    }

    #[test]
    fn test_branch_add_remove_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Set up environment with branches in old
        let mut old_env = Environment::new("main".to_string());
        old_env.add_branch("feature-a".to_string());
        old_env.add_branch("feature-b".to_string());
        old.add_environment("dev".to_string(), old_env).unwrap();

        // Set up environment with different branches in new
        let mut new_env = Environment::new("main".to_string());
        new_env.add_branch("feature-a".to_string()); // Keep this
        new_env.add_branch("feature-c".to_string()); // New branch
        new.add_environment("dev".to_string(), new_env).unwrap();

        let changes = diff_configs(&old, &new);

        // Should detect branch removal and addition
        let branch_removed = changes.iter().any(|c| {
            matches!(c,
                ConfigChange::BranchRemoved { env, branch }
                if env == "dev" && branch == "feature-b"
            )
        });
        assert!(branch_removed, "Should detect removed branch");

        let branch_added = changes.iter().any(|c| {
            matches!(c,
                ConfigChange::BranchAdded { env, branch }
                if env == "dev" && branch == "feature-c"
            )
        });
        assert!(branch_added, "Should detect added branch");
    }

    #[test]
    fn test_environment_add_remove_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add environment to old only
        old.add_environment("dev".to_string(), Environment::new("develop".to_string()))
            .unwrap();

        // Add different environment to new only
        new.add_environment("staging".to_string(), Environment::new("main".to_string()))
            .unwrap();

        let changes = diff_configs(&old, &new);
        assert_eq!(changes.len(), 2);

        // Check for removal
        let removed = changes.iter().any(|c| {
            matches!(c,
                ConfigChange::EnvironmentRemoved { name }
                if name == "dev"
            )
        });
        assert!(removed, "Should detect removed environment");

        // Check for addition
        let added = changes.iter().any(|c| {
            matches!(c,
                ConfigChange::EnvironmentAdded { name, .. }
                if name == "staging"
            )
        });
        assert!(added, "Should detect added environment");
    }

    #[test]
    fn test_base_branch_change_diff() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add environment with one base branch
        old.add_environment("dev".to_string(), Environment::new("develop".to_string()))
            .unwrap();

        // Add same environment with different base branch
        new.add_environment("dev".to_string(), Environment::new("main".to_string()))
            .unwrap();

        let changes = diff_configs(&old, &new);
        assert_eq!(changes.len(), 1);

        // Check for base branch change
        match &changes[0] {
            ConfigChange::EnvironmentModified {
                name,
                field,
                old_value,
                new_value,
            } => {
                assert_eq!(name, "dev");
                assert_eq!(field, "base");
                assert_eq!(old_value, "develop");
                assert_eq!(new_value, "main");
            }
            _ => panic!("Expected EnvironmentModified change"),
        }
    }

    #[test]
    fn test_create_summary_with_mixed_changes() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Create a mix of changes
        old.add_environment("dev".to_string(), Environment::new("develop".to_string()))
            .unwrap();

        let mut staging_env = Environment::new("main".to_string());
        staging_env.add_branch("v1.0".to_string());
        new.add_environment("staging".to_string(), staging_env)
            .unwrap();

        let changes = diff_configs(&old, &new);
        let summary = create_summary(&changes);

        assert!(summary.contains("1 Environments added"));
        assert!(summary.contains("1 Environments removed"));
        // Summary doesn't include environment names, just counts
    }

    #[test]
    fn test_json_diff_output() {
        let old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Add environment to new config
        new.add_environment("prod".to_string(), Environment::new("main".to_string()))
            .unwrap();

        let json = json_diff(&old, &new);

        // Should be valid JSON
        assert!(json.is_array());

        // Should have the change
        if let Some(array) = json.as_array() {
            assert_eq!(array.len(), 1);
            if let Some(change) = array[0].as_object() {
                // Check for the EnvironmentAdded variant with name field
                assert!(change.contains_key("EnvironmentAdded"));
                if let Some(env_added) = change.get("EnvironmentAdded") {
                    assert!(env_added.is_object());
                    if let Some(env_obj) = env_added.as_object() {
                        assert_eq!(
                            env_obj.get("name"),
                            Some(&serde_json::Value::String("prod".to_string()))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_format_diff_with_all_change_types() {
        let changes = vec![
            ConfigChange::EnvironmentAdded {
                name: "staging".to_string(),
                env: Environment::new("main".to_string()),
            },
            ConfigChange::EnvironmentRemoved {
                name: "dev".to_string(),
            },
            ConfigChange::EnvironmentModified {
                name: "prod".to_string(),
                field: "base".to_string(),
                old_value: "main".to_string(),
                new_value: "release".to_string(),
            },
            ConfigChange::BranchAdded {
                env: "staging".to_string(),
                branch: "feature-1".to_string(),
            },
            ConfigChange::BranchRemoved {
                env: "prod".to_string(),
                branch: "old-feature".to_string(),
            },
        ];

        let output = format_diff(&changes);

        // Check all sections are present
        assert!(output.contains("Additions"));
        assert!(output.contains("Removals"));
        assert!(output.contains("Modifications"));

        // Check specific changes
        assert!(output.contains("staging"));
        assert!(output.contains("dev"));
        assert!(output.contains("prod"));
        assert!(output.contains("feature-1"));
        assert!(output.contains("old-feature"));
    }

    #[test]
    fn test_create_summary_empty_changes() {
        let changes = vec![];
        let summary = create_summary(&changes);
        assert!(summary.trim().is_empty() || summary.contains("No changes"));
    }

    #[test]
    fn test_multiple_lock_changes() {
        let mut old = HitchConfig::default();
        let mut new = HitchConfig::default();

        // Set up multiple environments
        old.add_environment("dev".to_string(), Environment::new("main".to_string()))
            .unwrap();
        old.add_environment("staging".to_string(), Environment::new("main".to_string()))
            .unwrap();

        // Lock one and unlock the other in new
        new.add_environment("dev".to_string(), Environment::new("main".to_string()))
            .unwrap();
        new.add_environment("staging".to_string(), Environment::new("main".to_string()))
            .unwrap();
        new.environments
            .get_mut("dev")
            .unwrap()
            .lock("user1".to_string());
        // staging stays unlocked (simulating unlock if it was locked before)

        let changes = diff_configs(&old, &new);

        // Should detect the lock change
        let lock_changes = changes
            .iter()
            .filter(|c| {
                matches!(c,
                    ConfigChange::EnvironmentModified { field, .. }
                    if field == "locked"
                )
            })
            .count();

        assert!(
            lock_changes >= 1,
            "Should detect at least one lock status change"
        );
    }
}
