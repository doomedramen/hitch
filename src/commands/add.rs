use crate::commands::global_context::GlobalContext;
use crate::utils::validation::{
    validate_base_branch_exists, validate_environment_not_exists, validate_name,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct AddCommand {
    /// Environment name to add
    pub env_name: String,

    /// Source branch for the environment (defaults to main)
    #[arg(long)]
    source: Option<String>,
}

pub fn run(args: AddCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Adding environment '{}'...", args.env_name));

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    crate::utils::prelude::pre_check(context)?;

    // Step 2: Additional validation specific to add
    validate_preconditions(context, &args.env_name)?;

    // Step 3: Add the environment
    add_environment(context, &args.env_name, &args.source)?;

    context.log_success(&format!(
        "Successfully added environment '{}'!",
        args.env_name
    ));
    Ok(())
}

/// Validate that environment is ready for addition
fn validate_preconditions(context: &GlobalContext, env_name: &str) -> Result<()> {
    context.log_verbose("Validating add preconditions...");

    // Use reusable validation functions (following SPEC principles)
    validate_name(env_name, "Environment")?;
    validate_environment_not_exists(context, env_name)?;

    context.log_verbose(&format!("✓ Add validation passed for '{}'", env_name));
    Ok(())
}

/// Add a new environment to the configuration
fn add_environment(context: &GlobalContext, env_name: &str, source: &Option<String>) -> Result<()> {
    context.log_verbose(&format!(
        "Adding environment '{}' to configuration...",
        env_name
    ));

    // Use 'main' as default source branch if not specified
    let base_branch = source.as_deref().unwrap_or("main");

    // Use reusable base branch validation
    validate_base_branch_exists(context, base_branch)?;

    // Modify metadata to add the new environment
    crate::utils::prelude::modify_metadata(context, |config| {
        use crate::types::Environment;

        let environment = Environment::new(base_branch.to_string());
        config.add_environment(env_name.to_string(), environment);

        context.log_verbose(&format!(
            "✓ Added environment '{}' with base branch '{}'",
            env_name, base_branch
        ));
        Ok(())
    })
}
