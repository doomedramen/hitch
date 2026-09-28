use crate::commands::global_context::GlobalContext;
use crate::core::render::{confirm_plan, emit_plan, emit_receipt, render_plan};
use crate::operations::declaration::{
    apply_declaration_plan, plan_promote, DeclarationPlanOptions,
};
use crate::operations::model::OperationOutcome;
use crate::types::{RollbackInfo, RollbackOperation};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PromoteCommand {
    /// The branch to promote (e.g., feature/login)
    #[arg()]
    pub branch: String,

    /// The environment to promote the branch to
    #[arg()]
    pub env_name: String,

    /// Skip the automatic rebuild after promotion.
    /// Use this to batch multiple promotes and then run 'hitch rebuild <env>' once.
    #[arg(long)]
    pub no_rebuild: bool,

    /// Show the plan — what would be declared, what would be rebuilt, which
    /// environments would follow — and stop. Changes nothing: no stash, no
    /// lock, no writes.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: PromoteCommand, context: &GlobalContext) -> Result<()> {
    // Step 1: Ensure we are in a Git repository
    crate::utils::prelude::pre_check_repo_only(context)?;

    // Step 2: The environment must exist, and a lock set by a *human* (`hitch lock`) is a refusal; the lock
    // this command takes for itself is not. So the check happens here, before
    // `with_locked_env`, which would otherwise see its own lock and mistake it
    // for someone else's. Same shape as `commands/rebuild.rs`.
    // Checked here, not in the planner: the planner runs inside the lock, and
    // `with_locked_env`'s own first act is to lock the environment — which
    // reports a missing one as "not found" rather than "does not exist".
    crate::utils::command_helpers::ensure_environment_exists(context, &args.env_name)?;
    let config =
        crate::utils::prelude::access_metadata_read_only(context, |config| Ok(config.clone()))?;
    if config
        .environments
        .get(&args.env_name)
        .is_some_and(|e| e.is_locked())
    {
        anyhow::bail!(
            "Environment '{}' is currently locked by '{}'",
            args.env_name,
            crate::utils::command_helpers::environment::get_locked_by_user(
                context,
                &args.env_name
            )?
        );
    }

    let options = DeclarationPlanOptions {
        no_rebuild: args.no_rebuild,
    };

    if args.dry_run {
        // Preview. Planned *outside* the environment lock, which inverts the
        // rule the real path below follows, and is safe for exactly one reason:
        // a preview plan is never applied, so nothing can go stale between
        // planning and applying it. The rule the real path follows is safe
        // because the planner does not consult `is_locked()` — which is why the
        // human-lock refusal above, and not the planner, enforces it.
        //
        // No `with_auto_stash` either. It stashes the working tree, so a
        // "changes nothing" flag that rearranged the user's uncommitted work
        // would not be describing itself honestly.
        let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut |_| {})?;
        emit_plan(context, &plan)?;
        return Ok(());
    }

    // Create rollback info for this operation
    let mut rollback_info = RollbackInfo::new(
        RollbackOperation::Promote,
        args.env_name.clone(),
        args.branch.clone(),
    );

    // Step 3: Plan, then apply, both under the environment lock.
    //
    // Planning *inside* the lock is not incidental, it is forced: `with_locked_env`
    // commits the lock to `hitch-metadata` before running its closure, so a plan
    // built outside it is stale the moment it is applied — every promotion would
    // refuse itself. Planning under the lock is also the right shape, because the
    // repo-wide lock is already held for the whole window (see `operations`'s
    // module header) and this narrows that to the environment being edited.
    //
    // The snapshot is captured *before* the lock and armed *inside* it, and both
    // halves of that are load-bearing.
    //
    // Captured before, because the rollback runs after `with_locked_env` has
    // already released the lock. A snapshot taken inside the closure records
    // `locked: true`, so restoring it put the lock *back* and left the
    // environment wedged — every subsequent promote refused with "Environment
    // 'dev' is currently locked by …", naming a lock holder that had gone away.
    // The comment this replaces argued that capturing before the lock "would
    // undo the lock's own commit along with the edit"; the rollback is a later
    // commit, not a history rewrite, so restoring the pre-lock value is what
    // leaves the environment correct. Nothing else writes `hitch-metadata` in the
    // window — the planner composes nothing and anchors nothing — so a
    // pre-lock snapshot has nothing stale in it.
    //
    // Armed inside, because a snapshot taken unconditionally is a snapshot of a
    // repository the operation never touched, and rolling back to it costs two
    // metadata commits to report a repair that did not happen. Every refusal is
    // decided before the apply is reached — a plan that went stale, a
    // sibling-conflict policy block, a declined confirmation, an environment
    // locked by a human — and each of them now leaves no trace at all.
    let snapshot = crate::utils::rollback::capture_config_state(context)?;
    let result = crate::utils::prelude::with_auto_stash(context, || {
        crate::utils::prelude::with_locked_env(context, &args.env_name, || {
            let plan = plan_promote(context, &args.branch, &args.env_name, options, &mut |_| {})?;
            if !confirm_plan(context, &render_plan(&plan), &plan.confirmation)? {
                return Ok((plan, None));
            }
            rollback_info.previous_config = snapshot;
            let receipt = apply_declaration_plan(context, &plan, &mut |_| {})?;
            Ok((plan, Some(receipt)))
        })
    });

    // Step 4: Handle result with automatic rollback on failure
    match result {
        Ok((plan, Some(receipt))) => {
            emit_receipt(context, &plan, &receipt)?;
            if receipt.outcome == OperationOutcome::ApprovalRequested {
                // The remedy is printed *here* and not by the plan, because the
                // plan cannot know it: the request is created by the apply, and a
                // planner that named a request id would be naming one that does
                // not exist yet. A refusal to promote with no next step is the
                // same dead end the held-branch remedy exists to avoid.
                context.log_info(
                    "Run 'hitch approvals list' to see the request, then 'hitch approvals approve <id>' to grant it.",
                );
            }
            Ok(())
        }
        // Declined. The gate already showed the plan and the answer, and nothing
        // was written, so there is nothing to report and nothing to roll back —
        // a "rolled back" line for a no-op is a scare, not an account. Exit 0:
        // declining is not a failure.
        Ok((_plan, None)) => Ok(()),
        Err(e) => {
            // The cause is *not* reported here. `main` prints it, once, as
            // `Error: …`; this arm used to print it first as `❌ Error: …` and
            // hand the same error back, so a refused promote put the identical
            // sentence on stderr twice under two different prefixes. Every other
            // command in the CLI lets `main` do it, and the rollback narration
            // below is not a loss — the error now reads as the explanation *for*
            // that narration rather than a surprise after it.
            //
            // Roll back only what the apply could have written. An unarmed
            // snapshot means the apply was never reached, so there is nothing to
            // undo.
            if rollback_info.previous_config.is_none() {
                return Err(e);
            }
            if let Err(rollback_err) =
                crate::utils::rollback::rollback_metadata_changes(context, &rollback_info)
            {
                context.log_error(&format!(
                    "CRITICAL: failed to roll back the declaration: {}. Manual intervention may be required.",
                    rollback_err
                ));
            }
            Err(e)
        }
    }
}
