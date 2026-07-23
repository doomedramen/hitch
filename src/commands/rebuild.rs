use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{
    access_metadata_read_only, preflight_compatibility_report, with_locked_env,
    CompatibilityConflict,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RebuildCommand {
    /// The name of the environment to rebuild
    #[arg()]
    pub env_name: String,

    /// Force rebuild even if environment is locked
    #[arg(long)]
    pub force: bool,

    /// Show every branch that would compose cleanly and every branch that
    /// would conflict (and with what), without building, locking, or
    /// publishing anything
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: RebuildCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Rebuilding environment '{}'...", args.env_name));

    // Step 1: Precondition checks (require clean working tree)
    crate::utils::prelude::pre_check(context)?;
    validate_environment_exists_and_unlocked(context, &args.env_name, args.force)?;

    // Step 2: Compatibility preflight (read-only; must happen before locking or creating temp branches)
    let (base_branch, promoted_branches) = access_metadata_read_only(context, |config| {
        let env = config
            .environments
            .get(&args.env_name)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' does not exist", args.env_name))?;
        Ok((env.base.clone(), env.branches.clone()))
    })?;

    context.log_info(&format!(
        "Checking compatibility of {} promoted branches...",
        promoted_branches.len()
    ));

    let conflicts = preflight_compatibility_report(context, &base_branch, &promoted_branches)?;

    if args.dry_run {
        if conflicts.is_empty() {
            context.log_success(&format!(
                "'{}' would rebuild cleanly ({} branch{}).",
                args.env_name,
                promoted_branches.len(),
                if promoted_branches.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            return Ok(());
        }
        return Err(anyhow::anyhow!(format_compatibility_report_for_rebuild(
            &args.env_name,
            &conflicts
        )));
    }

    if !conflicts.is_empty() {
        return Err(anyhow::anyhow!(format_compatibility_report_for_rebuild(
            &args.env_name,
            &conflicts
        )));
    }

    // Step 3: Execute rebuild (now we know it can assemble cleanly)
    if args.force {
        context.log_info(&format!(
            "Force rebuilding locked environment '{}'...",
            args.env_name
        ));
        crate::utils::prelude::rebuild_environment(context, &args.env_name)?;
    } else {
        with_locked_env(context, &args.env_name, || {
            crate::utils::prelude::rebuild_environment(context, &args.env_name)
        })?;
    }

    context.log_success(&format!(
        "Environment '{}' rebuilt successfully!",
        args.env_name
    ));
    Ok(())
}

fn format_compatibility_report_for_rebuild(
    env_name: &str,
    conflicts: &[CompatibilityConflict],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "✗ Cannot rebuild '{}' — compatibility check failed\n\n",
        env_name
    ));

    for c in conflicts {
        out.push_str(&format!(
            "  {} conflicts with {}\n",
            c.branch, c.conflicts_with
        ));
        for f in &c.conflicted_files {
            out.push_str(&format!("    {}\n", f));
        }
        out.push('\n');
    }

    if let [only] = conflicts {
        out.push_str(&format!("Fix {} first:\n", only.branch));
        out.push_str(&format!(
            "  git checkout {} && git rebase {}\n",
            only.branch, only.conflicts_with
        ));
    } else {
        out.push_str(&format!(
            "Fix each of the {} branches above, then retry:\n",
            conflicts.len()
        ));
        for c in conflicts {
            out.push_str(&format!(
                "  git checkout {} && git rebase {}\n",
                c.branch, c.conflicts_with
            ));
        }
    }

    out
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
