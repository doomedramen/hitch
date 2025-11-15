use crate::commands::global_context::GlobalContext;
use clap::Args;
use anyhow::Result;

#[derive(Args)]
pub struct AddCommand {
    /// Environment name to add
    pub env_name: String,

    /// Source branch for the environment (defaults to main)
    #[arg(long)]
    source: Option<String>,
}

pub fn run(
    args: AddCommand,
    context: &GlobalContext,
) -> Result<()> {
    context.log_info(&format!("Adding environment '{}'...", args.env_name));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to add
    validate_preconditions(context, &args.env_name)?;

    // Step 3: Add the environment
    add_environment(context, &args.env_name, &args.source)?;

    context.log_success(&format!("Successfully added environment '{}'!", args.env_name));
    Ok(())
}

/// Validate that a name is valid for git branches/environments
fn validate_name(name: &str, name_type: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow::anyhow!("{} name cannot be empty", name_type));
    }

    if name.len() > 100 {
        return Err(anyhow::anyhow!("{} name cannot exceed 100 characters", name_type));
    }

    // Check for invalid characters that would cause issues in git
    let invalid_chars = ["..", "@{", ":", "[", "]", "\\", "^", "~", "?", "*"];
    for invalid in &invalid_chars {
        if name.contains(invalid) {
            return Err(anyhow::anyhow!(
                "{} name cannot contain '{}': '{}'",
                name_type,
                invalid,
                name
            ));
        }
    }

    // Cannot start or end with slash
    if name.starts_with('/') || name.ends_with('/') {
        return Err(anyhow::anyhow!(
            "{} name cannot start or end with '/': '{}'",
            name_type,
            name
        ));
    }

    // Cannot have consecutive slashes
    if name.contains("//") {
        return Err(anyhow::anyhow!(
            "{} name cannot contain consecutive slashes: '{}'",
            name_type,
            name
        ));
    }

    Ok(())
}

/// Validate that environment is ready for addition
fn validate_preconditions(
    context: &GlobalContext,
    env_name: &str,
) -> Result<()> {
    context.log_verbose("Validating add preconditions...");

    // Validate input name
    validate_name(env_name, "Environment")?;

    // Check if environment already exists
    let config = crate::utils::prelude::access_metadata_read_only(context, |config| {
        Ok(config.clone())
    })?;

    if config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' already exists", env_name));
    }

    context.log_verbose(&format!("✓ Add validation passed for '{}'", env_name));
    Ok(())
}

/// Add a new environment to the configuration
fn add_environment(
    context: &GlobalContext,
    env_name: &str,
    source: &Option<String>,
) -> Result<()> {
    context.log_verbose(&format!("Adding environment '{}' to configuration...", env_name));

    // Use 'main' as default source branch if not specified
    let base_branch = source.as_deref().unwrap_or("main");

    // Validate that the base branch exists
    if !context.git().branch_exists_anywhere(base_branch)? {
        return Err(anyhow::anyhow!(
            "Base branch '{}' does not exist locally or remotely",
            base_branch
        ));
    }

    // Modify metadata to add the new environment
    crate::utils::prelude::modify_metadata(context, |config| {
        use crate::types::Environment;

        let environment = Environment::new(base_branch.to_string());
        config.add_environment(env_name.to_string(), environment);

        context.log_verbose(&format!("✓ Added environment '{}' with base branch '{}'", env_name, base_branch));
        Ok(())
    })
}
