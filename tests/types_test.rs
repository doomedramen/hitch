use anyhow::Result;
use hitch::types::{Environment, HitchConfig};
use chrono::Utc;

#[test]
fn test_environment_new() -> Result<()> {
    let env = Environment::new("test".to_string(), "main".to_string());

    assert_eq!(env.name, "test");
    assert_eq!(env.source, "main");
    assert!(env.branches.is_empty());
    assert!(!env.is_locked());
    assert!(env.locked_by.is_none());
    assert!(env.locked_at.is_none());
    assert!(env.rebuilt_at.is_none());

    Ok(())
}

#[test]
fn test_environment_locking() -> Result<()> {
    let mut env = Environment::new("test".to_string(), "main".to_string());
    let user_email = "user@example.com".to_string();

    // Test initial state
    assert!(!env.is_locked());

    // Test locking
    env.lock(user_email.clone());
    assert!(env.is_locked());
    assert_eq!(env.locked_by, Some(user_email.clone()));
    assert!(env.locked_at.is_some());

    // Test unlocking
    env.unlock();
    assert!(!env.is_locked());
    assert!(env.locked_by.is_none());
    assert!(env.locked_at.is_none());

    Ok(())
}

#[test]
fn test_environment_branch_management() -> Result<()> {
    let mut env = Environment::new("test".to_string(), "main".to_string());

    // Test adding branches
    env.add_branch("feature1".to_string());
    env.add_branch("feature2".to_string());

    assert!(env.has_branch("feature1"));
    assert!(env.has_branch("feature2"));
    assert!(!env.has_branch("feature3"));
    assert_eq!(env.branches.len(), 2);

    // Test adding duplicate branch (should not duplicate)
    env.add_branch("feature1".to_string());
    assert_eq!(env.branches.len(), 2);

    // Test removing branches
    env.remove_branch("feature1");
    assert!(!env.has_branch("feature1"));
    assert!(env.has_branch("feature2"));
    assert_eq!(env.branches.len(), 1);

    // Test removing non-existent branch (should not error)
    env.remove_branch("nonexistent");
    assert_eq!(env.branches.len(), 1);

    Ok(())
}

#[test]
fn test_environment_rebuilt_timestamp() -> Result<()> {
    let mut env = Environment::new("test".to_string(), "main".to_string());

    // Initially no rebuilt timestamp
    assert!(env.rebuilt_at.is_none());

    // Update rebuilt timestamp
    env.update_rebuilt_timestamp();
    assert!(env.rebuilt_at.is_some());

    // Verify timestamp is recent (within last few seconds)
    if let Some(timestamp) = env.rebuilt_at {
        let now = Utc::now();
        let diff = now.signed_duration_since(timestamp);
        assert!(diff.num_seconds() < 5, "Timestamp should be recent");
    }

    Ok(())
}

#[test]
fn test_hitch_config_new() -> Result<()> {
    let config = HitchConfig::new();

    assert_eq!(config.version, "1.0");
    assert!(config.environments.is_empty());

    Ok(())
}

#[test]
fn test_hitch_config_add_environment() -> Result<()> {
    let mut config = HitchConfig::new();
    let env = Environment::new("dev".to_string(), "main".to_string());

    config.add_environment(env);

    assert_eq!(config.environments.len(), 1);
    assert!(config.environment_exists("dev"));
    assert!(!config.environment_exists("prod"));

    let retrieved_env = config.get_environment("dev").unwrap();
    assert_eq!(retrieved_env.name, "dev");
    assert_eq!(retrieved_env.source, "main");

    Ok(())
}

#[test]
fn test_hitch_config_remove_environment() -> Result<()> {
    let mut config = HitchConfig::new();
    let env1 = Environment::new("dev".to_string(), "main".to_string());
    let env2 = Environment::new("prod".to_string(), "main".to_string());

    config.add_environment(env1);
    config.add_environment(env2);

    assert_eq!(config.environments.len(), 2);

    config.remove_environment("dev");

    assert_eq!(config.environments.len(), 1);
    assert!(!config.environment_exists("dev"));
    assert!(config.environment_exists("prod"));

    // Test removing non-existent environment (should not error)
    config.remove_environment("nonexistent");
    assert_eq!(config.environments.len(), 1);

    Ok(())
}

#[test]
fn test_hitch_config_get_environment_mut() -> Result<()> {
    let mut config = HitchConfig::new();
    let env = Environment::new("dev".to_string(), "main".to_string());

    config.add_environment(env);

    // Test getting mutable environment
    let env_mut = config.get_environment_mut("dev").unwrap();
    env_mut.add_branch("feature1".to_string());

    // Verify changes persisted
    let env_immutable = config.get_environment("dev").unwrap();
    assert!(env_immutable.has_branch("feature1"));

    // Test getting non-existent environment
    assert!(config.get_environment_mut("nonexistent").is_none());

    Ok(())
}

#[test]
fn test_hitch_config_environment_names() -> Result<()> {
    let mut config = HitchConfig::new();

    // Initially empty
    assert!(config.get_environment_names().is_empty());

    // Add environments
    config.add_environment(Environment::new("dev".to_string(), "main".to_string()));
    config.add_environment(Environment::new("prod".to_string(), "main".to_string()));
    config.add_environment(Environment::new("staging".to_string(), "main".to_string()));

    let names = config.get_environment_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"dev".to_string()));
    assert!(names.contains(&"prod".to_string()));
    assert!(names.contains(&"staging".to_string()));

    // Remove environment
    config.remove_environment("dev");
    let names = config.get_environment_names();
    assert_eq!(names.len(), 2);
    assert!(!names.contains(&"dev".to_string()));

    Ok(())
}

#[test]
fn test_environment_serialization() -> Result<()> {
    let env = Environment::new("test".to_string(), "main".to_string());

    // Test serialization
    let json = serde_json::to_string_pretty(&env)?;
    assert!(json.contains("test"));
    assert!(json.contains("main"));

    // Test deserialization
    let deserialized: Environment = serde_json::from_str(&json)?;
    assert_eq!(deserialized.name, env.name);
    assert_eq!(deserialized.source, env.source);

    Ok(())
}

#[test]
fn test_hitch_config_serialization() -> Result<()> {
    let mut config = HitchConfig::new();
    config.add_environment(Environment::new("dev".to_string(), "main".to_string()));

    // Test serialization
    let json = serde_json::to_string_pretty(&config)?;
    assert!(json.contains("1.0"));
    assert!(json.contains("dev"));

    // Test deserialization
    let deserialized: HitchConfig = serde_json::from_str(&json)?;
    assert_eq!(deserialized.version, config.version);
    assert_eq!(deserialized.environments.len(), config.environments.len());

    Ok(())
}

#[test]
fn test_hitch_config_default() -> Result<()> {
    let config = HitchConfig::default();

    // Should be equivalent to new()
    let expected = HitchConfig::new();

    assert_eq!(config.version, expected.version);
    assert_eq!(config.environments.len(), expected.environments.len());

    // Verify it creates the expected default values
    assert_eq!(config.version, "1.0");
    assert!(config.environments.is_empty());

    Ok(())
}