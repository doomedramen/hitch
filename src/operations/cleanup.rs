//! The prune sweep: the one operation whose effects are *deletions*.
//!
//! Everything else in this directory edits a declaration or a branch tip, and
//! a plan for one of those can be re-derived from live state if it goes
//! wrong. A cleanup plan cannot be re-derived — the refs it is about to delete
//! are gone — so the plan has to be the whole statement of what was found, and
//! the receipt has to be the whole statement of what was actually removed.
//! Those are two different statements, and `OperationIntent::Cleanup` says so:
//! its `candidates` are "what the sweep found", and a delete can still fail at
//! apply time (a stale ref lock, say). A branch that is not merged is *not* such
//! a failure: the planner asks git's own question and keeps it, so the plan never
//! promises a deletion `git branch -d` will refuse.
//!
//! ## What moved here, and why it moved rather than being copied
//!
//! The candidate rules, the `hitch-tmp-` exclusion, the reserved-name set and
//! [`ARCHIVE_REF_RETENTION`] are moved from `commands/cleanup.rs` *verbatim*.
//! This is the second place the `state/` exclusion has nearly been wrong — the
//! first was when `state/` was added to the prunable set by an edit to this
//! list. A cleanup whose prunable set exists in two places is a cleanup whose
//! prunable set can disagree with itself, and the disagreement is always in
//! the direction of deleting something live.
//!
//! ## Two orderings, both load-bearing
//!
//! The plan is a *decision*, so two cleanups of the same repository must
//! produce the same plan — a plan whose effect list reorders between runs is
//! not a plan, and a receipt that reorders is not a record. Neither list was
//! ordered before it came here:
//!
//! - **Branches** are sorted by name. A branch list has no temporal meaning —
//!   these are the branches nobody promoted, not the ones most recently
//!   demoted — so name order is the only stable one, and it is the order a
//!   reader scanning the list expects anyway.
//! - **Archive refs** keep `stale_archive_refs`' own order, which is
//!   namespace-major, then environment, then *newest-first among the stale
//!   ones* (they are produced by `rev().skip(RETENTION)`). Environments are
//!   sorted by name inside each namespace, which they were not: the input was
//!   `HitchConfig::environments`, a `HashMap`, so the same repository produced
//!   a different plan on a different run. The timestamp segment is fixed-width,
//!   so the chronological part needs no date lookup — see
//!   [`stale_archive_refs`].

use crate::commands::global_context::GlobalContext;
use crate::utils::prelude::{access_metadata_read_only, pre_check_repo_only};
use anyhow::Result;

use super::model::{
    changed_inputs, ConfirmationRequirement, ExecutionReceipt, OperationKind, OperationOutcome,
    PlanApplyError, PlanFingerprint, PlanWarning, PlannedEffect, UnaffectedResource,
};
use super::model::{AppliedEffect, OperationIntent, OperationPlan, ResourceKind};
use crate::core::render::ApplyFailure;

/// How many of the most recent archive refs to keep per (namespace,
/// environment). Chosen to comfortably cover manual rollback while bounding
/// unconditional growth — see Task 10 in the production-hardening plan for
/// the reasoning against an age-based alternative.
pub const ARCHIVE_REF_RETENTION: usize = 10;

/// The namespaces under `refs/hitch/` that this sweep prunes.
///
/// `state` is absent from this list and that absence is the single most
/// important line in the file. `prev`/`backup` are archives (overwritten and
/// pruned by retention), `build`/`publish`/`resolutions` are transient
/// (hitch's bookkeeping, cleared by the operation that made them), and
/// `state` is a **live pointer**: exactly one record per environment,
/// overwritten in place, whose entire job is to be the current answer to
/// "what is in this environment branch". It looks like its neighbours because
/// they share the `refs/hitch/` root; it is not one of them. Deleting it as a
/// stale artefact silently degrades every verdict hitch reports from
/// `LegacyUnknown` back to inference.
const PRUNABLE_NAMESPACES: [&str; 2] = ["backup", "prev"];

/// The per-invocation bookkeeping a cleanup plan carries.
///
/// The two candidate lists are kept apart rather than merged because they are
/// deleted by two different mechanisms: a branch goes through
/// `git branch -d`, which *refuses* an unmerged branch, and an archive ref
/// goes through `update-ref -d`, which cannot refuse. The plan cannot predict
/// which branch deletes will fail (see [`apply_cleanup_plan`]), so the
/// executor has to know which list a ref came from to give the right remedy.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlanDetail {
    /// The `--env` filter, or `None` for "every environment".
    pub environment_filter: Option<String>,
    /// Local branches that are not promoted, not reserved, not the current
    /// branch, and that `git branch -d` will accept. Name order.
    pub branches: Vec<String>,
    /// Branches that passed every filter above but are not merged — into their
    /// upstream, or into `HEAD` — so `git branch -d` would refuse them and hitch
    /// will not force it: deleting unmerged work is data loss. Kept, and named in
    /// the plan so the reader knows why they are not on the list. Name order.
    pub unmerged: Vec<String>,
    /// Branches checked out in another worktree, with where. Kept whether or not
    /// they are merged: a branch someone is standing on is not stale, and git
    /// (or worse, a forced delete) would pull it out from under them.
    pub checked_out: Vec<CheckedOutBranch>,
    /// Archive refs beyond [`ARCHIVE_REF_RETENTION`], in
    /// [`stale_archive_refs`]'s order.
    pub refs: Vec<String>,
}

/// A candidate branch that is checked out in a worktree.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CheckedOutBranch {
    pub branch: String,
    pub path: String,
}

impl CleanupPlanDetail {
    /// Whether this sweep has anything to do.
    pub fn is_empty(&self) -> bool {
        self.branches.is_empty()
            && self.refs.is_empty()
            && self.unmerged.is_empty()
            && self.checked_out.is_empty()
    }

    /// Whether the sweep will delete anything at all.
    pub fn has_deletions(&self) -> bool {
        !self.branches.is_empty() || !self.refs.is_empty()
    }
}

/// Find the sweep's candidates, and the decision to delete them.
///
/// Writes nothing. Not "almost nothing" — nothing at all, which is what makes
/// `hitch cleanup` without `--apply` a genuine preview rather than a preview
/// that leaves a lock behind. (The same reason a metadata plan anchors no
/// commit: there is nothing to anchor.)
pub fn plan_cleanup(
    context: &GlobalContext,
    env_filter: Option<&str>,
) -> Result<OperationPlan<CleanupPlanDetail>> {
    // A cleanup touches no metadata, so it needs only the repository — and
    // saying so here rather than at the call site is what keeps the command to
    // argument parsing.
    pre_check_repo_only(context)?;

    let config = access_metadata_read_only(context, |config| Ok(config.clone()))?;

    // Everything promoted *in scope* is protected. Everything that is a base
    // is protected *out of scope* too: `--env dev` narrows which promoted
    // branches hold a branch back, and it has never narrowed which bases are
    // untouchable — `hitch cleanup --env dev` that deleted the base of `qa`
    // would be a catastrophe scoped by a flag whose name says nothing about
    // other environments.
    // Everything promoted is protected, and it is protected *globally* — the
    // `--env` filter does not narrow it. That is a change from the rule this
    // file used to carry, and the reason is the rule two lines down: a flag
    // whose name is one environment's says nothing about the others, and a
    // branch promoted into `dev` is not scratch work just because the reader
    // typed `--env qa`. Deleting it leaves `dev`'s declaration naming a ref that
    // no longer exists, and the next rebuild of `dev` fails on it.
    //
    // What `--env` still scopes is this environment's *archive* refs, which is
    // the half of the sweep where an environment genuinely is the unit of
    // retention. One flag, one meaning: a name is protected because the whole
    // declaration says so, and archives are pruned per environment.
    let mut promoted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reserved: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (env_name, environment) in &config.environments {
        reserved.insert(environment.base.clone());
        // The environment's *own* branch, and this line is a bug fix rather
        // than a move. `dev` is not a base branch and is not promoted into
        // anything, so the filter chain that lived in `commands/cleanup.rs`
        // offered to delete it — `hitch cleanup --apply` on a repository with
        // one built environment destroyed the build that
        // `refs/hitch/state/dev` then claimed to describe. It is recoverable (a
        // rebuild restores the same tree) and it is still data loss, and it
        // survived every existing test because each of them asserted the
        // presence or absence of one *feature* branch and none of them looked
        // at what the sweep also found.
        //
        // It is the same exclusion as the `state/` namespace above, one level
        // up: the branch is hitch's own build output and the state ref is the
        // live pointer to it. Neither is an artefact, so neither is prunable,
        // and the two belong next to each other for that reason.
        reserved.insert(env_name.clone());
        for b in &environment.branches {
            promoted.insert(b.clone());
        }
    }
    // Built-in reserved names.
    reserved.insert("hitch-metadata".to_string());

    let all_local = context.git().list_local_branches_with_prefix("")?;
    let current = context.git().get_current_branch().unwrap_or_default();

    // Candidates: local branches that are not currently promoted (in scope),
    // not a reserved/base branch, not the current branch, and not a hitch
    // internal branch.
    let mut branches: Vec<String> = all_local
        .into_iter()
        .filter(|b| {
            !promoted.contains(b)
                && !reserved.contains(b)
                && b != &current
                && !b.starts_with("hitch-tmp-")
                && b != "hitch-metadata"
        })
        .collect();
    // See the module header: a branch list has no temporal meaning, so name
    // order is the only ordering that is the same on every run.
    branches.sort();

    // A branch checked out in any worktree is set aside before the merge test:
    // the main checkout's branch is `current` above, and every *other* checkout
    // is someone's working state. Kept on purpose, not for want of a merge.
    let worktrees = context.git().list_worktrees()?;
    let mut checked_out: Vec<CheckedOutBranch> = Vec::new();
    branches.retain(|b| {
        match worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(b.as_str()))
        {
            Some(w) => {
                checked_out.push(CheckedOutBranch {
                    branch: b.clone(),
                    path: w.path.clone(),
                });
                false
            }
            None => true,
        }
    });

    // The predicate is git's own — see `GitOperations::branch_is_merged` — so
    // the plan never promises a deletion `git branch -d` will refuse. The
    // reference it measures against (the upstream, else `HEAD`) is tracked in
    // the fingerprint below, which is what makes this a prediction the
    // validator can check rather than one it cannot.
    let mut unmerged: Vec<String> = Vec::new();
    let mut references: Vec<String> = vec!["HEAD".to_string()];
    let mut deletable: Vec<String> = Vec::new();
    for branch in branches {
        let reference = context.git().delete_reference_for(&branch)?;
        if !references.contains(&reference) {
            references.push(reference);
        }
        if context.git().branch_is_merged(&branch)? {
            deletable.push(branch);
        } else {
            unmerged.push(branch);
        }
    }
    let branches = deletable;

    let envs = envs_in_scope(&config, env_filter);
    let refs = stale_archive_refs(context, &envs)?;

    // Every ref the sweep is about to touch is an *input* to the decision, and
    // so is the declaration — "is this branch promoted" is a declaration
    // question, and a plan that outlived a promote into the scope would find
    // itself deleting a branch that had just been promoted.
    //
    // What is deliberately *not* tracked is a branch that appears after the
    // plan was built: `changed_inputs` only compares refs the plan read, and a
    // new branch is not read. That is the same asymmetry every other operation
    // has (adding an unrelated branch does not invalidate anything), and here
    // it fails safe — a missed new branch is an under-delete, not a delete of
    // something live.
    let mut fingerprint = PlanFingerprint::new();
    fingerprint.metadata_sha = context.git().rev_parse_opt("refs/heads/hitch-metadata")?;
    for name in branches.iter().chain(refs.iter()).chain(unmerged.iter()) {
        let refname = fully_qualified(name);
        if let Some(sha) = context.git().rev_parse_opt(&refname)? {
            fingerprint.track_ref(refname, sha);
        }
    }
    // What "merged" was measured against. A plan that outlived a commit on
    // `HEAD` could otherwise delete a branch that had stopped being merged.
    for reference in &references {
        if let Some(sha) = context.git().rev_parse_opt(reference)? {
            fingerprint.track_ref(reference.clone(), sha);
        }
    }

    let effects: Vec<PlannedEffect> = branches
        .iter()
        .chain(refs.iter())
        .map(|name| PlannedEffect::LocalRefDelete {
            refname: fully_qualified(name),
        })
        .collect();

    let found = branches.len() + refs.len();
    let mut warnings = Vec::new();
    if !unmerged.is_empty() {
        warnings.push(PlanWarning::advisory(format!(
            "Kept: {} — not merged into HEAD or its upstream, so git would refuse to \
             delete {}. To remove {} anyway: {}",
            unmerged.join(", "),
            if unmerged.len() == 1 { "it" } else { "them" },
            if unmerged.len() == 1 { "it" } else { "one" },
            unmerged
                .iter()
                .map(|b| format!("`git branch -D {b}`"))
                .collect::<Vec<_>>()
                .join(" or "),
        )));
    }
    if !checked_out.is_empty() {
        warnings.push(PlanWarning::advisory(format!(
            "Kept, checked out in another worktree: {}",
            checked_out
                .iter()
                .map(|c| format!("{} ({})", c.branch, c.path))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let confirmation = if found == 0 {
        ConfirmationRequirement::not_required()
    } else {
        // Any non-empty delete asks, not just a large one. The plan's reason
        // for wanting `--yes` is a sweep that removes forty branches, but the
        // threshold that would let a one-branch sweep through unasked would be
        // arbitrary, and "destructive" does not have a count at which it stops
        // being. `--yes` is the answer, and it is what every other destructive
        // operation in the CLI already takes.
        ConfirmationRequirement::required(format!(
            "this deletes {} item{} that hitch does not own",
            found,
            if found == 1 { "" } else { "s" }
        ))
    };

    let plan = OperationPlan {
        id: String::new(),
        kind: OperationKind::Cleanup,
        intent: OperationIntent::Cleanup {
            candidates: branches
                .iter()
                .chain(refs.iter())
                .cloned()
                .collect::<Vec<String>>(),
        },
        fingerprint,
        // Neither side projects an environment, and that is a claim rather than
        // a gap: a cleanup composes nothing and changes no declaration. It
        // *can* change what `hitch status` reports — deleting a stale
        // environment branch is a `MissingBranch` that stops being one — which
        // is why the receipt still carries a `resulting_state`.
        current: None,
        proposed: None,
        compositions: Vec::new(),
        effects,
        // The promoted branches, because "why wasn't my branch deleted?" is the
        // first question anyone has about a cleanup, and the answer is not
        // visible anywhere else in the document.
        //
        // Sorted, because `promoted` is a `HashSet` and an unordered
        // "Will not change" list is a plan that renders differently on every
        // run — the same defect the archive-ref enumeration had.
        unaffected: {
            let mut names: Vec<String> = promoted.iter().cloned().collect();
            names.sort();
            names
                .into_iter()
                .map(|name| UnaffectedResource {
                    kind: ResourceKind::Branch,
                    name,
                })
                .collect()
        },
        warnings,
        confirmation,
        detail: CleanupPlanDetail {
            environment_filter: env_filter.map(str::to_string),
            branches,
            unmerged,
            checked_out,
            refs,
        },
    };

    let digest = plan.fingerprint.digest(context.git())?;
    let mut plan = plan;
    plan.id = format!(
        "{}:cleanup:{}:{}",
        plan.kind,
        plan.detail.environment_filter.as_deref().unwrap_or("all"),
        digest
    );
    Ok(plan)
}

/// What an apply did: the receipt of what was removed, and the deletes that
/// failed.
///
/// Two fields rather than one receipt because a failed delete is neither of the
/// things a receipt can say. It is not an applied effect, and it is not an
/// *owed* one — `Still owed` promises a follow-up, and nothing in hitch retries
/// a cleanup ref, so the promise would be false. The command prints the receipt
/// (what did apply) and then fails with `failures`.
pub struct CleanupRun {
    pub receipt: ExecutionReceipt,
    /// One entry per delete git refused *at apply time*. The
    /// plan already declines what git would refuse for want of a merge, so
    /// what lands here is what it could not foresee — a stale ref lock, a
    /// branch that changed after planning.
    pub failures: Vec<ApplyFailure>,
}

/// Delete what the plan found, and report exactly what that came to.
///
/// Every delete is attempted; a failure does not stop the sweep, because
/// stopping at the first would leave the rest of a valid plan undone for one
/// bad ref. The failures are collected in [`CleanupRun::failures`] rather than
/// raised, so the caller can print the receipt of what *did* apply before it
/// fails — an `Err` from in here would discard that record.
///
/// Whether a branch is *merged* is decided by the plan, not here: the planner
/// asks git's own question and leaves an unmerged branch out, so a
/// `git branch -d` refusal is no longer an expected outcome that needs a
/// receipt section of its own.
pub fn apply_cleanup_plan(
    context: &GlobalContext,
    plan: &OperationPlan<CleanupPlanDetail>,
) -> Result<CleanupRun> {
    let started_at = chrono::Utc::now();
    validate_cleanup_plan(context, plan).map_err(PlanApplyError::into_anyhow)?;

    if let Some(blocking) = plan.blocked_by() {
        return Err(PlanApplyError::PolicyBlocked {
            environment: plan
                .detail
                .environment_filter
                .clone()
                .unwrap_or_else(|| "all".to_string()),
            reason: blocking.message.clone(),
            remedy: blocking.remedy_for(&plan.kind.command_hint("cleanup", "")),
        }
        .into_anyhow());
    }

    let detail = plan.detail.clone();
    // Also the case where the sweep kept an unmerged branch and found nothing
    // else: there is nothing to write, and saying so is `NoChange`.
    if !detail.has_deletions() {
        return Ok(CleanupRun {
            receipt: ExecutionReceipt {
                plan_id: plan.id.clone(),
                operation: plan.kind,
                started_at,
                completed_at: chrono::Utc::now(),
                outcome: OperationOutcome::NoChange,
                effects: Vec::new(),
                warnings: Vec::new(),
                resulting_state: None,
            },
            failures: Vec::new(),
        });
    }

    let mut effects: Vec<AppliedEffect> = Vec::new();
    let mut failures: Vec<ApplyFailure> = Vec::new();

    for branch in &detail.branches {
        let refname = format!("refs/heads/{branch}");
        // Read the value at the moment of the delete rather than reusing the
        // plan's: a receipt is a *record*, and the plan's copy is a prediction.
        // `validate_cleanup_plan` has just established the two agree, and
        // recording what was actually there is what survives if they ever
        // don't.
        let old = context.git().rev_parse_opt(&refname)?;
        match context.git().delete_branch_strict(branch) {
            Ok(()) => effects.push(AppliedEffect::LocalRefDelete {
                refname,
                old: old.unwrap_or_default(),
            }),
            Err(e) => failures.push(ApplyFailure {
                refname,
                cause: first_line(&e),
            }),
        }
    }

    for refname in &detail.refs {
        let old = context.git().rev_parse_opt(refname)?;
        match context.git().delete_ref(refname) {
            Ok(()) => effects.push(AppliedEffect::LocalRefDelete {
                refname: refname.clone(),
                old: old.unwrap_or_default(),
            }),
            Err(e) => failures.push(ApplyFailure {
                refname: refname.clone(),
                cause: first_line(&e),
            }),
        }
    }

    Ok(CleanupRun {
        receipt: ExecutionReceipt {
            plan_id: plan.id.clone(),
            operation: plan.kind,
            started_at,
            completed_at: chrono::Utc::now(),
            outcome: OperationOutcome::Applied,
            effects,
            warnings: Vec::new(),
            // No `Result` block: a sweep of refs says nothing about what any
            // environment contains, and listing every environment under a
            // branch deletion is noise the reader has to skim past. Environment
            // branches and their state refs are reserved from the sweep anyway.
            resulting_state: None,
        },
        failures,
    })
}

/// Refuse a plan whose inputs moved under it.
///
/// Not a call to `operations::rebuild::validate_plan` or
/// `operations::metadata::validate_metadata_plan`: both are typed to their own
/// detail. This is the second such copy in the directory, and the third is the
/// point at which the shared `validate` earns a generic parameter.
pub fn validate_cleanup_plan(
    context: &GlobalContext,
    plan: &OperationPlan<CleanupPlanDetail>,
) -> std::result::Result<(), PlanApplyError> {
    let changed = changed_inputs(&plan.fingerprint, context.git());
    if changed.is_empty() {
        return Ok(());
    }
    Err(PlanApplyError::stale_plan(
        plan.kind,
        "cleanup",
        plan.detail.environment_filter.as_deref().unwrap_or("all"),
        &changed,
    ))
}

/// An error's first non-empty line, which is where git puts the cause.
///
/// Git's own stderr puts the diagnosis on line one and everything after it on
/// hints; quoting all of it would print advice under a failure it does not fit.
fn first_line(error: &anyhow::Error) -> String {
    error
        .to_string()
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git refused the delete for an unstated reason")
        .to_string()
}

/// The refname a candidate name refers to, for the fingerprint and the
/// effects.
///
/// A branch candidate is a `refs/heads/` name; an archive candidate is already
/// a full `refs/hitch/…` name. `starts_with("refs/")` is the whole of the
/// discrimination, and it is checked on the value rather than on which list the
/// name came from so that the two lists can stay `Vec<String>` and the plan's
/// `candidates` can be the single ordered concatenation both the fingerprint
/// and the effects are built from.
fn fully_qualified(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_string()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Environments whose archive refs get pruned, in name order.
///
/// `--env` scopes this exactly like it scopes branch cleanup. The sort is the
/// point that was missing: the caller's input is a `HashMap`'s keys, so without
/// it two cleanups of the same repository enumerate the same refs in a
/// different order and produce different plans.
fn envs_in_scope(config: &crate::types::HitchConfig, env_filter: Option<&str>) -> Vec<String> {
    let mut envs: Vec<String> = config
        .environments
        .keys()
        .filter(|e| env_filter.map(|f| f == *e).unwrap_or(true))
        .cloned()
        .collect();
    envs.sort();
    envs
}

/// Refs older than the most recent [`ARCHIVE_REF_RETENTION`] under
/// `refs/hitch/<namespace>/<env>/*`, for every namespace/env pair in scope.
/// Ref names sort chronologically because the timestamp segment is
/// fixed-width, so this needs no extra date lookup.
///
/// Newest-first within a namespace/env pair, which is `rev().skip()`'s order
/// and is preserved rather than tidied: the plan's effect list is a deletion
/// list, and a reader scanning it top-down sees the most-recently-obsolete ref
/// first, which is the one they are least likely to regret deleting.
fn stale_archive_refs(context: &GlobalContext, envs: &[String]) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for namespace in PRUNABLE_NAMESPACES {
        for env_name in envs {
            let prefix = format!("refs/hitch/{}/{}/", namespace, env_name);
            let mut refs = context.git().list_refs_under(&prefix)?;
            refs.sort(); // chronological: fixed-width timestamp suffix
            if refs.len() > ARCHIVE_REF_RETENTION {
                stale.extend(refs.into_iter().rev().skip(ARCHIVE_REF_RETENTION));
            }
        }
    }
    Ok(stale)
}
