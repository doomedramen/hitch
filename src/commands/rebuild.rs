use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{access_metadata_read_only, with_locked_env};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RebuildCommand {
    /// The name of the environment to rebuild
    #[arg()]
    pub env_name: String,

    /// Reuse shared conflict resolutions (rerere) stored in hitch-metadata
    #[arg(long)]
    pub reuse_resolutions: bool,

    /// Force rebuild even if environment is locked
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: RebuildCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Rebuilding environment '{}'...", args.env_name));

    // Step 1: Precondition checks (allow dirty tree — we'll stash it)
    // Only check git repo, not working tree cleanliness
    if let Err(e) = context.git().get_current_branch() {
        return Err(anyhow::anyhow!("Not in a Git repository: {}", e));
    }
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    let rerere_original = if args.reuse_resolutions {
        let original = context.git().get_git_config("rerere.enabled")?;
        context.git().set_git_config("rerere.enabled", "true")?;
        if let Ok(Some(summary)) = crate::utils::rerere::import_shared_rerere_cache(context) {
            context.log_verbose(&format!(
                "Imported {} shared rerere entr(y/ies) ({} files).",
                summary.imported_entries, summary.imported_files
            ));
        }
        original
    } else {
        None
    };

    // Step 2-5: Auto-stash dirty changes, execute rebuild, pop stash
    let result = crate::utils::prelude::with_auto_stash(context, || {
        if args.force {
            context.log_info(&format!(
                "Force rebuilding locked environment '{}'...",
                args.env_name
            ));
            crate::utils::prelude::rebuild_environment(context, &args.env_name)
        } else {
            with_locked_env(context, &args.env_name, || {
                crate::utils::prelude::rebuild_environment(context, &args.env_name)
            })
        }
    });

    match result {
        Ok(()) => {
            if args.reuse_resolutions {
                restore_rerere_config(context, rerere_original)?;
            }
        }
        Err(e) => {
            if args.reuse_resolutions
                && crate::utils::resolve_state::resolve_state_exists(&context.git().get_git_dir())
            {
                // Persist the reuse flag + rerere restore information into resolve state so
                // `hitch resolve --continue/--abort` can export and restore.
                let git_dir = context.git().get_git_dir();
                if let Ok(mut state) = crate::utils::resolve_state::read_resolve_state(&git_dir) {
                    state.reuse_resolutions = true;
                    state.rerere_restore = true;
                    state.rerere_original = rerere_original;
                    let _ = crate::utils::resolve_state::write_resolve_state(&git_dir, &state);
                }

                // Intentionally keep `rerere.enabled=true` while the user resolves conflicts.
                return Err(e);
            }

            if args.reuse_resolutions {
                let _ = restore_rerere_config(context, rerere_original);
            }
            return Err(e);
        }
    }

    context.log_success(&format!(
        "Environment '{}' rebuilt successfully!",
        args.env_name
    ));
    Ok(())
}

fn restore_rerere_config(context: &GlobalContext, original: Option<String>) -> Result<()> {
    match original {
        Some(v) => context.git().set_git_config("rerere.enabled", &v),
        None => context.git().unset_git_config("rerere.enabled"),
    }
}

/// Validate that environment exists and is not locked (unless force flag is used)
fn validate_environment_exists_and_unlocked(
    context: &GlobalContext,
    env_name: &str,
    force: bool,
) -> Result<()> {
    context.log_verbose("Validating environment preconditions...");

    // Check if environment exists
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    if !config.environments.contains_key(env_name) {
        return Err(anyhow::anyhow!("Environment '{}' does not exist", env_name));
    }

    let environment = &config.environments[env_name];

    // Check if environment is locked (unless force is used)
    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by {}. Use --force to override.",
            env_name,
            environment
                .locked_by
                .as_ref()
                .unwrap_or(&"unknown".to_string())
        ));
    }

    context.log_verbose(&format!("✓ Environment '{}' validation passed", env_name));
    Ok(())
}
