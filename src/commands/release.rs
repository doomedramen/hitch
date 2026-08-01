use crate::commands::global_context::GlobalContext;
use crate::utils::command_helpers::{
    ensure_branch_exists, ensure_environment_exists, environment::get_locked_by_user,
    logging::validation_success,
};
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::{access_metadata_read_only, push_branch_with_deploy_key_if_configured};
use crate::utils::validation::validate_name;
use anyhow::Result;
use clap::Args;
use std::collections::{HashMap, HashSet};

#[derive(Args)]
pub struct ReleaseCommand {
    /// The name of the environment to release
    #[arg()]
    pub env_name: String,

    /// Target branch to merge to (overrides environment base branch)
    #[arg()]
    pub target_branch: Option<String>,

    /// Override the environment lock and approval requirements.
    /// This does NOT skip the confirmation prompt — use the global --yes for that.
    #[arg(long)]
    pub force: bool,

    /// Skip post-release pruning of promoted branches that are now integrated into their bases
    #[arg(long)]
    pub no_prune: bool,

    /// Skip rebuilding environments that depend on the released target branch (and any pruned envs)
    #[arg(long)]
    pub no_rebuild_dependents: bool,

    /// Use squash merges instead of merge commits (does NOT preserve ancestry for stacked branches;
    /// GitHub will NOT auto-detect merged PRs in squash mode — use --no-ff (default) for GitHub PR workflows)
    #[arg(long)]
    pub squash: bool,
}

pub fn run(args: ReleaseCommand, context: &GlobalContext) -> Result<()> {
    context.log_info(&format!("Releasing environment '{}'...", args.env_name));

    // Step 1: Precondition checks
    validate_preconditions(context, &args.env_name, args.force)?;

    // Step 2: Resolve target branch
    let target_branch = resolve_target_branch(context, &args.env_name, args.target_branch)?;

    // Step 3: User confirmation (skip with the global --yes)
    if !confirm_release(context, &args.env_name, &target_branch)? {
        context.log_info("Release cancelled by user.");
        return Ok(());
    }

    // Step 4-7: Execute release. In force mode the environment may already be
    // locked, so we don't take the environment lock; otherwise we lock it for the
    // duration of the release. The core logic is identical either way.
    if args.force {
        context.log_info(&format!(
            "Force releasing locked environment '{}' to '{}'...",
            args.env_name, target_branch
        ));
        perform_release_core(
            context,
            &args.env_name,
            &target_branch,
            args.no_prune,
            args.no_rebuild_dependents,
            args.squash,
        )?;
    } else {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            perform_release_core(
                context,
                &args.env_name,
                &target_branch,
                args.no_prune,
                args.no_rebuild_dependents,
                args.squash,
            )
        })?;
    }

    context.log_success(&format!(
        "Environment '{}' released successfully to '{}'!",
        args.env_name, target_branch
    ));
    Ok(())
}

/// Validate that environment exists and is ready for release
fn validate_preconditions(context: &GlobalContext, env_name: &str, force: bool) -> Result<()> {
    context.log_verbose("Running release validation...");

    // Basic pre-checks
    crate::utils::prelude::pre_check(context)?;

    // Validate environment name
    validate_name(env_name, "Environment")?;

    // Check environment exists
    ensure_environment_exists(context, env_name)?;

    // Check environment lock status (unless force)
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = &config.environments[env_name];

    if environment.is_locked() && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' is locked by {}. Use --force to override.",
            env_name,
            get_locked_by_user(context, env_name)?
        ));
    }

    // Releasing an approval-gated environment merges its promoted branches into a
    // real target/deploy branch. The per-promote approval workflow does not cover
    // release, so require an explicit --force to acknowledge that the release is
    // not itself approval-gated.
    if environment.requires_approval && !force {
        return Err(anyhow::anyhow!(
            "Environment '{}' requires approval. Releasing it merges its promoted branches \
             into the target branch and is NOT covered by the per-promote approval workflow.\n\
             Re-run with --force to release anyway.",
            env_name
        ));
    }

    validation_success(context, env_name, "Release validation");
    Ok(())
}

/// Resolve the target branch for release (use override or environment base)
fn resolve_target_branch(
    context: &GlobalContext,
    env_name: &str,
    target_override: Option<String>,
) -> Result<String> {
    context.log_verbose("Resolving target branch for release...");

    // Validate target branch name if provided as override
    if let Some(ref target) = target_override {
        validate_name(target, "Target branch")?;
    }

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    let target = target_override.unwrap_or_else(|| environment.base.clone());

    context.log_verbose(&format!("Target branch resolved to: '{}'", target));

    // Use the existing branch validation helper
    ensure_branch_exists(context, &target)?;

    Ok(target)
}

/// Core release logic shared by normal and forced modes.
///
/// Composes the promoted branches into the target branch inside a disposable
/// worktree so the user's working tree is never touched. Once all merges
/// succeed the target ref is advanced with a single CAS `update-ref`; on any
/// merge failure the worktree is discarded and the target branch is never
/// modified — no rollback needed.
fn perform_release_core(
    context: &GlobalContext,
    env_name: &str,
    target_branch: &str,
    no_prune: bool,
    no_rebuild_dependents: bool,
    squash: bool,
) -> Result<()> {
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    if environment.branches.is_empty() {
        context.log_info(&format!(
            "No branches promoted to environment '{}', nothing to release",
            env_name
        ));
        return Ok(());
    }

    let released_branches = environment.branches.clone();

    context.log_info(&format!(
        "Releasing {} promoted branches from environment '{}' to '{}'",
        environment.branches.len(),
        env_name,
        target_branch
    ));

    // Synchronize all branches
    context.log_info("Synchronizing branches for release...");
    let sync_branches = {
        let mut b = environment.branches.clone();
        b.push(target_branch.to_string());
        b
    };
    context.git().synchronize_branches(&sync_branches)?;

    // Pin promoted branch SHAs so merges use the snapshot we synced.
    let mut pinned_branches: Vec<(String, String)> = Vec::new();
    for branch in &environment.branches {
        if !context.git().branch_exists(branch)? {
            return Err(anyhow::anyhow!(
                "Branch '{}' does not exist locally after synchronization",
                branch
            ));
        }
        let sha = context.git().get_branch_commit_sha(branch)?;
        pinned_branches.push((branch.clone(), sha));
    }

    // Snapshot the target branch SHA before building — CAS old value.
    let target_sha = context.git().get_branch_commit_sha(target_branch)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();

    // Compose the merges entirely in the object database — see
    // `GitOperations::merge_tree_compose`. A release is all-or-nothing: any
    // conflict aborts before the target ref is touched, and since nothing was
    // ever checked out there is no merge state to unwind.
    let build_result = (|| -> Result<String> {
        let git = context.git();
        let mut composed = target_sha.clone();

        for (branch, sha) in &pinned_branches {
            context.log_info(&format!("Merging '{}' into '{}'...", branch, target_branch));
            let merge_message = format!(
                "Hitch: release {} from {} to {}",
                branch, env_name, target_branch
            );

            let outcome = git.merge_tree_compose(&composed, sha)?;
            if !outcome.conflicted_stages.is_empty() {
                let conflicted_files = outcome
                    .conflicted_stages
                    .iter()
                    .map(|(path, _, _, _)| path.clone())
                    .collect();
                return Err(build_conflict_error(
                    branch,
                    Some(conflicted_files),
                    target_branch,
                    env_name,
                ));
            }

            // Squash keeps the released branch's commits out of the target's
            // history; the default merge commit records the second parent so
            // ancestry is preserved and GitHub can detect merged PRs.
            let parents: Vec<&str> = if squash {
                vec![composed.as_str()]
            } else {
                vec![composed.as_str(), sha.as_str()]
            };

            let composed_tree = git.rev_parse(&format!("{}^{{tree}}", composed))?;
            if squash && outcome.tree_oid == composed_tree {
                // Nothing to record — the branch is already contained in the
                // target. A merge commit is still worth making in --no-ff mode
                // because it is what carries the ancestry link.
                context.log_verbose(&format!(
                    "'{}' is already contained in '{}'",
                    branch, target_branch
                ));
                continue;
            }

            composed = git.commit_tree(&outcome.tree_oid, &parents, &merge_message)?;
            context.log_verbose(&format!("✓ Merged '{}' into '{}'", branch, target_branch));
        }

        Ok(composed)
    })();

    let new_sha = build_result?;

    // Keep the composed commit reachable until the tag and the publish CAS
    // take over that job, so a concurrent `git gc` cannot collect it.
    let build_ref = format!("refs/hitch/release/{}/{}", target_branch, timestamp);
    context.git().update_ref(&build_ref, &new_sha)?;
    let cleanup = || {
        let _ = context.git().delete_ref(&build_ref);
    };

    // Create the release tag pointing at the composed commit.
    let now = chrono::Utc::now();
    let datetime_str = now.format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let target_branch_clean = target_branch.replace('/', "-");
    let tag_name = format!(
        "hitch-release-{}-to-{}-{}",
        env_name, target_branch_clean, datetime_str
    );
    let tag_message = format!(
        "Hitch release of environment '{}' to '{}' at {}",
        env_name,
        target_branch,
        now.format("%Y-%m-%d %H:%M:%S UTC")
    );

    let tag_name = create_release_tag(context.git(), &tag_name, &tag_message, &new_sha)?;
    context.log_info(&format!("✓ Created release tag '{}'", tag_name));

    // Publish the target branch atomically — see `prelude::publish_branch`.
    // Not versioned into `refs/hitch/prev`/`backup`: the release tag created
    // above already serves as this publish's rollback anchor. The build-ref
    // anchor is only dropped *after* publish is attempted (success or
    // failure) — see the matching `drop_build_ref()` pattern in
    // `rebuild_environment` — so the composed commit stays reachable for the
    // whole window until `refs/heads/<target_branch>` takes over that job.
    let retry_hint = format!("hitch release {}", env_name);
    // No `-f`: release's push is a plain fast-forward/merge, not a rewrite,
    // and this branch is typically `main`/`production` — exactly what
    // `hitch setup`'s branch-protection ruleset guards against direct
    // force pushes. See `publish_branch`'s doc comment on `push_remedy`.
    let push_remedy = format!("hitch push {}", target_branch);
    let publish_result = crate::utils::prelude::publish_branch(
        context,
        target_branch,
        &new_sha,
        None,
        &retry_hint,
        &push_remedy,
        || push_branch_with_deploy_key_if_configured(context, target_branch),
    );
    cleanup();
    publish_result?;

    // Push the release tag as its own best-effort step, separate from the
    // branch push above. The branch is already published at this point, so a
    // tag-push failure must not be reported through `publish_branch`'s
    // generic push-failure warning: that warning's remedy
    // (`hitch push <branch> -f`) never pushes tags, and its "recovery will
    // report it again" claim is false here — `publish_journal::recover()`
    // only compares the branch tip, which already matches, so a leftover
    // journal record tells it nothing about the tag.
    //
    // `publish_branch` swallows a branch-push failure (it warns and returns
    // `Ok(())` rather than propagating, since a failed push doesn't undo an
    // already-successful local publish) — so reaching this point does NOT
    // mean the branch push actually landed. Pushing the tag unconditionally
    // would publish `refs/tags/<tag_name>` pointing at a commit
    // `origin/<target_branch>` doesn't actually contain. Check the
    // remote-tracking ref instead: `push_branch_with_deploy_key_if_configured`
    // calls `record_pushed_tip` on success, which updates
    // `refs/remotes/origin/<target_branch>` specifically, so this comparison
    // is reliable.
    if context.should_push() {
        let branch_pushed = context
            .git()
            .rev_parse_opt(&format!("refs/remotes/origin/{}", target_branch))?
            .as_deref()
            == Some(new_sha.as_str());

        if branch_pushed {
            if let Err(e) = context.git().push_tag(&tag_name) {
                context.log_warning(&format!(
                    "Release published and pushed, but pushing the release tag '{}' failed: {}",
                    tag_name, e
                ));
                context.log_warning(&format!(
                    "Push the tag manually with: git push origin {}",
                    tag_name
                ));
            }
        } else {
            context.log_warning(&format!(
                "Skipping the release tag push for '{}' because the branch push did not land \
                 — push the branch first (see the earlier warning for how), then push the tag \
                 manually with: git push origin {}",
                tag_name, tag_name
            ));
        }
    }

    // Post-release pruning and dependent rebuilds.
    let pruned_envs =
        update_release_metadata_and_prune(context, env_name, &released_branches, !no_prune)?;

    if !no_rebuild_dependents {
        rebuild_dependent_environments(context, target_branch, env_name, &pruned_envs)?;
    }

    Ok(())
}

/// Build detailed conflict error message
fn build_conflict_error(
    branch: &str,
    conflicted_files: Option<Vec<String>>,
    target_branch: &str,
    env_name: &str,
) -> anyhow::Error {
    let mut error_msg = format!(
        "Merge conflict detected when releasing branch '{}' to '{}'",
        branch, target_branch
    );

    if let Some(files) = conflicted_files {
        if !files.is_empty() {
            error_msg.push_str("\n\nConflicting files:");
            for file in files {
                error_msg.push_str(&format!("\n  • {}", file));
            }
        }
    }

    error_msg.push_str("\n\nTo resolve this:");
    error_msg.push_str(&format!(
        "\n1. Check out target branch: git checkout {}",
        target_branch
    ));
    error_msg.push_str(&format!(
        "\n2. Manually merge '{}': git merge {}",
        branch, branch
    ));
    error_msg.push_str("\n3. Resolve conflicts and commit");
    error_msg.push_str("\n4. Try release again with the environment instead:");
    error_msg.push_str(&format!(
        "\n   hitch release {} {}",
        env_name, target_branch
    ));

    anyhow::anyhow!("{}", error_msg)
}

/// Create the release tag, tolerating a same-name collision from a
/// crash-then-immediate-retry of the same release.
///
/// `create_tag_at` is a hard `git tag -a` with no `-f`, and `tag_name` is
/// stamped at second granularity — two releases of the same env/target
/// landing in the same wall-clock second collide on the name. A collision
/// where the existing tag already points at `sha` is the retry case
/// (harmless: the retry recomposed the exact same commit) and is treated as
/// already-done. A collision pointing at anything else is a second,
/// genuinely distinct release landing in the same second; disambiguate with
/// a short random suffix rather than fail outright — silently dropping or
/// overwriting a real release's tag would be worse than a slightly uglier
/// name. Any other tag-creation failure (not a name collision at all) is
/// propagated unchanged. Returns the tag name actually created or reused.
fn create_release_tag(
    git: &GitOperations,
    tag_name: &str,
    message: &str,
    sha: &str,
) -> Result<String> {
    match git.create_tag_at(tag_name, message, sha) {
        Ok(()) => Ok(tag_name.to_string()),
        Err(e) => match git.rev_parse_opt(&format!("{}^{{commit}}", tag_name))? {
            Some(existing_sha) if existing_sha == sha => Ok(tag_name.to_string()),
            Some(_) => {
                let disambiguated = format!("{}-{}", tag_name, short_random_suffix());
                git.create_tag_at(&disambiguated, message, sha)?;
                Ok(disambiguated)
            }
            // The tag doesn't exist even after the failure, so this wasn't a
            // name collision at all — some other real failure. Propagate it.
            None => Err(e),
        },
    }
}

/// 8 hex characters of a fresh UUIDv4 — enough entropy to make a
/// disambiguated tag name practically unique without a second lookup.
fn short_random_suffix() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

/// Update the release timestamp and optionally prune promoted branches that are now integrated.
///
/// Returns the list of environments whose `branches` list changed due to pruning.
fn update_release_metadata_and_prune(
    context: &GlobalContext,
    released_env: &str,
    released_branches: &[String],
    do_prune: bool,
) -> Result<Vec<String>> {
    context.log_verbose("Updating release metadata...");

    let mut pruned_envs: HashSet<String> = HashSet::new();

    crate::utils::prelude::modify_metadata(context, |config| {
        let env = config
            .get_environment_mut(released_env)
            .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found", released_env))?;
        env.update_released_timestamp();

        if !do_prune || released_branches.is_empty() {
            return Ok(());
        }

        context.log_info("Post-release: pruning promoted branches now in their base...");

        for (env_name, environment) in config.environments.iter_mut() {
            // Don't mutate an environment locked by another operation/user. Its rebuild is
            // skipped too (see rebuild_dependent_environments), so pruning its metadata here
            // would leave the metadata and the built branch inconsistent — and it violates
            // the lock. Defer the prune until that environment is next rebuilt. The env being
            // released is locked by *this* release, so it is still pruned.
            if environment.is_locked() && env_name.as_str() != released_env {
                context.log_verbose(&format!(
                    "Skipping prune from '{}' because it is locked by another operation",
                    env_name
                ));
                continue;
            }

            for branch in released_branches {
                if !environment.branches.contains(branch) {
                    continue;
                }

                // Skip pruning when the base branch doesn't exist (local-only repos, deleted envs, etc.).
                let base_exists = match context.git().branch_exists_anywhere(&environment.base) {
                    Ok(v) => v,
                    Err(e) => {
                        context.log_warning(&format!(
                            "Could not check whether base '{}' exists (env '{}'): {}",
                            environment.base, env_name, e
                        ));
                        false
                    }
                };
                if !base_exists {
                    context.log_warning(&format!(
                        "Skipping prune of '{}' from '{}' because base '{}' does not exist",
                        branch, env_name, environment.base
                    ));
                    continue;
                }

                let branch_exists = match context.git().branch_exists_anywhere(branch) {
                    Ok(v) => v,
                    Err(e) => {
                        context.log_warning(&format!(
                            "Could not check whether branch '{}' exists (env '{}'): {}",
                            branch, env_name, e
                        ));
                        false
                    }
                };
                if !branch_exists {
                    context.log_warning(&format!(
                        "Skipping prune of '{}' from '{}' because the branch no longer exists",
                        branch, env_name
                    ));
                    continue;
                }

                // If the promoted branch is now integrated into the base, remove it from metadata.
                if context
                    .git()
                    .is_branch_merged_into(branch, &environment.base)
                    .unwrap_or(false)
                {
                    environment.remove_branch(branch);
                    pruned_envs.insert(env_name.clone());
                }
            }
        }

        Ok(())
    })?;

    let mut out: Vec<String> = pruned_envs.into_iter().collect();
    out.sort();
    if !out.is_empty() {
        context.log_info(&format!(
            "Post-release: pruned promoted branches from {} environment(s): {}",
            out.len(),
            out.join(", ")
        ));
    }
    Ok(out)
}

/// Rebuild environments affected by a base move and any environments that were pruned.
///
/// This is best-effort: rebuild failures are reported as warnings but do not fail the release
/// (the release merge/tag has already been applied at this point).
fn rebuild_dependent_environments(
    context: &GlobalContext,
    target_branch: &str,
    released_env: &str,
    pruned_envs: &[String],
) -> Result<()> {
    use crate::utils::prelude::preflight_compatibility_merge_tree;

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    if config.environments.is_empty() {
        return Ok(());
    }

    let pruned_set: HashSet<&str> = pruned_envs.iter().map(|s| s.as_str()).collect();

    // Direct rebuild candidates:
    // - envs whose base is the released target branch (base moved)
    // - envs whose metadata was pruned (so env branch matches the updated list)
    let mut rebuild_set: HashSet<String> = HashSet::new();
    for (env_name, env) in &config.environments {
        if env.base == target_branch || pruned_set.contains(env_name.as_str()) {
            rebuild_set.insert(env_name.clone());
        }
    }

    if rebuild_set.is_empty() {
        return Ok(());
    }

    // Add transitive dependents where base is another environment name.
    loop {
        let before = rebuild_set.len();
        for (env_name, env) in &config.environments {
            if rebuild_set.contains(&env.base) {
                rebuild_set.insert(env_name.clone());
            }
        }
        if rebuild_set.len() == before {
            break;
        }
    }

    // Topological-ish order: base environments first, then dependents.
    let ordered = topological_environment_order(&config);

    context.log_info(&format!(
        "Post-release: rebuilding {} affected environment(s)...",
        rebuild_set.len()
    ));

    // Track whether each rebuilt env succeeded so we can skip dependents when the base wasn't rebuilt.
    let mut rebuild_ok: HashMap<String, bool> = HashMap::new();

    for env_name in ordered {
        if !rebuild_set.contains(&env_name) {
            continue;
        }

        let env = match config.environments.get(&env_name) {
            Some(e) => e,
            None => continue,
        };

        // The just-released environment is a special case: in the non-force path it is
        // still locked by *this* release (via the outer `with_locked_env`), so we must
        // NOT treat that lock as a reason to skip it — otherwise its metadata gets pruned
        // but its branch is never rebuilt, leaving it stale against the new base.
        let is_released_env = env_name == released_env;

        // If this env depends on another env we intended to rebuild, and that rebuild failed/skipped,
        // skip this one too so we don't rebuild on a stale base.
        if rebuild_set.contains(&env.base) && rebuild_ok.get(&env.base) != Some(&true) {
            context.log_warning(&format!(
                "Skipping rebuild of '{}' because its base environment '{}' was not rebuilt successfully",
                env_name, env.base
            ));
            rebuild_ok.insert(env_name.clone(), false);
            continue;
        }

        // Skip environments locked by *someone else*. The released environment is
        // (expected to be) locked by this release itself, so we rebuild it regardless.
        if env.is_locked() && !is_released_env {
            context.log_warning(&format!(
                "Skipping rebuild of '{}' because it is locked",
                env_name
            ));
            rebuild_ok.insert(env_name.clone(), false);
            continue;
        }

        // Preflight using the same merge-tree compatibility check as `hitch rebuild`.
        if let Some(failure) =
            preflight_compatibility_merge_tree(context, &env.base, &env.branches)?
        {
            context.log_warning(&format!(
                "Cannot rebuild '{}' — compatibility check failed when merging '{}' onto '{}'",
                env_name, failure.blocking_branch, failure.base_branch
            ));
            for f in &failure.conflicted_files {
                context.log_warning(&format!("  {}", f));
            }
            rebuild_ok.insert(env_name.clone(), false);
            continue;
        }

        // The released environment is already locked by this release (non-force) or was
        // force-released; rebuild it directly instead of taking a nested `with_locked_env`,
        // which would prematurely unlock it out from under the outer release lock. All
        // other environments take their own lock for the duration of their rebuild.
        let rebuild_res = if is_released_env {
            crate::utils::prelude::rebuild_environment(context, &env_name)
        } else {
            crate::utils::prelude::with_locked_env(context, &env_name, || {
                crate::utils::prelude::rebuild_environment(context, &env_name)
            })
        };

        match rebuild_res {
            Ok(outcome) => {
                if outcome.held.is_empty() {
                    context.log_success(&format!("✓ Rebuilt '{}'", env_name));
                } else {
                    context.log_warning(&format!(
                        "✓ Rebuilt '{}' with {} branch{} held: {}",
                        env_name,
                        outcome.held.len(),
                        if outcome.held.len() == 1 { "" } else { "es" },
                        outcome
                            .held
                            .iter()
                            .map(|c| c.branch.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                rebuild_ok.insert(env_name.clone(), true);
            }
            Err(e) => {
                context.log_warning(&format!("Failed to rebuild '{}': {}", env_name, e));
                rebuild_ok.insert(env_name.clone(), false);
            }
        }
    }

    Ok(())
}

fn topological_environment_order(config: &crate::types::HitchConfig) -> Vec<String> {
    // Edge: base_env -> env_name when env.base matches another environment name.
    let env_names: HashSet<String> = config.environments.keys().cloned().collect();
    let mut indegree: HashMap<String, usize> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();

    for name in &env_names {
        indegree.insert(name.clone(), 0);
    }

    for (env_name, env) in &config.environments {
        if env_names.contains(&env.base) {
            *indegree.entry(env_name.clone()).or_insert(0) += 1;
            children
                .entry(env.base.clone())
                .or_default()
                .push(env_name.clone());
        }
    }

    // Kahn's algorithm with a min-heap keyed by name, so ready nodes are emitted
    // in alphabetical order without the previous O(n²) `remove(0)` + re-`sort()`.
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    let mut queue: BinaryHeap<Reverse<String>> = indegree
        .iter()
        .filter_map(|(k, &d)| {
            if d == 0 {
                Some(Reverse(k.clone()))
            } else {
                None
            }
        })
        .collect();

    let mut out: Vec<String> = Vec::with_capacity(env_names.len());
    while let Some(Reverse(node)) = queue.pop() {
        if let Some(kids) = children.get(&node) {
            for kid in kids {
                if let Some(d) = indegree.get_mut(kid) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push(Reverse(kid.clone()));
                    }
                }
            }
        }
        out.push(node);
    }

    // If there was a cycle (should be rare), fall back to a stable alphabetical order.
    if out.len() != env_names.len() {
        let mut stable: Vec<String> = env_names.into_iter().collect();
        stable.sort();
        return stable;
    }

    out
}

/// Confirm release operation with user.
///
/// Returns `Ok(true)` if the user confirmed, `Ok(false)` if they declined.
fn confirm_release(context: &GlobalContext, env_name: &str, target_branch: &str) -> Result<bool> {
    // Get environment details to show user what will be released
    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;
    let environment = config.environments.get(env_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Environment '{}' does not exist. Available environments: {}",
            env_name,
            config.get_environment_names().join(", ")
        )
    })?;

    context.log_info("🚨 DANGEROUS OPERATION DETECTED!");
    context.log_info(&format!(
        "About to release environment '{}' to '{}'",
        env_name, target_branch
    ));
    context.log_info(&format!(
        "  • {} promoted branches will be merged",
        environment.branches.len()
    ));

    if environment.branches.is_empty() {
        context.log_info("  • No branches currently promoted (empty release)");
    } else {
        context.log_info("  • Branches to be merged:");
        for branch in &environment.branches {
            context.log_info(&format!("    - {}", branch));
        }
    }

    context.log_info(&format!("  • Target branch: {}", target_branch));
    context.log_info("  • This will merge changes permanently");

    if environment.is_locked() {
        context.log_warning("  • Environment is currently locked");
    }

    // Prompt for confirmation
    if !context.confirm("Do you want to continue?")? {
        return Ok(false);
    }

    context.log_info("User confirmed release - proceeding...");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::git_operations::GitOperations;

    /// A scratch git repo for tests exercising raw git plumbing directly —
    /// no hitch metadata needed. Mirrors the raw-git test helpers already
    /// established at `src/utils/prelude.rs:1430`/`:1989`.
    #[allow(clippy::disallowed_methods)] // test-only scratch repo bootstrap, same rationale as prelude.rs's raw-git test helpers
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_scratch_repo() -> (tempfile::TempDir, GitOperations) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.email", "test@test"]);
        git(repo, &["config", "user.name", "test"]);
        std::fs::write(repo.join("f.txt"), "one").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "one"]);
        let git_ops = GitOperations::new_at_path(&repo.to_string_lossy()).unwrap();
        (dir, git_ops)
    }

    /// A retried release (e.g. a crashed release re-run immediately, before
    /// the wall-clock second advances) regenerates the exact same composed
    /// commit and, with it, the exact same tag name. That must succeed and
    /// reuse the existing tag, not fail with "tag already exists" — this is
    /// the bug this function exists to fix.
    #[test]
    fn create_release_tag_reuses_existing_tag_for_same_content() {
        let (_dir, git_ops) = init_scratch_repo();
        let sha = git_ops.rev_parse("HEAD").unwrap();
        let tag_name = "hitch-release-dev-to-main-2026-08-01T00-00-00Z";

        let first = create_release_tag(&git_ops, tag_name, "msg", &sha).unwrap();
        assert_eq!(first, tag_name);

        let second = create_release_tag(&git_ops, tag_name, "msg", &sha).unwrap();
        assert_eq!(
            second, tag_name,
            "a same-content retry must reuse the existing tag name, not fail or rename"
        );
    }

    /// Two genuinely distinct releases (different composed commits) that
    /// happen to land in the same wall-clock second must both succeed, with
    /// distinct tags — neither silently dropped nor overwritten.
    #[test]
    fn create_release_tag_disambiguates_a_genuine_second_release_in_the_same_second() {
        let (dir, git_ops) = init_scratch_repo();
        let sha1 = git_ops.rev_parse("HEAD").unwrap();

        std::fs::write(dir.path().join("f.txt"), "two").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "two"]);
        let sha2 = git_ops.rev_parse("HEAD").unwrap();
        assert_ne!(sha1, sha2);

        let tag_name = "hitch-release-dev-to-main-2026-08-01T00-00-00Z";
        let first = create_release_tag(&git_ops, tag_name, "msg", &sha1).unwrap();
        assert_eq!(first, tag_name);

        let second = create_release_tag(&git_ops, tag_name, "msg", &sha2).unwrap();
        assert_ne!(
            second, tag_name,
            "a second, genuinely different release colliding on the same name must get a \
             distinct tag, not silently fail or overwrite the first"
        );
        assert!(
            second.starts_with(tag_name),
            "the disambiguated name should still be recognizable as this release's tag"
        );

        // Both tags must survive, pointing at their own respective commits.
        assert_eq!(
            git_ops
                .rev_parse(&format!("{}^{{commit}}", tag_name))
                .unwrap(),
            sha1,
            "the first release's tag must be untouched by the second release's collision"
        );
        assert_eq!(
            git_ops
                .rev_parse(&format!("{}^{{commit}}", second))
                .unwrap(),
            sha2
        );
    }
}
