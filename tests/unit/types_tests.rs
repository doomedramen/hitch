//! Unit tests for Hitch core types
//!
//! Provides comprehensive testing for Environment and HitchConfig structures
//! with 25+ granular test cases covering all functionality and edge cases.

use anyhow::Result;
use chrono::Utc;
use serde_json;

use crate::framework::TestSetup;
use crate::test_framework::*;
use hitch::types::{Environment, HitchConfig};

#[cfg(test)]
mod tests {
    use super::*;

    // Environment struct tests

    #[test]
    fn test_environment_new_default() -> Result<()> {
        let env = Environment::new("main".to_string());

        assert_eq!(env.base, "main");
        assert!(env.branches.is_empty());
        assert!(!env.locked);
        assert!(env.locked_by.is_none());
        assert!(env.locked_at.is_none());
        assert!(env.rebuilt_at.is_none());
        assert!(env.released_at.is_none());

        Ok(())
    }

    #[test]
    fn test_environment_new_with_custom_base() -> Result<()> {
        let env = Environment::new("develop".to_string());
        assert_eq!(env.base, "develop");
        Ok(())
    }

    #[test]
    fn test_environment_is_locked_initial_state() -> Result<()> {
        let env = Environment::new("main".to_string());
        assert!(!env.is_locked());
        Ok(())
    }

    #[test]
    fn test_environment_lock_functionality() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Test locking
        env.lock("user@example.com".to_string());
        assert!(env.is_locked());
        assert_eq!(env.locked_by, Some("user@example.com".to_string()));
        assert!(env.locked_at.is_some());

        // Test that lock timestamp is recent (within 1 second)
        let now = Utc::now();
        let lock_time = env.locked_at.unwrap();
        let duration = now.signed_duration_since(lock_time);
        assert!(duration.num_seconds() <= 1);

        Ok(())
    }

    #[test]
    fn test_environment_unlock_functionality() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Lock first
        env.lock("user@example.com".to_string());
        assert!(env.is_locked());

        // Then unlock
        env.unlock();
        assert!(!env.is_locked());
        assert!(env.locked_by.is_none());
        assert!(env.locked_at.is_none());

        Ok(())
    }

    #[test]
    fn test_environment_multiple_lock_operations() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Lock with first user
        env.lock("user1@example.com".to_string());
        assert_eq!(env.locked_by, Some("user1@example.com".to_string()));

        // Lock with different user
        env.lock("user2@example.com".to_string());
        assert_eq!(env.locked_by, Some("user2@example.com".to_string()));

        // Timestamp should be updated
        let first_lock_time = env.locked_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        env.lock("user3@example.com".to_string());
        let second_lock_time = env.locked_at.unwrap();
        assert!(second_lock_time > first_lock_time);

        Ok(())
    }

    #[test]
    fn test_environment_add_branch_functionality() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Add first branch
        env.add_branch("feature-1".to_string());
        assert_eq!(env.branches.len(), 1);
        assert!(env.branches.contains(&"feature-1".to_string()));

        // Add second branch
        env.add_branch("feature-2".to_string());
        assert_eq!(env.branches.len(), 2);
        assert!(env.branches.contains(&"feature-2".to_string()));

        // Test duplicate branch (should not be added)
        env.add_branch("feature-1".to_string());
        assert_eq!(env.branches.len(), 2);

        Ok(())
    }

    #[test]
    fn test_environment_remove_branch_functionality() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Add multiple branches
        env.add_branch("feature-1".to_string());
        env.add_branch("feature-2".to_string());
        env.add_branch("feature-3".to_string());
        assert_eq!(env.branches.len(), 3);

        // Remove middle branch
        env.remove_branch("feature-2");
        assert_eq!(env.branches.len(), 2);
        assert!(!env.branches.contains(&"feature-2".to_string()));
        assert!(env.branches.contains(&"feature-1".to_string()));
        assert!(env.branches.contains(&"feature-3".to_string()));

        // Remove non-existent branch (should not panic)
        env.remove_branch("non-existent");
        assert_eq!(env.branches.len(), 2);

        Ok(())
    }

    #[test]
    fn test_environment_has_branch_functionality() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Test empty environment
        assert!(!env.has_branch("feature-1"));

        // Add branch and test
        env.add_branch("feature-1".to_string());
        assert!(env.has_branch("feature-1"));
        assert!(!env.has_branch("feature-2"));

        Ok(())
    }

    #[test]
    fn test_environment_update_rebuilt_timestamp() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Initial state should have no rebuilt timestamp
        assert!(env.rebuilt_at.is_none());

        // Update timestamp
        env.update_rebuilt_timestamp();
        assert!(env.rebuilt_at.is_some());

        // Verify timestamp is recent (within 1 second)
        let now = Utc::now();
        let rebuilt_time = env.rebuilt_at.unwrap();
        let duration = now.signed_duration_since(rebuilt_time);
        assert!(duration.num_seconds() <= 1);

        // Test multiple updates
        let first_time = env.rebuilt_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        env.update_rebuilt_timestamp();
        let second_time = env.rebuilt_at.unwrap();
        assert!(second_time > first_time);

        Ok(())
    }

    #[test]
    fn test_environment_update_released_timestamp() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Initial state should have no released timestamp
        assert!(env.released_at.is_none());

        // Update timestamp
        env.update_released_timestamp();
        assert!(env.released_at.is_some());

        // Verify timestamp is recent (within 1 second)
        let now = Utc::now();
        let released_time = env.released_at.unwrap();
        let duration = now.signed_duration_since(released_time);
        assert!(duration.num_seconds() <= 1);

        // Test multiple updates
        let first_time = env.released_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        env.update_released_timestamp();
        let second_time = env.released_at.unwrap();
        assert!(second_time > first_time);

        Ok(())
    }

    #[test]
    fn test_environment_complex_workflow() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Simulate typical environment lifecycle
        env.add_branch("feature-auth".to_string());
        env.add_branch("feature-ui".to_string());
        env.update_rebuilt_timestamp();
        env.lock("admin@company.com".to_string());
        env.update_released_timestamp();
        env.unlock();

        // Verify final state
        assert_eq!(env.branches.len(), 2);
        assert!(env.rebuilt_at.is_some());
        assert!(env.released_at.is_some());
        assert!(!env.is_locked());

        Ok(())
    }

    // HitchConfig struct tests

    #[test]
    fn test_hitch_config_new_default() -> Result<()> {
        let config = HitchConfig::new();

        assert_eq!(config.version, "1.0");
        assert!(config.environments.is_empty());

        Ok(())
    }

    #[test]
    fn test_hitch_config_default_trait() -> Result<()> {
        let config: HitchConfig = Default::default();

        assert_eq!(config.version, "1.0");
        assert!(config.environments.is_empty());

        Ok(())
    }

    #[test]
    fn test_hitch_config_add_environment() -> Result<()> {
        let mut config = HitchConfig::new();
        let env = Environment::new("main".to_string());

        config.add_environment("dev".to_string(), env);

        assert_eq!(config.environments.len(), 1);
        assert!(config.environment_exists("dev"));
        assert_eq!(config.get_environment_names().len(), 1);

        Ok(())
    }

    #[test]
    fn test_hitch_config_remove_environment() -> Result<()> {
        let mut config = HitchConfig::new();
        let env = Environment::new("main".to_string());

        // Add environment
        config.add_environment("dev".to_string(), env);
        assert_eq!(config.environments.len(), 1);

        // Remove environment
        config.remove_environment("dev");
        assert_eq!(config.environments.len(), 0);
        assert!(!config.environment_exists("dev"));

        Ok(())
    }

    #[test]
    fn test_hitch_config_remove_nonexistent_environment() -> Result<()> {
        let mut config = HitchConfig::new();

        // Remove non-existent environment (should not panic)
        config.remove_environment("nonexistent");
        assert_eq!(config.environments.len(), 0);

        Ok(())
    }

    #[test]
    fn test_hitch_config_get_environment() -> Result<()> {
        let mut config = HitchConfig::new();
        let env = Environment::new("main".to_string());

        // Test getting non-existent environment
        assert!(config.get_environment("dev").is_none());

        // Add environment
        config.add_environment("dev".to_string(), env);

        // Test getting existing environment
        let retrieved = config.get_environment("dev").unwrap();
        assert_eq!(retrieved.base, "main");

        Ok(())
    }

    #[test]
    fn test_hitch_config_get_environment_mut() -> Result<()> {
        let mut config = HitchConfig::new();
        let env = Environment::new("main".to_string());

        // Test getting non-existent environment
        assert!(config.get_environment_mut("dev").is_none());

        // Add environment
        config.add_environment("dev".to_string(), env);

        // Test getting mutable reference and modifying
        {
            let retrieved = config.get_environment_mut("dev").unwrap();
            retrieved.add_branch("feature-1".to_string());
            retrieved.lock("user@example.com".to_string());
        }

        // Verify changes persisted
        let retrieved = config.get_environment("dev").unwrap();
        assert!(retrieved.has_branch("feature-1"));
        assert!(retrieved.is_locked());

        Ok(())
    }

    #[test]
    fn test_hitch_config_environment_exists() -> Result<()> {
        let mut config = HitchConfig::new();
        let env = Environment::new("main".to_string());

        // Test empty config
        assert!(!config.environment_exists("dev"));

        // Add environment
        config.add_environment("dev".to_string(), env);
        assert!(config.environment_exists("dev"));

        Ok(())
    }

    #[test]
    fn test_hitch_config_get_environment_names() -> Result<()> {
        let mut config = HitchConfig::new();

        // Test empty config
        assert!(config.get_environment_names().is_empty());

        // Add multiple environments
        config.add_environment("dev".to_string(), Environment::new("main".to_string()));
        config.add_environment("qa".to_string(), Environment::new("main".to_string()));
        config.add_environment("prod".to_string(), Environment::new("main".to_string()));

        let names = config.get_environment_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"dev".to_string()));
        assert!(names.contains(&"qa".to_string()));
        assert!(names.contains(&"prod".to_string()));

        Ok(())
    }

    #[test]
    fn test_hitch_config_complex_workflow() -> Result<()> {
        let mut config = HitchConfig::new();

        // Add multiple environments
        config.add_environment("dev".to_string(), Environment::new("develop".to_string()));
        config.add_environment("qa".to_string(), Environment::new("main".to_string()));
        config.add_environment("prod".to_string(), Environment::new("main".to_string()));

        // Modify environments
        {
            let dev_env = config.get_environment_mut("dev").unwrap();
            dev_env.add_branch("feature-auth".to_string());
            dev_env.update_rebuilt_timestamp();
        }

        {
            let qa_env = config.get_environment_mut("qa").unwrap();
            qa_env.add_branch("feature-auth".to_string());
            qa_env.lock("qa-team@company.com".to_string());
        }

        // Verify state
        assert_eq!(config.get_environment_names().len(), 3);

        let dev_env = config.get_environment("dev").unwrap();
        assert_eq!(dev_env.base, "develop");
        assert!(dev_env.has_branch("feature-auth"));

        let qa_env = config.get_environment("qa").unwrap();
        assert!(qa_env.is_locked());

        Ok(())
    }

    // Serialization/Deserialization tests

    #[test]
    fn test_environment_serialization_roundtrip() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let mut original_env = Environment::new("main".to_string());
            original_env.add_branch("feature-1".to_string());
            original_env.lock("user@example.com".to_string());
            original_env.update_rebuilt_timestamp();

            // Serialize to JSON
            env.fs.write_json("env.json", &original_env)?;

            // Deserialize from JSON
            let deserialized: Environment = env.fs.read_json("env.json")?;

            // Verify all fields match
            assert_eq!(deserialized.base, original_env.base);
            assert_eq!(deserialized.branches, original_env.branches);
            assert_eq!(deserialized.locked, original_env.locked);
            assert_eq!(deserialized.locked_by, original_env.locked_by);
            assert_eq!(deserialized.locked_at, original_env.locked_at);
            assert_eq!(deserialized.rebuilt_at, original_env.rebuilt_at);

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_hitch_config_serialization_roundtrip() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            let mut original_config = HitchConfig::new();
            original_config
                .add_environment("dev".to_string(), Environment::new("develop".to_string()));
            original_config
                .add_environment("prod".to_string(), Environment::new("main".to_string()));

            // Serialize to JSON
            env.fs.write_json("config.json", &original_config)?;

            // Deserialize from JSON
            let deserialized: HitchConfig = env.fs.read_json("config.json")?;

            // Verify all fields match
            assert_eq!(deserialized.version, original_config.version);
            assert_eq!(
                deserialized.environments.len(),
                original_config.environments.len()
            );
            assert!(deserialized.environment_exists("dev"));
            assert!(deserialized.environment_exists("prod"));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    #[test]
    fn test_environment_json_import_export() -> Result<()> {
        let framework = HitchTestFramework::new()?;

        let _ = framework.with_test_environment(TestSetup::HitchInit, |env| {
            // Create a JSON fixture
            let json_fixture = serde_json::json!({
                "base": "main",
                "branches": ["feature-1", "feature-2"],
                "locked": true,
                "locked_by": "admin@example.com",
                "locked_at": "2024-01-15T10:30:00Z",
                "rebuilt_at": "2024-01-14T15:45:00Z",
                "released_at": "2024-01-13T09:00:00Z"
            });

            // Write JSON fixture
            env.fs.write_json("fixture.json", &json_fixture)?;

            // Import as Environment
            let env_import: Environment = env.fs.read_json("fixture.json")?;

            // Verify imported data
            assert_eq!(env_import.base, "main");
            assert_eq!(env_import.branches.len(), 2);
            assert!(env_import.branches.contains(&"feature-1".to_string()));
            assert!(env_import.is_locked());
            assert_eq!(env_import.locked_by, Some("admin@example.com".to_string()));

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }

    // Edge cases and error handling tests

    #[test]
    fn test_environment_empty_branch_operations() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Test operations on empty branches list
        assert!(!env.has_branch("any-branch"));
        env.remove_branch("non-existent");
        assert!(env.branches.is_empty());

        Ok(())
    }

    #[test]
    fn test_environment_lock_unlock_cycles() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Test multiple lock/unlock cycles
        for i in 0..5 {
            env.lock(format!("user{}@example.com", i));
            assert!(env.is_locked());
            assert_eq!(env.locked_by, Some(format!("user{}@example.com", i)));

            env.unlock();
            assert!(!env.is_locked());
            assert!(env.locked_by.is_none());
            assert!(env.locked_at.is_none());
        }

        Ok(())
    }

    #[test]
    fn test_hitch_config_environment_name_edge_cases() -> Result<()> {
        let mut config = HitchConfig::new();

        // Test environment names with special characters, spaces, etc.
        let special_names = vec![
            "dev-environment",
            "qa_environment",
            "prod.environment",
            "test123",
            "ENVIRONMENT",
        ];

        for name in &special_names {
            config.add_environment(name.to_string(), Environment::new("main".to_string()));
            assert!(config.environment_exists(name));
        }

        let retrieved_names = config.get_environment_names();
        for name in &special_names {
            assert!(retrieved_names.contains(&name.to_string()));
        }

        Ok(())
    }

    #[test]
    fn test_environment_timestamp_consistency() -> Result<()> {
        let mut env = Environment::new("main".to_string());

        // Get a base timestamp
        let base_time = Utc::now();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Update timestamps in sequence
        env.update_rebuilt_timestamp();
        std::thread::sleep(std::time::Duration::from_millis(10));
        env.lock("user@example.com".to_string());
        std::thread::sleep(std::time::Duration::from_millis(10));
        env.update_released_timestamp();

        // Verify timestamp ordering
        assert!(env.rebuilt_at.unwrap() > base_time);
        assert!(env.locked_at.unwrap() > env.rebuilt_at.unwrap());
        assert!(env.released_at.unwrap() > env.locked_at.unwrap());

        Ok(())
    }
}
