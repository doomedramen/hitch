use crate::commands::global_context::GlobalContext;
use crate::types::{Environment, HitchConfig};
use crate::utils::prelude::{modify_metadata, pre_check};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct InitCommand {
    /// Optional comma-separated list of environments to create during initialization
    #[arg(long, value_delimiter = ',')]
    environments: Option<Vec<String>>,
}

pub fn run(args: InitCommand, context: &GlobalContext) -> Result<()> {
    context.log_info("Initializing Hitch in the current git repository...");

    // Step 1: pre-check() - Ensure current directory is a Git repository and working tree is clean
    pre_check(context)?;

    // Step 2: Ensure hitch-metadata branch does not exist
    context.log_verbose("Checking if hitch-metadata branch already exists...");
    if context.git().branch_exists("hitch-metadata")? {
        return Err(anyhow::anyhow!("Hitch is already initialized in this repository. hitch-metadata branch already exists."));
    }
    context.log_verbose("✓ hitch-metadata branch does not exist");

    // Record the original branch before switching to the orphan branch
    let original_branch = context.git().get_current_branch()?;
    context.log_verbose(&format!("Original branch recorded: {}", original_branch));

    // Step 3: Create orphan branch hitch-metadata
    context.log_info("Creating hitch-metadata branch...");
    context.git().create_orphan_branch("hitch-metadata")?;
    context.log_verbose("✓ Created orphan hitch-metadata branch");

    // Step 4: modify_metadata() - Create hitch.json skeleton and .gitignore
    modify_metadata(context, |config: &mut HitchConfig| {
        // Remove all files/folders except .git (this is already done by create_orphan_branch)
        context.log_verbose("Cleaning hitch-metadata branch...");

        // Create .gitignore
        context.log_verbose("Creating .gitignore...");
        let gitignore_content = "*\n!.gitignore\n!hitch.json\n!hitch/\n!hitch/**\n";
        context.git().write_file(".gitignore", gitignore_content)?;

        // Create or update hitch.json skeleton
        context.log_verbose("Creating hitch.json configuration...");

        // Add environments if specified
        if let Some(env_names) = &args.environments {
            context.log_info(&format!(
                "Creating {} environment(s): {}",
                env_names.len(),
                env_names.join(", ")
            ));

            for env_name in env_names {
                let env = Environment::new("main".to_string()); // Default to main as per spec
                config.add_environment(env_name.clone(), env).map_err(|e| {
                    anyhow::anyhow!("Failed to add environment '{}': {}", env_name, e)
                })?;
            }
        }

        context.log_verbose("✓ Configuration skeleton created");

        Ok(())
    })?;

    if context.should_push() {
        context.log_verbose("Pushing hitch-metadata branch to remote...");
        if let Err(e) = context.git().push_branch("hitch-metadata") {
            context.log_warning(&format!("Failed to push hitch-metadata to remote: {}", e));
            context.log_warning("You can push manually later with: git push origin hitch-metadata");
        } else {
            context.log_success("hitch-metadata branch pushed to remote");
        }
    } else {
        context.log_verbose("Skipping push due to --no-push flag");
    }

    // Return to original branch
    context.log_verbose(&format!(
        "Switching back to original branch: {}",
        original_branch
    ));
    if let Err(e) = context.git().checkout_branch(&original_branch) {
        context.log_error(&format!(
            "Failed to return to original branch '{}': {}",
            original_branch, e
        ));
        context.log_warning("You may need to manually switch back to your original branch");
    }

    context.log_success("Hitch initialized successfully!");

    if let Some(env_names) = &args.environments {
        context.log_info(&format!("Created environments: {}", env_names.join(", ")));
        context.log_info("You can now promote branches to these environments using 'hitch promote <branch> to <environment>'");
    } else {
        context.log_info("You can add environments using 'hitch add <environment-name>'");
    }

    Ok(())
}
