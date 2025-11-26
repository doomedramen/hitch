//! Test fixtures and data management
//!
//! Provides test data, templates, and fixtures for common testing scenarios
//! in Hitch CLI testing.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::test_framework::file_system_helpers::FileSystemHelpers;

/// Test fixtures and data management
#[derive(Debug)]
pub struct TestFixtures {
    #[allow(dead_code)]
    fixtures_dir: std::path::PathBuf,
}

impl Default for TestFixtures {
    fn default() -> Self {
        Self::new()
    }
}

impl TestFixtures {
    /// Create new test fixtures manager
    pub fn new() -> Self {
        TestFixtures {
            fixtures_dir: std::env::current_dir()
                .unwrap_or_default()
                .join("tests/test_data"),
        }
    }

    /// Create test fixtures with custom fixtures directory
    pub fn with_dir<P: AsRef<Path>>(fixtures_dir: P) -> Self {
        TestFixtures {
            fixtures_dir: fixtures_dir.as_ref().to_path_buf(),
        }
    }

    // Hitch configuration fixtures

    /// Get a basic hitch configuration fixture
    pub fn basic_hitch_config(&self) -> serde_json::Value {
        serde_json::json!({
            "version": "1.0",
            "environments": {
                "dev": {
                    "base": "main",
                    "branches": [],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": null,
                    "released_at": null
                },
                "qa": {
                    "base": "main",
                    "branches": [],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": null,
                    "released_at": null
                },
                "prod": {
                    "base": "main",
                    "branches": [],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": null,
                    "released_at": null
                }
            }
        })
    }

    /// Get a hitch configuration fixture with promoted branches
    pub fn hitch_config_with_promotions(&self) -> serde_json::Value {
        serde_json::json!({
            "version": "1.0",
            "environments": {
                "dev": {
                    "base": "main",
                    "branches": ["feature-auth", "feature-ui"],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": "2024-01-15T10:30:00Z",
                    "released_at": null
                },
                "qa": {
                    "base": "main",
                    "branches": ["feature-auth"],
                    "locked": false,
                    "locked_by": null,
                    "locked_at": null,
                    "rebuilt_at": "2024-01-14T15:45:00Z",
                    "released_at": null
                },
                "prod": {
                    "base": "main",
                    "branches": [],
                    "locked": true,
                    "locked_by": "admin@company.com",
                    "locked_at": "2024-01-13T09:00:00Z",
                    "rebuilt_at": "2024-01-12T12:00:00Z",
                    "released_at": "2024-01-12T13:30:00Z"
                }
            }
        })
    }

    /// Get a minimal hitch configuration fixture
    pub fn minimal_hitch_config(&self) -> serde_json::Value {
        serde_json::json!({
            "version": "1.0",
            "environments": {}
        })
    }

    // Git repository fixtures

    /// Create a basic git repository structure
    pub fn basic_git_structure(&self) -> HashMap<&'static str, Option<&'static str>> {
        let mut structure = HashMap::new();

        // Basic project structure
        structure.insert("src", None);
        structure.insert(
            "src/main.rs",
            Some(
                r#"fn main() {
    println!("Hello, World!");
}"#,
            ),
        );
        structure.insert(
            "Cargo.toml",
            Some(
                r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
            ),
        );
        structure.insert("README.md", Some("# Test Project\n\nA basic test project."));
        structure.insert(".gitignore", Some("/target/\nCargo.lock\n"));

        structure
    }

    /// Create a complex git repository structure with multiple branches
    pub fn complex_git_structure(&self) -> HashMap<&'static str, Option<&'static str>> {
        let mut structure = HashMap::new();

        // Main branch files
        structure.insert("src", None);
        structure.insert(
            "src/main.rs",
            Some(
                r#"fn main() {
    println!("Main application");
}"#,
            ),
        );
        structure.insert(
            "src/lib.rs",
            Some(
                r#"pub mod auth;
pub mod api;
pub mod utils;

pub fn version() -> &'static str {
    "1.0.0"
}
"#,
            ),
        );
        structure.insert(
            "src/auth.rs",
            Some(
                r#"pub fn authenticate(username: &str, password: &str) -> bool {
    username == "admin" && password == "secret"
}
"#,
            ),
        );
        structure.insert(
            "src/api.rs",
            Some(
                r#"pub mod user;
pub mod auth;
"#,
            ),
        );
        structure.insert(
            "src/api/user.rs",
            Some(
                r#"pub struct User {
    pub id: u32,
    pub name: String,
}

pub fn get_user(id: u32) -> Option<User> {
    Some(User {
        id,
        name: format!("User {}", id),
    })
}
"#,
            ),
        );
        structure.insert("src/api/auth.rs", Some(r#"pub fn login_endpoint(username: &str, password: &str) -> Result<String, &'static str> {
    if crate::auth::authenticate(username, password) {
        Ok("token_12345".to_string())
    } else {
        Err("Invalid credentials")
    }
}
"#));
        structure.insert(
            "src/utils.rs",
            Some(
                r#"pub fn format_error(error: &str) -> String {
    format!("Error: {}", error)
}
"#,
            ),
        );

        // Configuration files
        structure.insert(
            "Cargo.toml",
            Some(
                r#"[package]
name = "complex-project"
version = "1.0.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full"] }

[dev-dependencies]
tempfile = "3.0"
"#,
            ),
        );
        structure.insert(
            "README.md",
            Some("# Complex Project\n\nA more complex test project with multiple modules."),
        );
        structure.insert(".gitignore", Some("/target/\nCargo.lock\n*.log\n.env\n"));

        // Documentation
        structure.insert("docs", None);
        structure.insert("docs/api.md", Some("# API Documentation\n\n..."));
        structure.insert("docs/auth.md", Some("# Authentication\n\n..."));

        // Tests
        structure.insert("tests", None);
        structure.insert(
            "tests/integration_tests.rs",
            Some(
                r#"[test]
fn test_integration() {
    assert_eq!(1 + 1, 2);
}
"#,
            ),
        );

        structure
    }

    // Branch templates for different scenarios

    /// Data for feature-auth branch
    pub fn feature_auth_branch_files(&self) -> HashMap<&'static str, Option<&'static str>> {
        let mut files = HashMap::new();

        files.insert(
            "src/auth.rs",
            Some(
                r#"pub mod oauth;
pub mod jwt;

pub fn authenticate(username: &str, password: &str) -> bool {
    username == "admin" && password == "secret"
}

pub fn authenticate_with_oauth(token: &str) -> bool {
    token == "valid_oauth_token"
}

pub fn generate_jwt(user_id: u32) -> String {
    format!("jwt_{}", user_id)
}
"#,
            ),
        );

        files.insert(
            "src/auth/oauth.rs",
            Some(
                r#"pub fn validate_oauth_token(token: &str) -> bool {
    token.starts_with("oauth_") && token.len() > 10
}
"#,
            ),
        );

        files.insert(
            "src/auth/jwt.rs",
            Some(
                r#"pub fn validate_jwt(token: &str) -> bool {
    token.starts_with("jwt_") && token.len() > 5
}
"#,
            ),
        );

        files
    }

    /// Data for feature-ui branch
    pub fn feature_ui_branch_files(&self) -> HashMap<&'static str, Option<&'static str>> {
        let mut files = HashMap::new();

        files.insert(
            "src/ui/mod.rs",
            Some(
                r#"pub mod components;
pub mod pages;
pub mod styles;

pub fn render_app() -> String {
    "App rendered".to_string()
}
"#,
            ),
        );

        files.insert(
            "src/ui/components.rs",
            Some(
                r#"pub struct Button {
    pub text: String,
    pub onclick: Option<String>,
}

impl Button {
    pub fn new(text: &str) -> Self {
        Button {
            text: text.to_string(),
            onclick: None,
        }
    }
}
"#,
            ),
        );

        files.insert(
            "src/ui/pages.rs",
            Some(
                r#"pub struct HomePage {
    pub title: String,
}

impl HomePage {
    pub fn new() -> Self {
        HomePage {
            title: "Welcome".to_string(),
        }
    }
}
"#,
            ),
        );

        files
    }

    /// Data for hotfix-security branch
    pub fn hotfix_security_branch_files(&self) -> HashMap<&'static str, Option<&'static str>> {
        let mut files = HashMap::new();

        files.insert(
            "src/auth.rs",
            Some(
                r#"pub fn authenticate(username: &str, password: &str) -> bool {
    // Added password complexity validation
    if password.len() < 8 {
        return false;
    }

    // Added rate limiting (simplified)
    static mut ATTEMPTS: u32 = 0;
    unsafe {
        if ATTEMPTS >= 3 {
            return false;
        }
        ATTEMPTS += 1;
    }

    username == "admin" && password == "super_secret_password_123"
}
"#,
            ),
        );

        files.insert("SECURITY.md", Some("# Security Notes\n\n## Hotfix Details\n\n- Added password complexity validation\n- Added rate limiting\n- Updated security protocols"));
        files.insert("CHANGELOG.md", Some("# Changelog\n\n## [1.0.1] - 2024-01-20\n\n### Security\n- Added password complexity requirements\n- Implemented authentication rate limiting\n- Enhanced security protocols"));

        files
    }

    // Utility methods

    /// Create a complete project structure in the given file system
    pub fn create_project_structure(
        &self,
        fs: &FileSystemHelpers,
        structure: &HashMap<&str, Option<&str>>,
    ) -> Result<()> {
        for (path, content) in structure {
            if let Some(content) = content {
                // It's a file
                fs.write_file(path, content)?;
            } else {
                // It's a directory
                fs.create_dir(path)?;
            }
        }
        Ok(())
    }

    /// Create a basic hitch project with git and hitch initialization
    pub fn create_basic_hitch_project(&self, fs: &FileSystemHelpers) -> Result<()> {
        // Create basic git structure
        let git_structure = self.basic_git_structure();
        self.create_project_structure(fs, &git_structure)?;

        // Create hitch configuration
        let hitch_config = self.basic_hitch_config();
        fs.write_json("hitch.json", &hitch_config)?;

        Ok(())
    }

    /// Create a complex hitch project with multiple environments and promotions
    pub fn create_complex_hitch_project(&self, fs: &FileSystemHelpers) -> Result<()> {
        // Create complex git structure
        let git_structure = self.complex_git_structure();
        self.create_project_structure(fs, &git_structure)?;

        // Create hitch configuration with promotions
        let hitch_config = self.hitch_config_with_promotions();
        fs.write_json("hitch.json", &hitch_config)?;

        Ok(())
    }

    /// Create test data for specific scenarios
    pub fn create_test_scenario(
        &self,
        fs: &FileSystemHelpers,
        scenario: TestScenario,
    ) -> Result<()> {
        match scenario {
            TestScenario::BasicProject => self.create_basic_hitch_project(fs),
            TestScenario::ComplexProject => self.create_complex_hitch_project(fs),
            TestScenario::EmptyProject => {
                // Create minimal hitch config
                let config = self.minimal_hitch_config();
                fs.write_json("hitch.json", &config)?;
                Ok(())
            }
            TestScenario::CorruptedConfig => {
                // Write invalid JSON
                fs.write_file("hitch.json", "{ invalid json }")?;
                Ok(())
            }
        }
    }
}

/// Test scenarios for different project states
#[derive(Debug, Clone, Copy)]
pub enum TestScenario {
    /// A basic Hitch project with standard configuration
    BasicProject,

    /// A complex project with multiple environments and promotions
    ComplexProject,

    /// An empty project with minimal configuration
    EmptyProject,

    /// A project with corrupted hitch.json
    CorruptedConfig,
}

impl TestScenario {
    /// Get all test scenarios
    pub fn all() -> &'static [TestScenario] {
        use TestScenario::*;
        &[BasicProject, ComplexProject, EmptyProject, CorruptedConfig]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_basic_hitch_config() -> Result<()> {
        let fixtures = TestFixtures::new();
        let config = fixtures.basic_hitch_config();

        assert_eq!(config["version"], "1.0");
        assert!(config["environments"]
            .as_object()
            .unwrap()
            .contains_key("dev"));
        assert!(config["environments"]
            .as_object()
            .unwrap()
            .contains_key("qa"));
        assert!(config["environments"]
            .as_object()
            .unwrap()
            .contains_key("prod"));

        Ok(())
    }

    #[test]
    fn test_complex_git_structure() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());
        let fixtures = TestFixtures::new();

        let structure = fixtures.complex_git_structure();
        fixtures.create_project_structure(&fs, &structure)?;

        // Verify structure was created
        assert!(fs.file_exists("src/main.rs"));
        assert!(fs.file_exists("src/auth.rs"));
        assert!(fs.file_exists("Cargo.toml"));
        assert!(fs.dir_exists("docs"));
        assert!(fs.dir_exists("tests"));

        Ok(())
    }

    #[test]
    fn test_create_basic_hitch_project() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());
        let fixtures = TestFixtures::new();

        fixtures.create_basic_hitch_project(&fs)?;

        // Verify hitch project was created
        assert!(fs.file_exists("hitch.json"));
        assert!(fs.file_exists("src/main.rs"));
        assert!(fs.file_exists("Cargo.toml"));

        // Verify hitch config is valid
        let config: serde_json::Value = fs.read_json("hitch.json")?;
        assert!(config["environments"]
            .as_object()
            .unwrap()
            .contains_key("dev"));

        Ok(())
    }

    #[test]
    fn test_test_scenarios() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());
        let fixtures = TestFixtures::new();

        // Test each scenario
        for scenario in TestScenario::all() {
            let scenario_dir = format!("scenario_{}", fs.temp_file("test", "")?.display());
            fs.create_dir(&scenario_dir)?;

            let scenario_fs = FileSystemHelpers::new(&fs.resolve_path(&scenario_dir));
            fixtures.create_test_scenario(&scenario_fs, *scenario)?;

            match scenario {
                TestScenario::CorruptedConfig => {
                    assert!(scenario_fs.file_exists("hitch.json"));
                    // Should not be valid JSON
                    assert!(scenario_fs
                        .read_json::<serde_json::Value>("hitch.json")
                        .is_err());
                }
                _ => {
                    // Other scenarios should have valid hitch.json
                    assert!(scenario_fs.file_exists("hitch.json"));
                    assert!(scenario_fs
                        .read_json::<serde_json::Value>("hitch.json")
                        .is_ok());
                }
            }
        }

        Ok(())
    }
}
