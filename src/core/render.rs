//! Rendering a plan or a receipt as text, and a plan as JSON.
//!
//! **This module is the only place in the codebase allowed to choose words.**
//! Every other display path formats something itself, and §17 of the spec has
//! a terminology table that those paths have no way to enforce. A rule that
//! lives in a convention is a rule that decays; a rule that lives in the one
//! function that produces the output is a rule that holds.
//!
//! Two properties make that possible, and both are load-bearing:
//!
//! * **The renderers are pure.** [`render_plan`] and [`render_receipt`] take a
//!   value and return a `String` — no `GlobalContext`, no `Result`, no git, no
//!   clock, no I/O. A renderer that can open a repository is a renderer that
//!   can disagree with the thing it renders, and P3 already paid for that bug
//!   once in `commands/status.rs`, which derived the same verdict four times.
//!   The impure half is [`emit_json`] and [`confirm_plan`], and both take the
//!   already-rendered `String`.
//! * **The model is the vocabulary.** One function renders all four
//!   operations because none of them needs data the others do not have:
//!   [`crate::operations::model::OperationIntent`] names what is being done,
//!   `current`/`proposed` are the projections, `compositions` carries
//!   per-branch state, `effects` is the change list, `unaffected` is the "will
//!   not change" list, and `warnings` are the refusals. The one field a shared
//!   renderer ignores is `detail`, which is where per-operation bookkeeping
//!   lives precisely because display does not need it.
//!
//! That is also why there is no `render_promote` / `render_release` pair: a
//! second renderer is a second chance to describe the same operation in
//! different words, and the whole point of the phase is that there is one
//! description per operation.
//!
//! The other half of the contract is negative. A renderer **never derives a
//! verdict**: every "held", "still owed", or "already up to date" it prints is
//! read from a field, never inferred. `the_renderer_never_says_what_the_model_does_not_say`
//! is the regression test, and it is deliberately a negative test — the failure
//! mode of a display layer is not crashing, it is inventing.

use crate::commands::global_context::GlobalContext;
use crate::core::activity::{ActivityLog, ApprovalDirection, HitchEvent, RebuildOutcome};
use crate::core::state::{EnvironmentHealth, RepositoryStateSnapshot};
use crate::core::status::{MatrixModel, MatrixSummaryRow};
use crate::core::why::{
    NextAction, WhatHitchDid, WhyEnvironmentExplanation, WhyExplanation, WhyFeatureExplanation,
    WhyFeatureInEnvironment, WhyMembership, WhyReason,
};
use crate::operations::model::{
    AppliedEffect, ConfirmationRequirement, EnvironmentProjection, ExecutionReceipt, HoldPair,
    OperationOutcome, OperationPlan, PlannedBranch, PlannedBranchState, PlannedEffect,
};
use crate::utils::output::OutputLevel;
use anyhow::Context as _;

/// Render a plan as the block a human reads before agreeing to it.
///
/// Generic over the per-operation `detail` for the reason in the module header:
/// the detail is not display data. A plan renders the same way whether it came
/// from `plan_rebuild` or `plan_release`, and the only thing that changes the
/// output is the intent.
pub fn render_plan<I>(plan: &OperationPlan<I>) -> String {
    let mut out = String::new();

    heading(&mut out, &plan_headline(plan));

    if !plan.compositions.is_empty() {
        out.push('\n');
        heading(&mut out, "Composition");
        for composition in &plan.compositions {
            if composition.branches.is_empty() {
                // Not a formatting nicety: an environment with no promoted
                // branches still *has* a composition (of its base alone), and
                // saying "base only" is the only true thing about it. Dropping
                // the section would render a real build as if it were nothing
                // to do.
                out.push_str("    (base only — no promoted branches)\n");
                continue;
            }
            for branch in &composition.branches {
                render_planned_branch(&mut out, branch);
            }
        }
    }

    // The `Current`/`Proposed` pair exists to show a *transition*, so it
    // renders only when there is one. Three arms, total, decided on the model
    // rather than on the prose — which is why the third needs no invented
    // wording:
    //
    // - both `Some` and equal: an operation that composes nothing and changes
    //   nothing about what is composed (`hitch lock dev`). Rendering the pair
    //   anyway would print `dev = main` twice — the same fact twice, saying
    //   nothing about the lock, which is the thing the user is here to read.
    // - both `Some` and different: the ordinary case, both sides.
    // - exactly one `Some`: create and destroy. `hitch add qa` has no `qa` to
    //   project before it runs, and `hitch remove qa` will have none after, so
    //   `None` is the honest claim and the one side that exists speaks alone.
    //   The headline already names which way the operation goes, so this arm
    //   does not have to say "does not exist yet" as well.
    if plan.current != plan.proposed {
        for (title, projection) in [("Current", &plan.current), ("Proposed", &plan.proposed)] {
            let Some(projection) = projection else {
                continue;
            };
            out.push('\n');
            heading(&mut out, title);
            out.push_str(&format!("  {}\n", describe_projection(projection)));
        }
    }

    // An anchor is a write hitch makes and then takes back, so it is listed —
    // but not as a change, because it is not one. Putting it in the same table
    // as `dev  a16a75c → 5bf671e` would claim a ref exists afterwards that will
    // not exist.
    let (anchors, durable): (Vec<_>, Vec<_>) = plan
        .effects
        .iter()
        .partition(|effect| is_transient_anchor(&effect.refname()));

    if !durable.is_empty() {
        out.push('\n');
        heading(&mut out, "Will change");
        // Two columns, resource then what happens. The name is the *short* ref
        // (`dev`, not `refs/heads/dev`) because §17 says normal UX should speak
        // in branches; the full refname stays in the plan, which is the
        // machine-readable half.
        let width = durable
            .iter()
            .map(|e| short_ref(&e.refname()).len())
            .max()
            .unwrap_or(0);
        for effect in durable {
            let name = short_ref(&effect.refname());
            out.push_str(&format!(
                "  {name:<width$}   {}\n",
                describe_planned_effect(effect, plan),
                width = width
            ));
        }
    }

    if !anchors.is_empty() {
        out.push('\n');
        heading(&mut out, "Held only until the publish lands");
        // The anchor's ref name is mechanism (`--verbose` and `--json` carry it
        // in the plan); the meaning is that the composed commit is protected.
        out.push_str("  The new build is kept safe until it is published.\n");
    }

    if !plan.unaffected.is_empty() {
        out.push('\n');
        heading(&mut out, "Will not change");
        for resource in &plan.unaffected {
            out.push_str(&format!("  {}\n", resource.name));
        }
    }

    // The approval gate is blocking in the model — the declaration edit does not
    // happen — but what the reader is told is that a request will be filed, so
    // it gets its own heading rather than the refusal's. By *kind*, never by
    // matching the message.
    let (approval, other): (Vec<_>, Vec<_>) = plan
        .warnings
        .iter()
        .partition(|w| w.kind == crate::operations::model::PlanWarningKind::ApprovalRequired);

    if !other.is_empty() {
        out.push('\n');
        // "Needs your decision" is right for a confirmation and wrong for a
        // policy refusal, whose whole content is that no decision available to
        // this reader will let it through — and a heading inviting a decision
        // that cannot help is the kind of thing a reader acts on before reading
        // the line under it.
        heading(
            &mut out,
            if other.iter().any(|w| w.is_blocking()) {
                "Why this cannot apply"
            } else if plan.confirmation.required {
                "Needs your decision"
            } else {
                // Nobody is being asked anything — `--force` or `--yes` already
                // answered, or the operation never asked — so a heading that
                // invites a decision would be inviting one that was made.
                "Worth knowing"
            },
        );
        for warning in &other {
            let glyph = if warning.is_blocking() {
                "⛔"
            } else {
                "⚠️"
            };
            annotated(&mut out, glyph, &warning.message);
        }
    }

    if !approval.is_empty() {
        out.push('\n');
        heading(&mut out, "Needs approval");
        for warning in &approval {
            annotated(&mut out, "⏳", &warning.message);
        }
    }

    out.trim_end().to_string()
}

/// The one-line name of what a plan is for, derived from the intent.
///
/// Four phrasings, one per `OperationIntent` variant, because the headline is
/// the first thing a human reads and a generic one ("operation planned") costs
/// them the entire benefit of having read it. Every component comes out of the
/// intent, so there is nothing here for a renderer to get wrong against the
/// command line that produced it.
pub fn plan_headline<I>(plan: &OperationPlan<I>) -> String {
    let list = |items: &[String]| {
        if items.len() == 1 {
            items[0].clone()
        } else {
            items.join(", ")
        }
    };
    match &plan.intent {
        crate::operations::model::OperationIntent::RebuildEnvironment { environment } => {
            format!("Rebuild {environment}")
        }
        crate::operations::model::OperationIntent::PromoteBranches {
            environment,
            branches,
        } => format!("Promote {} → {environment}", list(branches)),
        crate::operations::model::OperationIntent::DemoteBranches {
            environment,
            branches,
        } => format!("Demote {} → {environment}", list(branches)),
        crate::operations::model::OperationIntent::ReleaseEnvironment {
            environment,
            target,
        } => format!("Release {environment} → {target}"),
        // The seven metadata operations. Each names *what* it does and nothing
        // it does not: `hitch set dev` with three fields changed says "3
        // settings" rather than listing them, because the field list is the
        // plan's "Will change" section and repeating it in the headline would
        // print the same three words twice in one screen.
        crate::operations::model::OperationIntent::LockEnvironment { environment } => {
            format!("Lock {environment}")
        }
        crate::operations::model::OperationIntent::UnlockEnvironment { environment } => {
            format!("Unlock {environment}")
        }
        crate::operations::model::OperationIntent::SetEnvironment {
            environment,
            changes,
        } => format!("Set {environment} · {}", count(changes.len(), "setting")),
        crate::operations::model::OperationIntent::AddEnvironment { environment, base } => {
            format!("Add {environment} on {base}")
        }
        crate::operations::model::OperationIntent::RemoveEnvironment { environment } => {
            format!("Remove {environment}")
        }
        // The count, not the refs. Thirty refnames in a headline is a wall, and
        // the plan's effect list is where a reader goes to see what was found.
        // "ref", not "archived ref": the candidates are unpromoted *branches*
        // as well as stale archive refs, and a headline naming only the second
        // kind is wrong for every plan whose first candidate is a branch — which
        // is most of them.
        crate::operations::model::OperationIntent::Cleanup { candidates } => {
            if candidates.is_empty() {
                // A sweep can find nothing to delete and still have a plan to
                // show: the branches it kept, and why.
                "Clean up — nothing to delete".to_string()
            } else {
                let branches_noun = |n: usize| {
                    if n == 1 {
                        "1 branch".to_string()
                    } else {
                        format!("{n} branches")
                    }
                };
                let archived = candidates
                    .iter()
                    .filter(|c| c.starts_with("refs/hitch/"))
                    .count();
                let branches = candidates.len() - archived;
                match (branches, archived) {
                    (b, 0) => format!("Clean up {}", branches_noun(b)),
                    (0, a) => format!("Clean up {}", count(a, "archived build")),
                    (b, a) => format!(
                        "Clean up {} and {}",
                        branches_noun(b),
                        count(a, "archived build")
                    ),
                }
            }
        }
        crate::operations::model::OperationIntent::ApplyApproval {
            environment,
            branches,
            ..
        } => format!("Approve {} → {environment}", list(branches)),
    }
}

/// `1 setting`, `3 settings`, `0 settings` — count and noun, pluralised.
///
/// Separate from the branch-list helper above because a branch list *names* its
/// members when there is one and only counts them when there are many, which is
/// the opposite trade: a list of one refname is easier to read than "1 ref", and
/// a list of thirty is not.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A short SHA for display, or a stand-in when there was no previous value.
///
/// §17: "OID / SHA → commit / revision, show short SHA where useful". Short is
/// useful and full is unreadable, so this is the only place a SHA is rendered
/// and it is always shortened. The full value is in the JSON.
fn short(sha: &str) -> String {
    // 7 is git's own default and enough to identify a commit in any repository
    // a human is looking at; a longer prefix is noise in a two-column table.
    sha.chars().take(7).collect()
}

/// An absent predecessor, rendered as a word rather than as an empty string.
///
/// `old → new` with an empty left column is unreadable, and it is the ordinary
/// first-build case rather than an error.
fn previous(old: Option<&String>) -> String {
    old.map(|o| short(o)).unwrap_or_else(|| "none".to_string())
}

/// A ref's display name: the last thing a person would call it.
///
/// `refs/heads/dev` is `dev`; `refs/remotes/origin/dev` is `origin/dev`, because
/// dropping the remote there would make a local branch and its published
/// counterpart render as the same row. `refs/hitch/…` names a hitch-internal
/// thing, and the honest way to present one is to say what it is for rather
/// than print a path the user has never seen.
fn short_ref(refname: &str) -> String {
    if refname == "refs/heads/hitch-metadata" || refname == "refs/remotes/origin/hitch-metadata" {
        return "settings".to_string();
    }
    if let Some(branch) = refname.strip_prefix("refs/heads/") {
        return branch.to_string();
    }
    if let Some(remote) = refname.strip_prefix("refs/remotes/") {
        return remote.to_string();
    }
    if let Some(tag) = refname.strip_prefix("refs/tags/") {
        return format!("tag {tag}");
    }
    if let Some(env) = refname.strip_prefix("refs/hitch/state/") {
        return format!("build record for {env}");
    }
    refname
        .strip_prefix("refs/hitch/")
        .unwrap_or(refname)
        .to_string()
}

/// Whether a ref named by an effect is one hitch creates and then removes within
/// the same operation.
///
/// Matched on the refname, never on the effect's description: `refs/hitch/build/*`
/// and `refs/hitch/release/*` exist for exactly one reason — to keep a
/// `commit-tree` result reachable between composing it and the CAS that publishes
/// it — and nothing else ever writes them. That makes the prefix a fact about
/// hitch's ref layout rather than a guess about prose, which is the same reason
/// `commands/cleanup.rs` can name those families as a set.
///
/// `refs/hitch/state/*` is deliberately *not* in this set: the build record
/// survives the operation, and reporting it as transient would be its own kind of
/// lie. Neither are `prev/` and `backup/`, which a user reaches for with
/// `hitch rollback`.
fn is_transient_anchor(refname: &str) -> bool {
    refname.starts_with("refs/hitch/build/") || refname.starts_with("refs/hitch/release/")
}

/// One line per promoted branch, with a glyph per state.
///
/// The glyphs are load-bearing and cannot be merged: `Included` and `Held` are
/// the two states a reader must be able to tell apart at a glance, and
/// `AlreadyInBase` is separate from `Included` because only branches that
/// advanced the composition get their own commit — a plan that claimed N
/// branches landed when N−1 commits exist would be the plan lying.
fn render_planned_branch(out: &mut String, branch: &PlannedBranch) {
    let at = format!(" at {}", short(&branch.sha));
    match &branch.state {
        PlannedBranchState::Included => out.push_str(&format!("  ✓ {}{at}\n", branch.branch)),
        PlannedBranchState::AlreadyInBase => {
            out.push_str(&format!("  = {} (already in base)\n", branch.branch))
        }
        PlannedBranchState::ReplayedResolution { resolution_id } => {
            out.push_str(&format!(
                "  ♻️ {}{at} (from recorded resolution {})\n",
                branch.branch,
                short(resolution_id)
            ));
        }
        PlannedBranchState::Held {
            conflicts_with,
            files,
        } => {
            out.push_str(&format!(
                "  ⛔ {} held — conflicts with {conflicts_with}\n",
                branch.branch
            ));
            for file in files {
                out.push_str(&format!("      {file}\n"));
            }
            // The remedy is per-conflict, and it is the one piece of guidance
            // this renderer reconstructs rather than copies. Losing it would be
            // a real regression — a held branch with no way to fix it is a dead
            // end the user has to go find `git rebase` documentation for.
            out.push_str(&format!(
                "    fix: git checkout {} && git rebase {conflicts_with}\n",
                branch.branch
            ));
        }
        // Cannot happen with today's planner. Modelled explicitly so that a
        // planner bug renders as "this branch is accounted for nowhere" rather
        // than silently reading as a branch that was included.
        PlannedBranchState::Missing => out.push_str(&format!(
            "  ⚠️ {} — accounted for in neither the build nor the held list\n",
            branch.branch
        )),
    }
}

// ── The environment equation (spec §13) ───────────────────────────────────

/// One environment's composition, reduced to the minimum any renderer needs.
///
/// §13 asks for `dev = main + auth + payments` to be spelled the *same way*
/// wherever hitch shows a composition — in a plan, in `hitch status`, in
/// `hitch why`, in `hitch tree` — and the way to guarantee that is to make the
/// equation a value rather than a format string. A display path that
/// hand-formats its own version is a display path that will eventually say
/// `dev: main, auth, payments`, and §13's whole point is that it does not.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEquation {
    pub environment: String,
    pub base: String,
    /// In **composition order**. Never sorted: sorting this list describes a
    /// different build than the one the caller observed.
    pub terms: Vec<EquationTerm>,
    /// Branches that are declared but deliberately *not* a term — held, or
    /// unaccounted for. Rendered indented beneath the equation rather than as
    /// terms, because an excluded branch is not part of the sum.
    pub excluded: Vec<ExcludedTerm>,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct EquationTerm {
    pub branch: String,
    /// A term can be annotated when "in the equation" and "actually changing
    /// anything" differ — a branch already reachable from the base is in the
    /// declaration but not a separate commit.
    pub state: EquationTermState,
}

/// Whether a term is an ordinary term or one already reachable from the base.
///
/// `snake_case` for the reason every other enum in a `--json` envelope has it:
/// the equation is embedded in both `hitch status --json` and `hitch why
/// --json`, so `"Plain"` is a Rust type name in a wire contract.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EquationTermState {
    Plain,
    InBase,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ExcludedTerm {
    pub branch: String,
    pub reason: ExclusionReason,
}

/// Why a declared branch is not a term in the equation.
///
/// `snake_case`, as on [`EquationTermState`] — an externally-tagged enum puts
/// the variant name in the document, so this one is `"Held"` in the JSON unless
/// it is renamed.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Excluded from the last build because it conflicted. The partner is
    /// carried because the remedy differs depending on whether it was the base
    /// or a specific peer, and a reason that cannot be acted on is not a
    /// reason.
    Held {
        conflicts_with: String,
        files: Vec<String>,
    },
    /// Declared, but hitch cannot say where it stands.
    Unknown,
}

/// The reason a promote is refused because the branch would be held by the
/// rebuild it triggers. `conflict.conflicts_with` is the real partner: the
/// base gets the plain wording, a promoted peer is named as being already in
/// the environment. Either way the remedy is rebasing the branch onto that
/// partner: `hitch resolve` only acts on a branch the environment already
/// declares, and this one is not promoted yet.
pub fn render_promote_refusal(
    new_branch: &str,
    environment: &str,
    base: &str,
    conflict: &crate::utils::prelude::CompatibilityConflict,
) -> String {
    let mut msg = format!(
        "Cannot promote '{}' to environment '{}': compatibility check failed.\n\n",
        new_branch, environment
    );
    if conflict.conflicts_with == base {
        msg.push_str(&format!("  {} conflicts with {}\n", new_branch, base));
    } else {
        msg.push_str(&format!(
            "  {} conflicts with {}, which is already in {}\n",
            new_branch, conflict.conflicts_with, environment
        ));
    }
    for f in &conflict.conflicted_files {
        msg.push_str(&format!("    {}\n", f));
    }
    msg
}

/// Why an approval-gated change cannot even be requested.
pub fn render_insufficient_approvers(
    environment: &str,
    required: usize,
    eligible: usize,
    requester: &str,
) -> String {
    format!(
        "Cannot request approval for '{environment}': {required} approval(s) required, but only \
         {eligible} eligible approver(s) are available.\n\
         The requester ({requester}) cannot approve their own request, so they don't count \
         toward the threshold. You can also lower the threshold with \
         `hitch set {environment} --min-approvals <n>`."
    )
}

/// The next step after a promote refusal: the fix, then the promote that the
/// refusal stopped (re-running it first would only be refused again).
pub fn promote_refusal_remedy(
    new_branch: &str,
    environment: &str,
    conflict: &crate::utils::prelude::CompatibilityConflict,
) -> String {
    format!(
        "git checkout {} && git rebase {}, then {}",
        new_branch,
        conflict.conflicts_with,
        crate::operations::model::OperationKind::Promote.command_hint(environment, new_branch)
    )
}

/// The advisory a release plan carries for a dependent environment it will not
/// rebuild because its composition would hold a branch.
pub fn render_dependent_skip(
    environment: &str,
    conflict: &crate::utils::prelude::CompatibilityConflict,
) -> String {
    let files: String = conflict
        .conflicted_files
        .iter()
        .flat_map(|f| ["\n  ", f.as_str()])
        .collect();
    format!(
        "'{}' will not be rebuilt — compatibility check failed when merging '{}' onto '{}':{}\n  \
         fix: git checkout {} && git rebase {}",
        environment,
        conflict.branch,
        conflict.conflicts_with,
        files,
        conflict.branch,
        conflict.conflicts_with
    )
}

/// The conflict policy in words a user would choose, never the variant name.
pub fn describe_conflict_policy(policy: crate::types::OnConflict) -> &'static str {
    match policy {
        crate::types::OnConflict::Eject => "hold conflicting branches",
        crate::types::OnConflict::Halt => "stop the build",
    }
}

/// Render an environment equation: `dev = main + auth + payments`.
///
/// The `excluded` list is rendered underneath, indented, one per line — so a
/// held branch appears in both the plan's `Composition` section (per-branch,
/// with a remedy) and here (as an absence). That is not duplication: one names
/// the branch's own state and what to do about it, the other makes the
/// *arithmetic* honest. A reader who saw only `dev = main + auth + payments`
/// would not know a fourth declared branch was involved.
pub fn render_equation(equation: &EnvironmentEquation) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} = {}", equation.environment, equation.base));
    // An environment with no promoted branches still *has* a composition, and
    // `dev = main` is that composition in full — not a truncated one needing an
    // apology. The "base only" wording lives in `render_plan`'s Composition
    // section, which is about what a build did with branches, and having both
    // would print the same remark twice.
    if !equation.terms.is_empty() {
        for term in &equation.terms {
            out.push_str(" + ");
            out.push_str(&term.branch);
            if term.state == EquationTermState::InBase {
                out.push_str(" (already in base)");
            }
        }
    }

    for excluded in &equation.excluded {
        out.push('\n');
        match &excluded.reason {
            ExclusionReason::Held {
                conflicts_with,
                files,
            } => {
                out.push_str(&format!(
                    "    {} ⛔ held — conflicts with {}",
                    excluded.branch, conflicts_with
                ));
                for file in files {
                    out.push_str(&format!("\n        {file}"));
                }
            }
            ExclusionReason::Unknown => {
                out.push_str(&format!("    {} ◌ not accounted for", excluded.branch));
            }
        }
    }

    out
}

impl EnvironmentEquation {
    /// The environment's **declaration**, as an equation.
    ///
    /// Every term is plain, and nothing is excluded, because a declaration has
    /// no excluded terms: a branch is declared whether or not the last build
    /// managed to take it. This is the `Desired` half of `hitch why` and what
    /// `hitch tree` shows.
    pub fn from_declaration(state: &crate::core::state::EnvironmentState) -> EnvironmentEquation {
        Self::from_parts(
            &state.name,
            &state.base,
            state.desired.branches.iter().map(|b| b.name.as_str()),
        )
    }

    /// The same declaration equation, read straight off `hitch.json`.
    ///
    /// A second constructor because `hitch tree` reads the config directly
    /// rather than building a snapshot: the tree is a view of *declarations*,
    /// and making it pay for a snapshot — one `rev_parse_opt` per branch, a
    /// build-record read per environment, a `merge-base` walk for the
    /// `already in base` check — to then discard every field except `base` and
    /// `branches` would buy nothing and cost a network round trip's worth of
    /// latency. Both constructors delegate to [`Self::from_parts`], so there is
    /// still exactly one place that knows what a declaration equation is.
    pub fn from_config(name: &str, env: &crate::types::Environment) -> EnvironmentEquation {
        Self::from_parts(name, &env.base, env.branches.iter().map(String::as_str))
    }

    /// The one definition of "a declaration, as an equation": every declared
    /// branch is a plain term and nothing is excluded.
    ///
    /// Order is the caller's — never sorted. `hitch why` and `hitch status` read
    /// `EnvironmentState::desired.branches`, which is already in declaration
    /// order, and `hitch tree` reads `Environment.branches`, also declaration
    /// order. Promotion order is the order the user composed the environment
    /// in, and re-sorting it here would make a merge conflict depend on
    /// alphabetical luck.
    fn from_parts<'a>(
        name: &str,
        base: &str,
        branches: impl Iterator<Item = &'a str>,
    ) -> EnvironmentEquation {
        EnvironmentEquation {
            environment: name.to_string(),
            base: base.to_string(),
            terms: branches
                .map(|branch| EquationTerm {
                    branch: branch.to_string(),
                    state: EquationTermState::Plain,
                })
                .collect(),
            excluded: Vec::new(),
        }
    }

    /// The environment's **last build**, as an equation, or `None` when there is
    /// no build to describe.
    ///
    /// `None` is the honest answer for a `LegacyUnknown` environment, and it is
    /// why this returns an `Option` rather than an equation with no terms: `dev
    /// = main` as an *actual* composition would assert that the last build
    /// contained nothing but its base, which is not what "hitch cannot say" means.
    /// The caller renders no `Actual` section at all.
    ///
    /// `in_base` answers "does this branch contribute anything new to this
    /// environment's build?" and is only consulted for branches the record says
    /// were included. A branch that was already reachable from the base was in
    /// the build and changed nothing, and saying so is the difference between
    /// an equation and an accurate one.
    pub fn from_build(
        state: &crate::core::state::EnvironmentState,
        in_base: &dyn Fn(&str) -> bool,
    ) -> Option<EnvironmentEquation> {
        let record = state.actual.actual()?;
        Some(EnvironmentEquation {
            environment: state.name.clone(),
            // The record's own base name rather than the current declaration's:
            // this equation describes a build, and that build consumed the base
            // as it was then. Substituting today's base would describe a
            // composition nobody performed — which is exactly what the
            // `NeedsRebuild` verdict is about.
            base: record.record.base_name.clone(),
            terms: record
                .included
                .iter()
                .map(|b| EquationTerm {
                    branch: b.branch.clone(),
                    state: if in_base(&b.branch) {
                        EquationTermState::InBase
                    } else {
                        EquationTermState::Plain
                    },
                })
                .collect(),
            excluded: record
                .held
                .iter()
                .map(|conflict| ExcludedTerm {
                    branch: conflict.branch.clone(),
                    reason: ExclusionReason::Held {
                        conflicts_with: conflict.conflicts_with.clone(),
                        files: conflict.conflicted_files.clone(),
                    },
                })
                .collect(),
        })
    }

    /// Build an equation from a plan's projection, with every term plain and
    /// nothing excluded.
    ///
    /// The `excluded` list is deliberately empty here. A plan already lists
    /// every branch and its state in its `Composition` section, with a remedy;
    /// putting the held ones into the equation as well would print the same
    /// fact twice in two vocabularies, which is the exact failure §13 is
    /// arguing against.
    pub fn from_projection(projection: &EnvironmentProjection) -> EnvironmentEquation {
        EnvironmentEquation {
            environment: projection.environment.clone(),
            base: projection.base.clone(),
            terms: projection
                .branches
                .iter()
                .map(|b| EquationTerm {
                    branch: b.branch.clone(),
                    state: EquationTermState::Plain,
                })
                .collect(),
            excluded: Vec::new(),
        }
    }
}

/// `dev = main + auth + search`, in declaration order.
///
/// The order is composition order and is load-bearing
/// (`EnvironmentProjection::branches` says so), so this renders the list as it
/// is and never sorts it. A renderer that alphabetised the list would be
/// rendering a *different build* than the one it was handed.
fn describe_projection(projection: &EnvironmentProjection) -> String {
    render_equation(&EnvironmentEquation::from_projection(projection))
}

fn describe_planned_effect<I>(effect: &PlannedEffect, plan: &OperationPlan<I>) -> String {
    match effect {
        // The model's own words. It already says "add login into 'dev'" or
        // "record of what 'dev' last built", which is closer to §17's
        // "desired-state change" than anything this function could invent
        // without knowing which of the two it is.
        PlannedEffect::MetadataChange { description, .. } => description.clone(),
        PlannedEffect::LocalRefUpdate { refname, old, new } => {
            let transition = format!("{} → {}", previous(old.as_ref()), short(new));
            // A local ref update of the environment this plan composes is a
            // rebuild, and saying "rebuild from 3 branches" tells the reader
            // *why* the SHA moved. One that is not — release's target, which is
            // a merge — gets the bare transition, because "rebuild" there would
            // be false. The match has to be able to fail.
            match plan
                .compositions
                .iter()
                .find(|c| format!("refs/heads/{}", c.environment) == *refname)
            {
                Some(composition) => format!(
                    "rebuild from {} branch{} · {transition}",
                    composition.branches.len(),
                    plural_s(composition.branches.len())
                ),
                None => transition,
            }
        }
        PlannedEffect::RemoteRefUpdate {
            refname: _,
            old,
            new,
        } => {
            format!("publish {} → {}", previous(old.as_ref()), short(new))
        }
        // "delete", not a transition with an empty right-hand side. The
        // two-column table this renders into is resource → what happens, so the
        // cell has to name the verb to be a sentence at all.
        PlannedEffect::LocalRefDelete { refname } => format!("delete {}", short_ref(refname)),
        // The resource column already reads `tag <name>`; naming it again here
        // printed the tag twice on one row.
        PlannedEffect::TagCreation { target_sha, .. } => {
            format!("create at {}", short(target_sha))
        }
        PlannedEffect::DependentEnvironmentRebuild {
            environment,
            because,
            ..
        } => format!("rebuild {environment} — {because}"),
        PlannedEffect::PromotionPrune {
            environment,
            branches,
            ..
        } => format!("prune {} from {environment}", branch_list(branches)),
    }
}

fn plural_s(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "es"
    }
}

fn branch_list(branches: &[String]) -> String {
    if branches.len() == 1 {
        branches[0].clone()
    } else {
        branches.join(", ")
    }
}

/// Render a receipt as the block a human reads after the fact.
///
/// The three structural rules, in order of how much they cost to get wrong:
///
/// 1. **The headline is the outcome, and `AppliedWithHolds` never reads as
///    `Applied`.** The two call for different urgency and carry different CI
///    meaning — a rebuild that exits 0 with branches held is a pipeline that
///    looks green and is not shipping what it declared. The enum exists to keep
///    them apart; a renderer that flattens it undoes the phase's own rationale.
/// 2. **✓ lines come from `effects`, and only from `effects`.** `effects` is the
///    observation, not the prediction, so an effect that did not happen cannot
///    appear as one.
/// 3. **Owed work is its own section, after the applied work, and never a ✓.**
///    An owed effect is neither a failure nor a success: the operation is done
///    and something is still outstanding. Filing it among the warnings with the
///    same glyph is how "the push did not land" gets read as a footnote.
///
/// The sections, in order, are applied work, owed work, and the resulting
/// state. Nothing else is emitted — a fact that fits none of the three has to
/// go into `effects`, `warnings` or `resulting_state`, and being made to choose
/// is what keeps the receipt free of lines nobody can act on.
///
/// Render the feature × environment grid (spec §12).
///
/// The cell is **glyph + word**, never a glyph alone, and never colour. §12
/// requires the view to be understandable without colour, and that
/// requirement turns out to be the same as "readable in a pipe, in CI output,
/// and by a screen reader" — a grid that only carries meaning in colour is a
/// grid whose meaning is lost exactly where automation needs it.
///
/// No width budget. See [`render_matrix_at`] for the narrow case and
/// [`MatrixLayout`] for why the fallback is prose rather than truncation.
pub fn render_matrix(model: &MatrixModel) -> String {
    let layout = MatrixLayout::measure(model);
    render_matrix_within(model, &layout)
}

/// The grid's column widths, measured once.
///
/// Every width in the table comes from here, so `minimum_width` and the render
/// cannot disagree about how wide the grid is — which is the whole content of a
/// narrow-terminal check, and a second measurement is exactly how it would come
/// to disagree.
struct MatrixLayout {
    feature_width: usize,
    /// One per column, in column order.
    cell_widths: Vec<usize>,
}

impl MatrixLayout {
    fn measure(model: &MatrixModel) -> MatrixLayout {
        let headers: Vec<String> = model.columns.iter().map(|c| c.to_uppercase()).collect();
        let cells: Vec<Vec<String>> = model
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| format!("{} {}", cell.glyph(), cell.label()))
                    .collect()
            })
            .collect();

        // One width per column, from its header and every cell in it. Measured in
        // *chars*, not bytes: `⛔` is three bytes and one character, and a
        // byte-measured table puts the row containing it one glyph out of line —
        // which reads as a slightly ragged table rather than as a bug.
        let mut cell_widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
        for row in &cells {
            for (i, cell) in row.iter().enumerate() {
                if i < cell_widths.len() {
                    cell_widths[i] = cell_widths[i].max(cell.chars().count());
                }
            }
        }
        let feature_width = model
            .rows
            .iter()
            .map(|r| r.feature.chars().count())
            .max()
            .unwrap_or(0)
            .max("Feature".len());

        MatrixLayout {
            feature_width,
            cell_widths,
        }
    }

    /// The narrowest terminal the grid can be printed in without wrapping.
    ///
    /// `feature_width`, plus a two-space gutter in front of each column, plus
    /// the column widths. The rule line is the same arithmetic.
    fn minimum_width(&self, columns: usize) -> usize {
        self.feature_width + 2 * columns + self.cell_widths.iter().sum::<usize>()
    }
}

fn render_matrix_within(model: &MatrixModel, layout: &MatrixLayout) -> String {
    let feature_header = "Feature";
    let headers: Vec<String> = model.columns.iter().map(|c| c.to_uppercase()).collect();
    let cells: Vec<Vec<String>> = model
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| format!("{} {}", cell.glyph(), cell.label()))
                .collect()
        })
        .collect();
    let widths = &layout.cell_widths;
    let feature_width = layout.feature_width;

    // Everything is **left**-aligned within its column, header included. This
    // is the non-obvious half and the whole table depends on it: a
    // right-aligned cell puts its first glyph at `width - len`, so two rows
    // whose cells have different lengths have their glyphs at different
    // columns — the definition of not being a table. The rule underneath spans
    // the full computed width; the rows are trimmed back so they carry no
    // trailing spaces, which are invisible in a terminal and visible in a diff.
    let mut lines: Vec<String> = Vec::with_capacity(model.rows.len() + 2);

    let mut header_line = pad_right(feature_header, feature_width);
    for (i, header) in headers.iter().enumerate() {
        header_line.push_str("  ");
        header_line.push_str(&pad_right(header, widths[i]));
    }
    lines.push(header_line.trim_end().to_string());

    lines.push("─".repeat(layout.minimum_width(model.columns.len())));

    for (row, row_cells) in model.rows.iter().zip(&cells) {
        let mut line = pad_right(&row.feature, feature_width);
        for (i, cell) in row_cells.iter().enumerate() {
            line.push_str("  ");
            line.push_str(&pad_right(cell, widths[i]));
        }
        lines.push(line.trim_end().to_string());
    }

    lines.join("\n")
}

/// Render the grid into a column budget, or say the budget is too small.
///
/// # The fallback is prose, and that is the decision
///
/// A grid wider than the terminal does not degrade gracefully on its own: the
/// terminal wraps it, and a wrapped table's cells stop lining up — which means
/// the columns, the one thing a table *is*, are the first thing lost. Truncating
/// branch names or dropping columns would keep a rectangle and lose the
/// contents, and a `feature/pay…` that no branch is called is worse than no
/// table: it is a wrong answer presented in the shape of a right one.
///
/// So below [`MatrixLayout::minimum_width`] the grid is replaced by the one thing
/// that is *more* useful in a narrow terminal than a wrapped grid — the shape of
/// the repository, and where to read it in full. §12.1's expansion is exactly
/// that shape, so the fallback points at the flag that produces it rather than
/// inventing a second expansion.
///
/// `render_matrix` is this with an unbounded budget, which is the right default
/// for a caller with no width to report: a pipe, a CI log, a file. Those are not
/// narrow, they are *unbounded*, and a budget invented from a guess would be
/// exactly the kind of untruthful input this program keeps refusing to accept.
pub fn render_matrix_at(model: &MatrixModel, budget: usize) -> String {
    let layout = MatrixLayout::measure(model);
    let needed = layout.minimum_width(model.columns.len());
    if needed <= budget {
        return render_matrix_within(model, &layout);
    }

    let count = model.columns.len();
    let features = model.rows.len();
    format!(
        "{count} environment{plural} declared, {features} feature{feature_plural}.\n\
         The matrix needs {needed} columns to line up, and this terminal has {budget}.\n\
         Per-environment detail: 'hitch status --environments'",
        plural = if count == 1 { "" } else { "s" },
        feature_plural = if features == 1 { "" } else { "s" },
    )
}

fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    format!("{}{}", text, " ".repeat(width.saturating_sub(len)))
}

/// Render the per-environment lines beneath the grid.
///
/// The counts come from [`MatrixSummaryRow`], which counted them *from the
/// cells*, so these lines cannot disagree with the grid above them. The health
/// word is [`EnvironmentHealth::label`]'s — the same function `hitch status`
/// already used — rather than a second vocabulary of health words.
pub fn render_environment_summaries(summaries: &[MatrixSummaryRow]) -> String {
    // The name column is as wide as the widest name, not a fixed four. A fixed
    // width is fine right up until an environment is called `stage`, and then
    // its counts sit one column right of everyone else's and the block stops
    // being a table. Same rule as `render_matrix`, for the same reason.
    let name_width = summaries
        .iter()
        .map(|s| s.environment.to_uppercase().chars().count())
        .max()
        .unwrap_or(0)
        .max(3);
    let mut out = String::new();
    for summary in summaries {
        let name = pad_right(&summary.environment.to_uppercase(), name_width);
        out.push_str(&format!(
            "{name}  desired {} · actual {}",
            summary.desired, summary.realised
        ));
        if summary.held > 0 {
            out.push_str(&format!(
                " · {}",
                plural_count(summary.held, "held", "held")
            ));
        }
        if summary.needs_rebuild > 0 {
            out.push_str(&format!(
                " · {}",
                plural_count(summary.needs_rebuild, "needs rebuild", "need rebuild")
            ));
        }
        if summary.missing > 0 {
            out.push_str(&format!(
                " · {}",
                plural_count(summary.missing, "missing", "missing")
            ));
        }
        if summary.actual_unknown > 0 {
            out.push_str(&format!(
                " · {}",
                plural_count(summary.actual_unknown, "actual unknown", "actual unknown")
            ));
        }
        if summary.locked {
            // In the name's own field, not bolted on the front and not appended
            // to the counts. Bolting it on the front was the first version and
            // it pushed this row's name eleven columns right of every other
            // row's, which is a table losing its own alignment over an
            // annotation — so the annotation takes the padding the name
            // already had, and the names stay in a column.
            out.push_str(" 🔒");
        }
        out.push_str(&format!("\n    {}\n", summary.health.label()));
    }
    out
}

fn plural_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub fn render_receipt(receipt: &crate::operations::model::ExecutionReceipt) -> String {
    let mut out = String::new();

    heading(&mut out, &outcome_headline(&receipt.outcome));

    if !receipt.effects.is_empty() {
        out.push('\n');
        for effect in &receipt.effects {
            render_applied_effect(&mut out, effect);
        }
    }

    let owed: Vec<_> = receipt.warnings.iter().filter(|w| w.owes_effect).collect();
    let advisory: Vec<_> = receipt.warnings.iter().filter(|w| !w.owes_effect).collect();

    if !advisory.is_empty() {
        out.push('\n');
        for warning in advisory {
            annotated(&mut out, "⚠️", &warning.message);
        }
    }

    if !owed.is_empty() {
        out.push('\n');
        heading(&mut out, "Still owed");
        for warning in owed {
            // The message already names its remedy (`hitch push dev -f`,
            // `hitch rebuild qa`), because the thing that knows the remedy is
            // the code that discovered the obligation. The renderer repeats it
            // rather than composing one, so there is no second place to forget
            // it.
            annotated(&mut out, "⧗", &warning.message);
        }
    }

    if let Some(snapshot) = &receipt.resulting_state {
        render_resulting_state(&mut out, snapshot);
    }

    out.trim_end().to_string()
}

fn outcome_headline(outcome: &OperationOutcome) -> String {
    match outcome {
        OperationOutcome::Applied => "Applied".to_string(),
        OperationOutcome::AppliedWithHolds => "Applied, with branches held".to_string(),
        OperationOutcome::ApprovalRequested => "Waiting for approval".to_string(),
        OperationOutcome::NoChange => "Already up to date".to_string(),
    }
}

fn render_applied_effect(out: &mut String, effect: &AppliedEffect) {
    match effect {
        AppliedEffect::MetadataChange {
            refname,
            description,
        } => {
            out.push_str(&format!("  ✓ {}   {description}\n", short_ref(refname)));
        }
        AppliedEffect::LocalRefUpdate { refname, old, new } => {
            out.push_str(&format!(
                "  ✓ {}\n    {} → {}\n",
                short_ref(refname),
                previous(old.as_ref()),
                short(new)
            ));
        }
        AppliedEffect::RemoteRefUpdate { refname, old, new } => {
            out.push_str(&format!(
                "  ✓ publish {}\n    {} → {}\n",
                short_ref(refname),
                previous(old.as_ref()),
                short(new)
            ));
        }
        // A ✓, because the ref *was* deleted — this is the observation, so it
        // cannot be ✓ for something that failed (that is the receipt's `⧗`
        // `Still owed` section's job). The value it had is the second line,
        // because "deleted feat-x" without it cannot be distinguished from a
        // delete of a *different* commit of the same branch.
        AppliedEffect::LocalRefDelete { refname, old } => {
            out.push_str(&format!(
                "  ✓ delete {}\n    was {}\n",
                short_ref(refname),
                short(old)
            ));
        }
        // The name that exists, not the one that was intended — release
        // disambiguates a second-granularity collision, so the receipt is
        // allowed to disagree with the plan here and this renderer must not
        // paper over it.
        AppliedEffect::TagCreation { name, target_sha } => {
            out.push_str(&format!("  ✓ tag {name}\n    at {}\n", short(target_sha)))
        }
        AppliedEffect::DependentEnvironmentRebuild {
            environment,
            outcome,
            held,
            ..
        } => {
            // Three glyphs, and the middle one is new: a rebuild that landed
            // with branches held out did rebuild, so `✓` would be true and
            // useless, and `⧗` is reserved for work the user is *owed* — which
            // a hold is not. `⚠️` is the receipt's existing word for an
            // anomaly that is not a failure, so the hold reads as one.
            let glyph = if outcome.owes_effect() {
                "⧗"
            } else if held.is_empty() {
                "✓"
            } else {
                "⚠️"
            };
            // The holds are named with the neighbour each conflicts against,
            // because a hold without its partner is indistinguishable from a
            // base that moved underneath the branch — and those have different
            // remedies.
            let held_note = if held.is_empty() {
                String::new()
            } else {
                format!(
                    " — {} branch{} held: {}",
                    held.len(),
                    if held.len() == 1 { "" } else { "es" },
                    held.iter()
                        .map(|h| format!("{} (conflicts with {})", h.branch, h.conflicts_with))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            match outcome.reason() {
                Some(reason) => out.push_str(&format!(
                    "  {glyph} rebuild {environment} — {reason}{held_note}\n"
                )),
                None => out.push_str(&format!("  {glyph} rebuild {environment}{held_note}\n")),
            }
        }
        AppliedEffect::PromotionPrune {
            environment,
            branches,
            ..
        } => out.push_str(&format!(
            "  ✓ pruned {} from {environment}\n",
            branch_list(branches)
        )),
    }
}

/// The post-condition, read from the snapshot the receipt carries.
///
/// Read, not recomputed: `resulting_state` comes from
/// `core::state::build_state_snapshot`, the single authority on "does this
/// environment need a rebuild". A receipt renderer that re-derived the verdict
/// would be a fourth place to answer that question — P3 counted three.
/// `LegacyUnknown` is spelled out rather than skipped, because a state that
/// renders as silence reads as good news, and P3 made that case first-class for
/// exactly this reason.
///
/// A different section from the effects above, and the heading is what keeps
/// them apart. A release moves its target ref *and* leaves every environment
/// based on that target behind, so the same `main 5e701ab → 7986a00` legitimately
/// appears twice in one receipt: once as an effect (what this operation did) and
/// once as `main moved` under `dev` (what that operation left behind). Those are
/// two true facts about two different subjects, and the reader can only tell
/// them apart because the second is a verdict read from the snapshot and the
/// first is a ref edit read back from the object database. Collapsing the
/// sections would lose the distinction; indenting the detail deeper than the
/// effect lines is what keeps the two kinds from sharing a column.
///
/// The heading is omitted when there is nothing to report, which `hitch remove`
/// is the first command to reach. Every other mutation has a named environment
/// in the snapshot by construction — the reader asked about `dev`, so `dev` is
/// in the answer — and `remove` is defined by making it stop being the case. A
/// bare `Result` heading with no lines under it reads as a rendering that broke,
/// which is the worst thing a receipt can say after it has just told the reader
/// it did the thing correctly.
///
/// The reader's actual question is answered elsewhere and more precisely: the
/// effect line above says `remove environment 'dev' from the declaration`, so
/// nothing is lost by not also opening a section that has no contents. This
/// follows `unaffected_resources_are_listed_and_the_heading_vanishes_when_empty`
/// for the same reason — a document section that has nothing in it should not
/// be on the page.
fn render_resulting_state(out: &mut String, snapshot: &RepositoryStateSnapshot) {
    if snapshot.environments.is_empty() {
        return;
    }
    out.push('\n');
    heading(out, "Result");
    for environment in &snapshot.environments {
        out.push_str(&format!(
            "  {} {}   {}\n",
            health_glyph(&environment.health),
            environment.name,
            environment.health.label()
        ));
        if let EnvironmentHealth::NeedsRebuild {
            changed_inputs,
            added,
            removed,
        } = &environment.health
        {
            // A removed branch is *necessarily* also a changed input, not
            // incidentally: `health_from_record` builds `changed_inputs` by
            // walking the branches the record pinned, and a branch that has
            // left the declaration resolves to no current SHA, so it fails that
            // comparison too. Printing both lines therefore said the same thing
            // twice, and the specific one ("removed from the declaration") is
            // the actionable one — its SHA is irrelevant now that it is out.
            //
            // `added` needs no such guard: it is drawn from the *desired*
            // branches and `changed_inputs` from the recorded ones, so the two
            // are disjoint by construction rather than by coincidence.
            for input in changed_inputs {
                if removed.contains(&input.branch) {
                    continue;
                }
                // "moved" so the line states what happened, in the same grammar
                // as the two declaration-change lines below it. Barely an
                // `old → new` arrow reads as a ref update and is not one.
                out.push_str(&format!(
                    "      {} moved   {} → {}\n",
                    input.branch,
                    input
                        .previous_sha
                        .as_deref()
                        .map(short)
                        .unwrap_or_else(|| "gone".into()),
                    input
                        .current_sha
                        .as_deref()
                        .map(short)
                        .unwrap_or_else(|| "gone".into())
                ));
            }
            for branch in added {
                out.push_str(&format!("      {branch} added to the declaration\n"));
            }
            for branch in removed {
                out.push_str(&format!("      {branch} removed from the declaration\n"));
            }
        }
        if let EnvironmentHealth::PartiallyRealised { held } = &environment.health {
            out.push_str(&format!("      held: {}\n", branch_list(held)));
        }
    }
}

/// Three glyphs, not two.
///
/// `is_actionable` alone is not enough to pick one, because it answers "is
/// there work owed" and not "is this known to be fine" — and
/// `EnvironmentHealth::LegacyUnknown` is neither. A ✓ beside "actual unknown"
/// would be a claim the model explicitly refuses to make, in the one place the
/// user is being told the result.
fn health_glyph(health: &EnvironmentHealth) -> &'static str {
    match health {
        EnvironmentHealth::Realised | EnvironmentHealth::PartiallyRealised { .. } => "✓",
        EnvironmentHealth::LegacyUnknown => "◌",
        EnvironmentHealth::NeedsRebuild { .. }
        | EnvironmentHealth::NeverBuilt
        | EnvironmentHealth::MissingBranch => "⧗",
    }
}

fn heading(out: &mut String, title: &str) {
    out.push_str(title);
    out.push('\n');
}

/// A glyph and a possibly multi-line message, with every continuation line
/// aligned under the text rather than under the glyph.
///
/// Not a nicety: several warnings embed their remedy on a second line
/// (`"'qa' will be left stale until it is rebuilt. To rebuild it:\n  hitch
/// rebuild qa"`), and left alone the remedy lands one column left of the text
/// it belongs to.
///
/// Render one `hitch why` explanation (spec §14).
///
/// Three forms, three shapes, and the shape is chosen by the *form* rather than
/// by which fields happen to be populated: a section that is present-but-empty
/// is worse than an absent one, and "present when there is something to say" is
/// the only rule that gets that right without a per-section emptiness check.
///
/// The equations go through [`render_equation`] rather than a second format, so
/// `hitch why dev` and a plan about `dev` cannot describe it differently —
/// which is §13's requirement and the reason the equation is a value.
pub fn render_why(explanation: &WhyExplanation) -> String {
    match explanation {
        WhyExplanation::FeatureInEnvironment(e) => render_why_feature_in(e),
        WhyExplanation::Feature(e) => render_why_feature(e),
        WhyExplanation::Environment(e) => render_why_environment(e),
    }
}

fn render_why_feature(e: &WhyFeatureExplanation) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", e.branch));

    if e.environments.is_empty() {
        out.push_str("\n  Not promoted to any environment.\n");
        out.push_str(&format!("\n{}\n", e.summary));
        return out.trim_end().to_string();
    }

    out.push('\n');
    let width = e
        .environments
        .iter()
        .map(|m| m.environment.chars().count())
        .max()
        .unwrap_or(0);
    for membership in &e.environments {
        out.push_str(&format!(
            "  {name:<width$}  {}\n",
            why_membership_label(membership.membership),
            name = membership.environment,
            width = width
        ));
        if let Some(reason) = &membership.reason {
            out.push_str(&format!(
                "{}{}\n",
                " ".repeat(width + 4),
                why_reason_sentence(reason)
            ));
        }
    }

    out.push_str(&format!("\n{}\n", e.summary));
    out.trim_end().to_string()
}

fn render_why_feature_in(e: &WhyFeatureInEnvironment) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} → {}\n", e.branch, e.environment));

    out.push('\n');
    heading(&mut out, "Desired");
    out.push_str(&format!("  {}\n", render_equation(&e.desired_equation)));

    // Absent, not empty. An environment with no trustworthy build has no actual
    // composition, and rendering `Actual\n  (nothing)` would be a claim hitch
    // cannot make.
    if let Some(actual) = &e.actual_equation {
        out.push('\n');
        heading(&mut out, "Actual");
        out.push_str(&format!("  {}\n", render_equation(actual)));
    }

    out.push('\n');
    heading(&mut out, "Membership");
    out.push_str(&format!("  {}\n", why_membership_label(e.membership)));

    // `Why?` appears only when there is a reason. A branch that is simply in the
    // build has nothing to explain, and a section saying so would be noise on
    // the most common query the command will ever answer.
    if let Some(reason) = &e.reason {
        out.push('\n');
        heading(&mut out, "Why?");
        out.push_str(&format!("  {}\n", why_reason_sentence(reason)));
        if let WhyReason::HeldAgainst { files, .. } = reason {
            if !files.is_empty() {
                out.push('\n');
                heading(&mut out, "Files");
                for file in files {
                    out.push_str(&format!("  {file}\n"));
                }
            }
        }
    }

    // Then the environment's own state, from the same function and in the same
    // order the environment form uses. The material above is about the branch;
    // this is about the environment the question was asked in, and a reader
    // about to promote needs both — "● Included" means something different in a
    // locked environment that is two rebuilds behind than in a current one.
    environment_state_block(&mut out, &e.environment, &e.health, e.locked);

    if !e.what_hitch_did.is_empty() {
        out.push('\n');
        heading(&mut out, "What Hitch did");
        for did in &e.what_hitch_did {
            match did {
                WhatHitchDid::Included { branch } => {
                    out.push_str(&format!("  Included {branch} in the build\n"))
                }
                WhatHitchDid::Held {
                    branch,
                    conflicts_with,
                } => out.push_str(&format!(
                    "  Excluded {branch} — it conflicts with {conflicts_with}\n"
                )),
                WhatHitchDid::ReplayedResolution { branch, key } => out.push_str(&format!(
                    "  Replayed the recorded resolution for {branch} ({key})\n"
                )),
            }
        }
    }

    if let Some(action) = &e.next_action {
        out.push('\n');
        heading(&mut out, "Next");
        match next_action_command(action) {
            NextStep::Command(command) => out.push_str(&format!("  {command}\n")),
            NextStep::Advice(advice) => out.push_str(&format!("  {advice}\n")),
            NextStep::Nothing => {}
        }
    }

    out.trim_end().to_string()
}

fn render_why_environment(e: &WhyEnvironmentExplanation) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", e.environment));

    out.push('\n');
    heading(&mut out, "Desired");
    out.push_str(&format!("  {}\n", render_equation(&e.desired_equation)));

    if let Some(actual) = &e.actual_equation {
        out.push('\n');
        heading(&mut out, "Actual");
        out.push_str(&format!("  {}\n", render_equation(actual)));
    }

    if !e.branches.is_empty() {
        out.push('\n');
        heading(&mut out, "Branches");
        let width = e
            .branches
            .iter()
            .map(|b| b.branch.chars().count())
            .max()
            .unwrap_or(0);
        for branch in &e.branches {
            out.push_str(&format!(
                "  {name:<width$}  {}\n",
                why_membership_label(branch.membership),
                name = branch.branch,
                width = width
            ));
            if let Some(reason) = &branch.reason {
                out.push_str(&format!(
                    "{}{}\n",
                    " ".repeat(width + 4),
                    why_reason_sentence(reason)
                ));
            }
        }
    }

    // The verdict as a sentence, then the lock if there is one. `EnvironmentHealth::label`
    // is a bare predicate and half of them do not fit "X is <label>": `needs rebuild`
    // yields "dev is needs rebuild". The sentence form is a *display* choice, so it
    // lives here and is derived from the same enum — which is what keeps the
    // verdict itself computed in exactly one place.
    environment_state_block(&mut out, &e.environment, &e.health, e.locked);

    if let Some(action) = &e.next_action {
        out.push('\n');
        heading(&mut out, "Next");
        match next_action_command(action) {
            NextStep::Command(command) => out.push_str(&format!("  {command}\n")),
            NextStep::Advice(advice) => out.push_str(&format!("  {advice}\n")),
            NextStep::Nothing => {}
        }
    }

    out.trim_end().to_string()
}

/// The environment's own state: one verdict sentence, plus what a human lock
/// will refuse.
///
/// Shared by both renderers that display an environment, because the two forms
/// are two views of the *same* snapshot and a reader running
/// `hitch why <branch> <env>` must not learn something different about that
/// environment than `hitch why <env>` tells them. One function rather than two
/// call sites that each build the same two sentences, for the same reason
/// `build_status_model` is a projection: two formatters can disagree, and here
/// they would disagree about a verdict.
fn environment_state_block(
    out: &mut String,
    environment: &str,
    health: &crate::core::state::EnvironmentHealth,
    locked: bool,
) {
    out.push_str(&format!("\n{}\n", health_sentence(environment, health)));
    if locked {
        out.push_str(&format!("{}\n", lock_sentence(environment)));
    }
}

/// What a human lock will refuse, as one sentence.
///
/// Its own function only because it is a *sentence about a consequence* rather
/// than a restatement of the flag, and the consequence has a command in it. The
/// caller decides whether the environment is locked; this decides what that
/// means.
fn lock_sentence(environment: &str) -> String {
    format!(
        "{environment} is locked, so it will refuse a promote until 'hitch unlock {environment}'."
    )
}

/// The one place that writes a `why` membership's words.
///
/// Same reasoning as the matrix: the glyph and the word both carry the state, so
/// the text survives a terminal with colour disabled and a screen reader, and a
/// pipe into `grep`. It is a *different* function from `MatrixCell::label` on
/// purpose — §14 capitalises them (`Included`, `⛔ Held`) and the two are
/// displayed at different sizes — but both are projections of the same
/// classifier, so the states themselves cannot drift.
pub fn why_membership_label(membership: WhyMembership) -> String {
    let (glyph, word) = match membership {
        WhyMembership::NotDesired => ("—", "Not desired"),
        WhyMembership::Included => ("●", "Included"),
        WhyMembership::Held => ("⛔", "Held"),
        WhyMembership::InBase => ("=", "In base"),
        WhyMembership::NeedsRebuild => ("↻", "Needs rebuild"),
        WhyMembership::ActualUnknown => ("?", "Actual unknown"),
        WhyMembership::Missing => ("!", "Missing"),
    };
    format!("{glyph} {word}")
}

/// A `Next` step, which is either a command to paste or a sentence, or nothing.
///
/// `Debug`/`PartialEq` because a `Next` step is a claim about what a user should
/// run, and "the renderer maps this action to this command" is a testable
/// property rather than something to check by eye. It was previously neither,
/// which meant the mapping had no test at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    Command(String),
    Advice(String),
    Nothing,
}

/// The one place that writes the words of a next action.
///
/// A table rather than a `match` inside three renderers, because the argument
/// order of `hitch resolve` (`<env> --branch <branch>`, per
/// `src/commands/resolve.rs`) is the kind of detail that has to be right in
/// exactly one place. `NextAction::None` renders as advice, never as a command:
/// "there is nothing to run" and "here is a command" are different claims, and
/// printing a command for a state no command can fix would be the lie.
pub fn next_action_command(action: &NextAction) -> NextStep {
    match action {
        NextAction::Rebuild(environment) => {
            NextStep::Command(format!("hitch rebuild {environment}"))
        }
        NextAction::Resolve {
            environment,
            branch,
        } => NextStep::Command(format!("hitch resolve {environment} --branch {branch}")),
        NextAction::Demote {
            environment,
            branch,
        } => NextStep::Command(format!("hitch demote {branch} {environment}")),
        NextAction::Promote {
            branch,
            environment,
        } => NextStep::Command(format!("hitch promote {branch} {environment}")),
        NextAction::None { reason } => NextStep::Advice(reason.clone()),
    }
}

/// An environment's health as one grammatical sentence.
///
/// Every arm is a `match` on [`EnvironmentHealth`] with no wildcard, for the
/// same reason [`MatrixCell::classify`] has none: a new variant added to the
/// model should fail to compile here rather than render as a sentence with an
/// `{}` hole in it.
fn health_sentence(environment: &str, health: &EnvironmentHealth) -> String {
    match health {
        EnvironmentHealth::Realised => {
            format!("{environment} is realised: the build matches its declaration.")
        }
        EnvironmentHealth::PartiallyRealised { held } => {
            format!(
                "{environment} is partially realised: {} held out of the build.",
                held.join(", ")
            )
        }
        EnvironmentHealth::NeedsRebuild { .. } => {
            format!("{environment} needs a rebuild: its inputs have moved since the last build.")
        }
        EnvironmentHealth::NeverBuilt => {
            format!("{environment} has never been built.")
        }
        EnvironmentHealth::LegacyUnknown => {
            format!("{environment} has a build hitch cannot describe.")
        }
        EnvironmentHealth::MissingBranch => {
            format!("{environment}'s branch does not exist locally.")
        }
    }
}

/// One reason, as a sentence.
///
/// Every variant is a *fact* read from the snapshot. None of them mentions a
/// branch that is not the subject, and none of them is hedged — a `why` that
/// said "may be held" would be indistinguishable from the prediction-displayed
/// call sites this program exists to separate from fact.
fn why_reason_sentence(reason: &WhyReason) -> String {
    match reason {
        WhyReason::HeldAgainst { conflicts_with, .. } => {
            format!("it conflicts with {conflicts_with}")
        }
        WhyReason::ChangedSinceBuild { from, to } => {
            format!("it moved since the last build ({from} → {to})")
        }
        WhyReason::BaseMoved { from, to } => {
            format!("the environment's base moved since the last build ({from} → {to})")
        }
        WhyReason::PromotedSinceBuild => {
            "it was promoted after the last build ran".to_string()
        }
        WhyReason::DemotedSinceBuild => {
            "it was demoted after the last build ran, so the build still contains it".to_string()
        }
        WhyReason::NoRef => "no branch by that name exists, locally or on origin".to_string(),
        WhyReason::NoBuildRecord => {
            "hitch has no build record for this environment, so it cannot say what the last build contained".to_string()
        }
        WhyReason::EnvironmentBranchMissing => {
            "the environment branch does not exist".to_string()
        }
        WhyReason::AlreadyInBase => {
            "its tip is already reachable from the environment's base, so building it would fold in nothing"
                .to_string()
        }
        WhyReason::EnvironmentBehind => {
            "the environment is behind its declaration, for a reason that is not about this branch"
                .to_string()
        }
    }
}

fn annotated(out: &mut String, glyph: &str, message: &str) {
    for (index, line) in message.lines().enumerate() {
        if index == 0 {
            out.push_str(&format!("  {glyph} {line}\n"));
        } else {
            out.push_str(&format!("     {line}\n"));
        }
    }
}

// ---------------------------------------------------------------------------
// The impure half. Everything above is a pure function of the model; what
// follows is where a rendered `String` meets a channel and a terminal.
// ---------------------------------------------------------------------------

/// The version of the `--json` document's shape.
///
/// Present from the first release, and the only sanctioned way to change the
/// document: a field added here is additive and leaves `1` intact, and anything
/// else is a new version. A consumer that reads this and refuses an unknown
/// value is then protected from every change, which is the whole point of
/// versioning rather than promising the bytes will never move.
pub const JSON_SCHEMA_VERSION: u32 = 1;

/// What a mutating command emits: the plan it decided on, and the receipt for
/// the apply — either of which may be absent, and which of them is absent tells
/// you what happened.
///
/// `receipt: null` rather than an omitted key, deliberately. A consumer reading
/// one field then finds either an object or an explicit "there was none", where
/// a missing key would be indistinguishable from a version of hitch that never
/// emitted one.
#[derive(Debug, serde::Serialize)]
pub struct JsonDocument<'a, I> {
    pub schema_version: u32,
    pub plan: Option<&'a OperationPlan<I>>,
    pub receipt: Option<&'a crate::operations::model::ExecutionReceipt>,
    /// What the apply attempted and could not do. Absent (not `[]`) when nothing
    /// failed, so every other command's document is byte-for-byte what it was.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<ApplyFailure>,
}

/// One step an apply attempted and git refused, as a typed record.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ApplyFailure {
    pub refname: String,
    pub cause: String,
}

impl std::fmt::Display for ApplyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.refname, self.cause)
    }
}

impl<'a, I> JsonDocument<'a, I> {
    /// The document for a command that planned and did not apply — a
    /// `--dry-run`, or a plan the apply refused before writing anything.
    pub fn preview(plan: &'a OperationPlan<I>) -> Self {
        Self {
            schema_version: JSON_SCHEMA_VERSION,
            plan: Some(plan),
            receipt: None,
            failures: Vec::new(),
        }
    }

    /// The document for a command that planned and applied.
    pub fn applied(plan: &'a OperationPlan<I>, receipt: &'a ExecutionReceipt) -> Self {
        Self {
            schema_version: JSON_SCHEMA_VERSION,
            plan: Some(plan),
            receipt: Some(receipt),
            failures: Vec::new(),
        }
    }
}

/// Write a document to stdout, pretty-printed, followed by exactly one newline.
///
/// The explicit flush is load-bearing for the same reason `main.rs`'s is before
/// `process::exit(2)`: `process::exit` skips normal shutdown, so a rebuild that
/// exits 2 with branches held would otherwise lose the very document that
/// explains why. A CI job that cannot read the JSON is the failure this flag
/// exists to prevent.
///
/// `to_string_pretty` over `to_string` because the document is read by humans in
/// a terminal about as often as by programs, and a single-line plan object is
/// not reviewable in a CI log.
pub fn emit_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|e| anyhow::anyhow!("Could not render the JSON document: {e}"))?;
    println!("{body}");
    use std::io::Write;
    std::io::stdout()
        .flush()
        .context("Failed to flush the JSON document to stdout")?;
    Ok(())
}

/// A vote that was recorded and did not reach its threshold.
///
/// The one `approvals approve` outcome with no plan and no receipt: nothing was
/// declared or rebuilt, only the request moved. It is a fact about the request,
/// so it gets its own small shape rather than a fabricated plan whose effects
/// would be empty — an empty receipt would say "nothing happened", which is the
/// opposite of true.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ApprovalRecorded {
    pub request_id: String,
    pub environment: String,
    pub approvals: usize,
    pub required: usize,
    pub threshold_met: bool,
    pub remaining_approvers: Vec<String>,
}

/// The words for an [`ApprovalRecorded`]. Pure, like the other renderers.
pub fn render_approval_recorded(vote: &ApprovalRecorded) -> String {
    let mut out = format!(
        "Approval recorded ({}/{}). Waiting for {} more approval(s) from:",
        vote.approvals,
        vote.required,
        vote.required.saturating_sub(vote.approvals)
    );
    for approver in &vote.remaining_approvers {
        out.push_str(&format!("\n  - {approver}"));
    }
    out
}

/// Emit a recorded vote: text on the sink, or under `--json` a document with the
/// same `schema_version` and `plan`/`receipt` keys as every other mutation
/// (both `null`, since there was neither) plus an `approval` object.
pub fn emit_approval_recorded(
    context: &GlobalContext,
    vote: &ApprovalRecorded,
) -> anyhow::Result<()> {
    if context.json {
        return emit_json(&serde_json::json!({
            "schema_version": JSON_SCHEMA_VERSION,
            "plan": null,
            "receipt": null,
            "approval": vote,
        }));
    }
    context
        .output
        .log(OutputLevel::Info, &render_approval_recorded(vote));
    Ok(())
}

/// Emit a plan, for a command that decided and stopped. A `--dry-run`, or any
/// other path that plans without applying.
///
/// Under `--json` this is a document with `receipt: null`. Otherwise it is the
/// rendered plan, on the sink.
///
/// It exists beside [`emit_receipt`] rather than as a mode of one function
/// because the two are *not* alternatives: on the applying path the plan has
/// already been shown, by the gate, before the apply — so emitting it again here
/// would print the same forty lines twice with a receipt in between.
pub fn emit_plan<I: serde::Serialize>(
    context: &GlobalContext,
    plan: &OperationPlan<I>,
) -> anyhow::Result<()> {
    if context.json {
        return emit_json(&JsonDocument::preview(plan));
    }
    context.output.log(OutputLevel::Info, &render_plan(plan));
    Ok(())
}

/// Say that a `--dry-run` printed a plan and stopped there.
///
/// A plan alone reads like the first half of an operation that then ran; this is
/// the line that says it did not. `detail` is what to add after the dash — a
/// command with a flag to re-run with names it there.
pub fn emit_preview_note(context: &GlobalContext, detail: &str) {
    context
        .output
        .log(OutputLevel::Info, &format!("(preview — {detail})"));
}

/// Emit the receipt, for a command that applied.
///
/// Under `--json` this is a document carrying the plan *and* the receipt, so a
/// machine consumer gets one object describing the whole operation rather than
/// two it has to correlate. Otherwise it is the rendered receipt alone, because
/// the plan above it on this terminal was [`confirm_plan`]'s to print.
///
/// The text goes through the sink rather than `println!` even though this branch
/// is unreachable under `--json`. Routing through the sink is what makes it
/// unreachable: a `println!` here would be a claim about a flag that a later
/// change could quietly falsify.
pub fn emit_receipt<I: serde::Serialize>(
    context: &GlobalContext,
    plan: &OperationPlan<I>,
    receipt: &ExecutionReceipt,
) -> anyhow::Result<()> {
    if context.json {
        return emit_json(&JsonDocument::applied(plan, receipt));
    }
    context
        .output
        .log(OutputLevel::Info, &render_receipt(receipt));
    Ok(())
}

/// [`emit_receipt`] for an apply that also failed in part: the document carries
/// the failures next to the receipt of what did apply. The caller still fails
/// the command afterwards — the document is the account, not the verdict.
pub fn emit_receipt_with_failures<I: serde::Serialize>(
    context: &GlobalContext,
    plan: &OperationPlan<I>,
    receipt: &ExecutionReceipt,
    failures: &[ApplyFailure],
) -> anyhow::Result<()> {
    if context.json {
        let mut document = JsonDocument::applied(plan, receipt);
        document.failures = failures.to_vec();
        return emit_json(&document);
    }
    emit_receipt(context, plan, receipt)
}

/// What the confirmation gate decides, before anything is printed or asked.
///
/// Split out from [`confirm_plan`] so the decision is a pure function of the
/// flags and the plan's own `confirmation.required`, and can be tested without
/// a repository. The gate has three outcomes rather than two, and the third is
/// the one that is easy to get wrong by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    /// Apply it. Either `--yes` said so, or there was nothing to authorise.
    Proceed,
    /// Show the plan and put the question to a human.
    Ask,
    /// Refuse, with the reason. Nothing is printed and nothing is written.
    Refuse(String),
}

/// Decide what to do about a plan before touching a channel.
///
/// `--json` without `--yes` is [`GateDecision::Refuse`], and specifically not
/// "ask anyway": a JSON consumer is a program, and a program that blocks on a
/// terminal read is the failure this flag removes. The reason names the flag,
/// because a refusal that does not say what to do about it is a worse failure
/// than the hang would have been.
pub fn decide_gate(assume_yes: bool, json: bool, required: bool) -> GateDecision {
    if assume_yes {
        return GateDecision::Proceed;
    }
    if json {
        return GateDecision::Refuse(
            "Refusing to prompt under --json: this operation needs confirmation.\n\
             Re-run with --yes (or set HITCH_YES=1) to confirm without a terminal."
                .to_string(),
        );
    }
    if required {
        GateDecision::Ask
    } else {
        // Nothing to authorise. Still shown — §10.2 is explicit that the plan
        // is not conditional on a prompt existing — but not asked about, so a
        // caller cannot accidentally gate an operation that declared no gate.
        GateDecision::Proceed
    }
}

/// The question to put to a human, built from the plan's own stated reason.
///
/// Pure, and a function rather than an inline `format!`, because a prompt that
/// does not say what is being authorised is the one piece of this gate that
/// cannot be verified by looking at the exit code: `ConfirmationRequirement`
/// carried a `reason` that no code path ever printed, so every prompt in the
/// CLI read `Apply this plan?` — including the promote into an approval-gated
/// environment, where the plan's *Will change* section is empty because the
/// apply will file an approval request instead of changing anything. The user
/// was asked to authorise a plan that visibly does nothing, with no statement
/// of what the answer would actually do.
///
/// The reason goes *above* the question rather than inside it: `Confirm`
/// prefixes an emoji and a level to what it is given, so a multi-line reason
/// inside the prompt reads as a log message that swallowed a question.
pub fn confirmation_question(requirement: &ConfirmationRequirement) -> String {
    match requirement.reason.as_deref().map(str::trim) {
        // A reason that is absent, empty, or only whitespace is no reason at
        // all, and printing an empty line above the question would look like a
        // truncated plan.
        Some(reason) if !reason.is_empty() => format!("{reason}\nApply this plan?"),
        _ => "Apply this plan?".to_string(),
    }
}

/// Show a plan and ask whether to apply it.
///
/// The decision is [`decide_gate`]'s; this is the side of it. The plan goes to
/// the sink (stdout normally, stderr under `--json`) and the question is asked
/// on the same channel, so `--json` gets a clean stdout and a person sees the
/// plan above the question.
///
/// The plan is *not* embedded in the question for the reason given on
/// [`confirmation_question`].
///
/// `Ok(false)` means nothing has happened and the caller should report that and
/// return success — a declined confirmation is not a failure.
pub fn confirm_plan(
    context: &GlobalContext,
    rendered: &str,
    requirement: &ConfirmationRequirement,
) -> anyhow::Result<bool> {
    match decide_gate(context.assume_yes, context.json, requirement.required) {
        GateDecision::Refuse(reason) => anyhow::bail!(reason),
        GateDecision::Proceed => {
            // Printed even under `--yes`: §10.2 requires that `--yes` skip the
            // prompt, not the plan.
            context.output.log(OutputLevel::Info, rendered);
            Ok(true)
        }
        GateDecision::Ask => {
            context.output.log(OutputLevel::Info, rendered);
            context.confirm(&confirmation_question(requirement))
        }
    }
}

/// One event as one sentence: no actor, no time, no glyph.
///
pub fn render_event(event: &HitchEvent) -> String {
    match event {
        HitchEvent::EnvironmentCreated { environment, base } => {
            format!("created environment {environment} from {base}")
        }
        HitchEvent::EnvironmentRemoved { environment } => {
            format!("removed environment {environment}")
        }
        HitchEvent::BaseChanged {
            environment,
            from,
            to,
        } => format!("changed {environment}'s base from {from} to {to}"),
        HitchEvent::Promoted {
            environment,
            branch,
        } => format!("added {branch} to {environment}"),
        HitchEvent::Demoted {
            environment,
            branch,
        } => format!("removed {branch} from {environment}"),
        HitchEvent::Locked { environment, .. } => format!("locked {environment}"),
        HitchEvent::Unlocked { environment } => format!("unlocked {environment}"),
        HitchEvent::Rebuilt {
            environment,
            outcome: RebuildOutcome::WithHolds { held, .. },
        } if !held.is_empty() => {
            let mut names: Vec<&str> = Vec::new();
            for pair in held {
                if !names.contains(&pair.branch.as_str()) {
                    names.push(&pair.branch);
                }
            }
            format!("rebuilt {environment}, holding {}", names.join(", "))
        }
        HitchEvent::Rebuilt { environment, .. } => format!("rebuilt {environment}"),
        HitchEvent::Released { environment } => format!("released {environment}"),
        HitchEvent::ApprovalRequested {
            environment,
            branch,
            direction,
            ..
        } => match direction {
            ApprovalDirection::Promote => format!("asked to add {branch} to {environment}"),
            ApprovalDirection::Demote => format!("asked to remove {branch} from {environment}"),
        },
        HitchEvent::ApprovalVoted {
            environment,
            branch,
            approvals,
            required,
            direction,
            ..
        } => format!(
            "approved {} ({approvals} of {required})",
            change_phrase(*direction, branch, environment)
        ),
        HitchEvent::ApprovalGranted {
            environment,
            branch,
            direction,
            ..
        } => format!(
            "{} is approved",
            change_phrase(*direction, branch, environment)
        ),
        HitchEvent::ApprovalRejected {
            environment,
            branch,
            direction,
            ..
        } => format!(
            "rejected {}",
            change_phrase(*direction, branch, environment)
        ),
        HitchEvent::ApprovalApplied {
            environment,
            branch,
            direction,
            ..
        } => match direction {
            ApprovalDirection::Promote => {
                format!("applied the approved change: {branch} to {environment}")
            }
            ApprovalDirection::Demote => {
                format!("applied the approved change: {branch} out of {environment}")
            }
        },
        HitchEvent::ApprovalCancelled {
            environment,
            branch,
            direction,
            ..
        } => match direction {
            ApprovalDirection::Promote => {
                format!("cancelled the request to add {branch} to {environment}")
            }
            ApprovalDirection::Demote => {
                format!("cancelled the request to remove {branch} from {environment}")
            }
        },
    }
}

fn change_phrase(direction: ApprovalDirection, branch: &str, environment: &str) -> String {
    match direction {
        ApprovalDirection::Promote => format!("adding {branch} to {environment}"),
        ApprovalDirection::Demote => format!("removing {branch} from {environment}"),
    }
}

fn activity_hold_sentence(pair: &HoldPair) -> String {
    format!(
        "{} was held \u{2014} it conflicts with {}",
        pair.branch, pair.conflicts_with
    )
}

/// The deployment story, newest first. Pure: `now` is only used to place day
/// headings. The "not rebuilt since" line is a statement about this log, not a
/// verdict — whether the environment needs a rebuild belongs to `core::state`.
pub fn render_activity(
    log: &ActivityLog,
    now: chrono::DateTime<chrono::FixedOffset>,
    verbose: bool,
) -> String {
    use std::collections::{BTreeMap, BTreeSet};

    if log.entries.is_empty() && log.skipped.is_empty() && !log.truncated {
        return "No activity yet.\n".to_string();
    }

    // entry index -> environments whose newest touching entry is a declaration change
    let mut pointers: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    let mut decided: BTreeSet<&str> = BTreeSet::new();
    // A branch filter removes events from entries, so absence proves nothing:
    // "not rebuilt since" would be claimed from a log that hides the rebuild.
    // An unreadable (skipped) commit can hide one just the same.
    let pointer_entries = if log.branch_filtered || !log.skipped.is_empty() {
        0
    } else {
        log.entries.len()
    };
    for (index, entry) in log.entries.iter().enumerate().take(pointer_entries) {
        let envs: BTreeSet<&str> = entry.events.iter().map(|e| e.environment()).collect();
        for env in envs {
            if decided.contains(env) {
                continue;
            }
            if entry
                .events
                .iter()
                .filter(|e| e.environment() == env)
                .all(|e| matches!(e, HitchEvent::Locked { .. } | HitchEvent::Unlocked { .. }))
            {
                continue;
            }
            decided.insert(env);
            let mut rebuilt = false;
            let mut declared = false;
            for event in entry.events.iter().filter(|e| e.environment() == env) {
                match event {
                    HitchEvent::Rebuilt { .. } => rebuilt = true,
                    HitchEvent::Promoted { .. }
                    | HitchEvent::Demoted { .. }
                    | HitchEvent::BaseChanged { .. } => declared = true,
                    _ => {}
                }
            }
            if declared && !rebuilt {
                pointers.entry(index).or_default().insert(env);
            }
        }
    }

    let offset = *now.offset();
    let today = now.date_naive();
    let mut out = String::new();
    let mut current_day = None;
    for (index, entry) in log.entries.iter().enumerate() {
        let local = entry.when.with_timezone(&offset);
        let day = local.date_naive();
        if current_day != Some(day) {
            if current_day.is_some() {
                out.push('\n');
            }
            let heading = if day == today {
                "Today".to_string()
            } else if today.pred_opt() == Some(day) {
                "Yesterday".to_string()
            } else {
                local.format("%a %-d %b %Y").to_string()
            };
            out.push_str(&heading);
            out.push('\n');
            current_day = Some(day);
        }
        let time = local.format("%H:%M");
        let pad = "         ";
        let mut lines: Vec<String> = Vec::new();
        for event in &entry.events {
            let voted_here = |id: &str| {
                entry.events.iter().any(
                    |e| matches!(e, HitchEvent::ApprovalVoted { request_id, .. } if request_id == id),
                )
            };
            match event {
                // The vote line already says the request is approved.
                HitchEvent::ApprovalGranted { request_id, .. } if voted_here(request_id) => {
                    continue
                }
                HitchEvent::ApprovalVoted { request_id, .. }
                    if entry.events.iter().any(|e| {
                        matches!(e, HitchEvent::ApprovalGranted { request_id: r, .. } if r == request_id)
                    }) =>
                {
                    lines.push(format!(
                        "{}, which completes the approval",
                        render_event(event)
                    ));
                    continue;
                }
                _ => {}
            }
            lines.push(render_event(event));
            if let HitchEvent::Rebuilt {
                outcome: RebuildOutcome::WithHolds { held, .. },
                ..
            } = event
            {
                lines.extend(held.iter().map(activity_hold_sentence));
            }
        }
        if let Some(envs) = pointers.get(&index) {
            for env in envs {
                lines.push(format!(
                    "{env} has not been rebuilt since \u{2014} see hitch status"
                ));
            }
        }
        // build_activity drops event-less entries, but this renderer is pure over
        // any log, so an empty one still gets its actor line (and verbose suffix).
        if lines.is_empty() {
            lines.push(String::new());
        }
        for (n, line) in lines.iter().enumerate() {
            if n == 0 {
                out.push_str(format!("  {time}  {} {line}", entry.actor).trim_end());
                if verbose {
                    let short: String = entry.commit.chars().take(7).collect();
                    out.push_str(&format!("  (metadata commit {short})"));
                }
            } else {
                out.push_str(&format!("{pad}{line}"));
            }
            out.push('\n');
        }
    }

    if !log.skipped.is_empty() || log.truncated {
        if !log.entries.is_empty() {
            out.push('\n');
        }
        if !log.skipped.is_empty() {
            let n = log.skipped.len();
            out.push_str(&format!(
                "{n} change{} to hitch's settings could not be read and {} not shown.\n",
                if n == 1 { "" } else { "s" },
                if n == 1 { "is" } else { "are" }
            ));
        }
        if log.truncated {
            out.push_str("Older activity not shown \u{2014} use --limit to see more.\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{
        ActualComposition, ApprovalPolicy, ChangedInput, DeclaredBranch, DesiredComposition,
        EnvironmentHealth, EnvironmentState,
    };
    use crate::core::status::{MatrixCell, MatrixRow};
    use crate::operations::model::{
        CompositionPlan, ConfirmationRequirement, DependentRebuildOutcome, EnvironmentField,
        EnvironmentFieldChange, EnvironmentFieldValue, ExecutionReceipt, ExecutionWarning,
        HoldPair, OperationIntent, OperationKind, PlanFingerprint, PlanWarning, PlanWarningKind,
        ResourceKind, UnaffectedResource, OPERATION_KINDS,
    };
    use crate::utils::build_record::PinnedBranch;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    struct Detail;

    fn sha(seed: char) -> String {
        seed.to_string().repeat(40)
    }

    fn pinned(branch: &str, sha: &str) -> PinnedBranch {
        PinnedBranch {
            branch: branch.to_string(),
            sha: sha.to_string(),
        }
    }

    fn planned(branch: &str, sha: &str, state: PlannedBranchState) -> PlannedBranch {
        PlannedBranch {
            branch: branch.to_string(),
            sha: sha.to_string(),
            state,
        }
    }

    fn projection(
        environment: &str,
        base: &str,
        branches: &[(&str, &str)],
    ) -> EnvironmentProjection {
        EnvironmentProjection {
            environment: environment.to_string(),
            base: base.to_string(),
            branches: branches.iter().map(|(b, s)| pinned(b, s)).collect(),
            branch_sha: Some(sha('a')),
        }
    }

    /// A plan with everything empty, so each test sets only the field it is
    /// about. A fixture that is mostly real data means a rendering change for an
    /// unrelated reason shows up as a failure in the wrong test.
    fn plan(intent: OperationIntent) -> OperationPlan<Detail> {
        OperationPlan {
            id: "test-plan".to_string(),
            kind: OperationKind::Promote,
            intent,
            fingerprint: PlanFingerprint::new(),
            // Equal on both sides, so the fixture has no transition in it. Every
            // test that is *about* a projection therefore has to differ the two
            // explicitly, which is the property the three-arm rule is for: a test
            // that sets only `proposed` and finds nothing rendered is being told
            // it has not actually made a transition.
            current: Some(projection("dev", "main", &[])),
            proposed: Some(projection("dev", "main", &[])),
            compositions: Vec::new(),
            effects: Vec::new(),
            unaffected: Vec::new(),
            warnings: Vec::new(),
            confirmation: ConfirmationRequirement::not_required(),
            detail: Detail,
        }
    }

    /// Whether some line of `rendered` is exactly `expected`.
    ///
    /// Compared as a whole line rather than as a substring so a test pins
    /// *content*, not the column padding that happens to surround it — a
    /// renderer is free to widen its table when one entry is long, and a test
    /// that failed when it did would be asserting a layout decision rather than
    /// a fact about the model.
    fn has_line(rendered: &str, expected: &str) -> bool {
        rendered.lines().any(|l| l == expected)
    }

    /// Whether a run of consecutive lines matches `block` exactly.
    ///
    /// Used for the multi-line sections, and for the same reason as
    /// [`has_line`]: a substring assertion on a block that happens to be last
    /// depends on a trailing newline that `render_plan` deliberately trims.
    fn has_block(rendered: &str, block: &str) -> bool {
        let lines: Vec<&str> = rendered.lines().collect();
        let expected: Vec<&str> = block.lines().collect();
        lines
            .windows(expected.len())
            .any(|w| w == expected.as_slice())
    }

    /// The one boundary the anchor split has to get exactly right.
    ///
    /// Written as a table because the interesting half is what must *not*
    /// match: a predicate loose enough to swallow `refs/hitch/state/` would
    /// stop reporting the build record, which is a surviving write and one of
    /// the two things the record exists to explain. A prefix test on
    /// `refs/hitch/` would have passed every positive case here and failed that
    /// one silently.
    #[test]
    fn only_the_two_anchor_families_read_as_transient() {
        for anchor in [
            "refs/hitch/build/dev/20260926110859",
            "refs/hitch/release/main/20260926110859",
        ] {
            assert!(
                is_transient_anchor(anchor),
                "{anchor} is created and removed by the operation that names it"
            );
        }
        for durable in [
            // The build record: one per environment, replaced in place, and the
            // current answer to "what is in this environment branch".
            "refs/hitch/state/dev",
            // What `hitch rollback` reaches for.
            "refs/hitch/prev/dev/20260926110859",
            "refs/hitch/backup/dev/20260926110859",
            // The publish journal: transient in the everyday sense, and still
            // not this, because a crash leaves it behind and `recover` reads it.
            "refs/hitch/publish/dev",
            // The two things a plan exists to move.
            "refs/heads/dev",
            "refs/remotes/origin/dev",
        ] {
            assert!(
                !is_transient_anchor(durable),
                "{durable} survives the operation, so listing it as a \
                 transient would be a lie in the other direction"
            );
        }
    }

    fn composition_of(branches: Vec<PlannedBranch>) -> Vec<CompositionPlan> {
        vec![CompositionPlan {
            environment: "dev".into(),
            base: pinned("main", &sha('9')),
            branches,
            result_sha: sha('f'),
            holds: Vec::new(),
        }]
    }

    // ---- headline --------------------------------------------------------

    #[test]
    fn the_headline_names_the_operation_the_intent_describes() {
        let cases = [
            (
                OperationIntent::RebuildEnvironment {
                    environment: "dev".into(),
                },
                "Rebuild dev",
            ),
            (
                OperationIntent::PromoteBranches {
                    environment: "dev".into(),
                    branches: vec!["feature/login".into()],
                },
                "Promote feature/login → dev",
            ),
            (
                OperationIntent::DemoteBranches {
                    environment: "qa".into(),
                    branches: vec!["old-thing".into()],
                },
                "Demote old-thing → qa",
            ),
            (
                OperationIntent::ReleaseEnvironment {
                    environment: "dev".into(),
                    target: "main".into(),
                },
                "Release dev → main",
            ),
            (
                OperationIntent::LockEnvironment {
                    environment: "dev".into(),
                },
                "Lock dev",
            ),
            (
                OperationIntent::UnlockEnvironment {
                    environment: "dev".into(),
                },
                "Unlock dev",
            ),
            (
                OperationIntent::SetEnvironment {
                    environment: "dev".into(),
                    changes: vec![EnvironmentFieldChange {
                        field: EnvironmentField::Base,
                        old: EnvironmentFieldValue::Branch("main".into()),
                        new: EnvironmentFieldValue::Branch("develop".into()),
                    }],
                },
                "Set dev · 1 setting",
            ),
            (
                OperationIntent::AddEnvironment {
                    environment: "staging".into(),
                    base: "main".into(),
                },
                "Add staging on main",
            ),
            (
                OperationIntent::RemoveEnvironment {
                    environment: "legacy".into(),
                },
                "Remove legacy",
            ),
            (
                OperationIntent::Cleanup {
                    candidates: vec!["refs/hitch/backup/a".into()],
                },
                "Clean up 1 archived build",
            ),
            (
                OperationIntent::ApplyApproval {
                    environment: "production".into(),
                    request_id: "req-7".into(),
                    branches: vec!["feature/login".into()],
                },
                "Approve feature/login → production",
            ),
        ];
        for (intent, expected) in cases {
            assert_eq!(plan_headline(&plan(intent)), expected);
        }
    }

    /// Every `OperationKind` reaches a headline, and none of them collides.
    ///
    /// The table above is a list of what the eleven intents happen to say, and a
    /// list is not a total function: an intent with no arm, or two that render
    /// the same words, both pass it. This walks `OPERATION_KINDS` — the constant
    /// a new kind is added to — so a twelfth kind fails here rather than in a
    /// document. Pairing each kind with a representative intent is the only way
    /// to say "a kind and an intent are not the same thing": `SetEnvironment` and
    /// `LockEnvironment` are different kinds whose headlines are both one verb
    /// away from each other, and a kind/intent mixup between them would be
    /// invisible to a test that only looked at intents.
    #[test]
    fn every_operation_kind_has_a_distinct_headline() {
        let representatives = [
            (
                OperationKind::Rebuild,
                OperationIntent::RebuildEnvironment {
                    environment: "dev".into(),
                },
            ),
            (
                OperationKind::Promote,
                OperationIntent::PromoteBranches {
                    environment: "dev".into(),
                    branches: vec!["a".into()],
                },
            ),
            (
                OperationKind::Demote,
                OperationIntent::DemoteBranches {
                    environment: "dev".into(),
                    branches: vec!["a".into()],
                },
            ),
            (
                OperationKind::Release,
                OperationIntent::ReleaseEnvironment {
                    environment: "dev".into(),
                    target: "main".into(),
                },
            ),
            (
                OperationKind::Lock,
                OperationIntent::LockEnvironment {
                    environment: "dev".into(),
                },
            ),
            (
                OperationKind::Unlock,
                OperationIntent::UnlockEnvironment {
                    environment: "dev".into(),
                },
            ),
            (
                OperationKind::SetEnvironment,
                OperationIntent::SetEnvironment {
                    environment: "dev".into(),
                    changes: vec![],
                },
            ),
            (
                OperationKind::AddEnvironment,
                OperationIntent::AddEnvironment {
                    environment: "staging".into(),
                    base: "main".into(),
                },
            ),
            (
                OperationKind::RemoveEnvironment,
                OperationIntent::RemoveEnvironment {
                    environment: "dev".into(),
                },
            ),
            (
                OperationKind::Cleanup,
                OperationIntent::Cleanup { candidates: vec![] },
            ),
            (
                OperationKind::ApprovalApply,
                OperationIntent::ApplyApproval {
                    environment: "dev".into(),
                    request_id: "req-1".into(),
                    branches: vec!["a".into()],
                },
            ),
        ];

        assert_eq!(
            representatives.len(),
            OPERATION_KINDS.len(),
            "a new OperationKind needs a headline, and a representative intent to \
             check it with: {representatives:?} vs {OPERATION_KINDS:?}"
        );

        let mut seen: BTreeMap<String, OperationKind> = BTreeMap::new();
        for (kind, intent) in &representatives {
            let mut p = plan(intent.clone());
            p.kind = *kind;
            let headline = plan_headline(&p);
            assert!(
                !headline.is_empty(),
                "{kind:?} renders an empty headline, which is a document that opens \
                 with a blank line"
            );
            if let Some(previous) = seen.insert(headline.clone(), *kind) {
                assert_eq!(
                    previous, *kind,
                    "{kind:?} and {previous:?} both render `{headline}`, so a reader \
                     cannot tell which operation a plan is for — and the plan's own \
                     `kind` field is what a `--json` consumer keys on"
                );
            }
        }
        assert_eq!(
            seen.len(),
            representatives.len(),
            "every kind should have contributed one distinct headline"
        );
    }

    #[test]
    fn a_multi_branch_headline_lists_them_in_the_order_given() {
        let p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["zebra".into(), "apple".into()],
        });
        assert_eq!(plan_headline(&p), "Promote zebra, apple → dev");
    }

    // ---- projections -----------------------------------------------------

    #[test]
    fn current_and_proposed_render_as_a_base_plus_its_branches() {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.current = Some(projection(
            "dev",
            "main",
            &[("auth", &sha('1')), ("search", &sha('2'))],
        ));
        p.proposed = Some(projection(
            "dev",
            "main",
            &[
                ("auth", &sha('1')),
                ("search", &sha('2')),
                ("login", &sha('3')),
            ],
        ));
        let rendered = render_plan(&p);
        assert!(
            has_block(&rendered, "Current\n  dev = main + auth + search"),
            "{rendered}"
        );
        assert!(
            has_block(&rendered, "Proposed\n  dev = main + auth + search + login"),
            "{rendered}"
        );
    }

    #[test]
    fn a_projection_renders_in_declaration_order_and_is_never_sorted() {
        // The guard for `EnvironmentProjection::branches`'s "never sort": order
        // is composition order, so a renderer that alphabetised the list would
        // be describing a *different build* than the one it was handed.
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.proposed = Some(projection(
            "dev",
            "main",
            &[("zebra", &sha('1')), ("apple", &sha('2'))],
        ));
        let rendered = render_plan(&p);
        assert!(
            has_line(&rendered, "  dev = main + zebra + apple"),
            "{rendered}"
        );
        let zebra = rendered.find("zebra").expect("zebra rendered");
        let apple = rendered.find("apple").expect("apple rendered");
        assert!(zebra < apple, "zebra must precede apple: {rendered}");
    }

    #[test]
    fn an_environment_with_no_promoted_branches_renders_as_its_base_alone() {
        // The demote-to-empty shape, because the fixture's `current` and
        // `proposed` are equal and a plan with no transition renders neither
        // side — so setting only `proposed` here would test nothing.
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.current = Some(projection("dev", "main", &[("auth", &sha('1'))]));
        p.proposed = Some(projection("dev", "main", &[]));
        assert!(has_line(&render_plan(&p), "  dev = main"));
    }

    /// The arm that keeps a metadata operation from narrating itself twice.
    ///
    /// Written as a test rather than left to the shape of the renderer because
    /// the failure it guards against is silent in the worst way: a `hitch lock
    /// dev` plan that printed `Current / dev = main` and `Proposed / dev = main`
    /// would be *correct* on both lines and useless on both, and no review of the
    /// rendered output would obviously catch it.
    #[test]
    fn a_plan_with_no_transition_renders_neither_side_of_the_equation() {
        let mut p = plan(OperationIntent::LockEnvironment {
            environment: "dev".into(),
        });
        p.effects.push(PlannedEffect::MetadataChange {
            refname: "refs/heads/hitch-metadata".into(),
            description: "lock 'dev'".into(),
        });
        let rendered = render_plan(&p);
        assert!(
            !rendered.contains("Current") && !rendered.contains("Proposed"),
            "an unchanged composition has no transition to show: {rendered}"
        );
        // And the operation is still fully described — dropping the equation
        // must not cost the plan its only effect. Matched as a substring because
        // the effect table prefixes each row with its resource.
        assert!(
            rendered.contains("lock 'dev'"),
            "dropping the equation must not cost the plan its effect: {rendered}"
        );
    }

    /// The create arm: there is no `qa` before, so there is nothing to project.
    #[test]
    fn a_created_environment_proposes_an_equation_and_has_no_current_one() {
        let mut p = plan(OperationIntent::AddEnvironment {
            environment: "qa".into(),
            base: "main".into(),
        });
        p.current = None;
        p.proposed = Some(projection("qa", "main", &[]));
        let rendered = render_plan(&p);
        assert!(has_block(&rendered, "Proposed\n  qa = main"), "{rendered}");
        assert!(!rendered.contains("Current"), "{rendered}");
    }

    /// The destroy arm: `qa` exists now and will not afterwards.
    #[test]
    fn a_removed_environment_has_a_current_equation_and_no_proposed_one() {
        let mut p = plan(OperationIntent::RemoveEnvironment {
            environment: "qa".into(),
        });
        p.current = Some(projection("qa", "main", &[("auth", &sha('1'))]));
        p.proposed = None;
        let rendered = render_plan(&p);
        assert!(
            has_block(&rendered, "Current\n  qa = main + auth"),
            "{rendered}"
        );
        assert!(!rendered.contains("Proposed"), "{rendered}");
    }

    // ---- composition -----------------------------------------------------

    #[test]
    fn every_planned_branch_state_renders_distinctly_and_says_its_remedy() {
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.compositions = composition_of(vec![
            planned("auth", &sha('1'), PlannedBranchState::Included),
            planned("legacy", &sha('2'), PlannedBranchState::AlreadyInBase),
            planned(
                "payments",
                &sha('3'),
                PlannedBranchState::ReplayedResolution {
                    resolution_id: sha('d'),
                },
            ),
            planned(
                "dashboard",
                &sha('4'),
                PlannedBranchState::Held {
                    conflicts_with: "payments".into(),
                    files: vec!["src/pay.ts".into()],
                },
            ),
            planned("ghost", &sha('5'), PlannedBranchState::Missing),
        ]);
        let rendered = render_plan(&p);

        assert!(has_line(&rendered, "  ✓ auth at 1111111"), "{rendered}");
        assert!(
            has_line(&rendered, "  = legacy (already in base)"),
            "{rendered}"
        );
        assert!(
            has_line(
                &rendered,
                "  ♻️ payments at 3333333 (from recorded resolution ddddddd)"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("⛔ dashboard held — conflicts with payments"),
            "{rendered}"
        );
        assert!(rendered.contains("src/pay.ts"), "{rendered}");
        // The remedy is the one thing this renderer reconstructs rather than
        // copies; losing it is a real regression, so it is asserted directly.
        assert!(
            has_line(
                &rendered,
                "    fix: git checkout dashboard && git rebase payments"
            ),
            "{rendered}"
        );
        assert!(
            has_line(
                &rendered,
                "  ⚠️ ghost — accounted for in neither the build nor the held list"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn a_composition_of_nothing_still_says_so_rather_than_rendering_nothing() {
        // An environment with no promoted branches *does* get rebuilt. A
        // renderer that dropped the section would make a real build look like no
        // work at all.
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.compositions = composition_of(Vec::new());
        let rendered = render_plan(&p);
        assert!(
            rendered.contains("(base only — no promoted branches)"),
            "{rendered}"
        );
    }

    // ---- effects ---------------------------------------------------------

    #[test]
    fn every_planned_effect_renders_and_names_its_resource() {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.compositions = composition_of(vec![planned(
            "login",
            &sha('1'),
            PlannedBranchState::Included,
        )]);
        p.effects = vec![
            PlannedEffect::MetadataChange {
                refname: "refs/heads/hitch-metadata".into(),
                description: "add login into 'dev' (now: auth, login)".into(),
            },
            PlannedEffect::LocalRefUpdate {
                refname: "refs/heads/dev".into(),
                old: Some(sha('a')),
                new: sha('f'),
            },
            PlannedEffect::RemoteRefUpdate {
                refname: "refs/remotes/origin/dev".into(),
                old: Some(sha('a')),
                new: sha('f'),
            },
            PlannedEffect::TagCreation {
                name: "hitch-release-dev-to-main-2026-01-01T00-00-00Z".into(),
                target_sha: sha('f'),
            },
            PlannedEffect::DependentEnvironmentRebuild {
                environment: "qa".into(),
                because: "it is built on 'dev', which was rebuilt".into(),
                refname: "refs/heads/qa".into(),
            },
            PlannedEffect::PromotionPrune {
                environment: "qa".into(),
                branches: vec!["login".into(), "search".into()],
                refname: "refs/heads/hitch-metadata".into(),
            },
            PlannedEffect::LocalRefDelete {
                refname: "refs/heads/hitch/prev/dev".into(),
            },
        ];
        let rendered = render_plan(&p);

        // Every effect gets a line, and each line names its resource. The tag
        // name is long enough to set the table's column width, so the assertions
        // are on content rather than on padding.
        let lines: Vec<&str> = rendered
            .lines()
            .skip_while(|l| *l != "Will change")
            .skip(1)
            .collect();
        assert_eq!(lines.len(), 7, "{rendered}");
        for line in &lines {
            let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                [
                    // The model's own words, passed through.
                    "settings add login into 'dev' (now: auth, login)",
                    // A local ref update of the environment this plan composes is
                    // a rebuild, and the reader is told why the SHA moved.
                    "dev rebuild from 1 branch · aaaaaaa → fffffff",
                    "origin/dev publish aaaaaaa → fffffff",
                    "tag hitch-release-dev-to-main-2026-01-01T00-00-00Z create at fffffff",
                    "qa rebuild qa — it is built on 'dev', which was rebuilt",
                    "settings prune login, search from qa",
                    // A deletion says the verb. `LocalRefUpdate` cannot express
                    // one, so this is the only way the sweep can cover it — and
                    // a `→` with an empty right-hand side would be the failure
                    // this arm exists to prevent.
                    "hitch/prev/dev delete hitch/prev/dev",
                ]
                .contains(&collapsed.as_str()),
                "unexpected effect line {collapsed:?} in {rendered}"
            );
        }
    }

    #[test]
    fn a_local_ref_update_the_plan_did_not_compose_gets_the_bare_transition() {
        // Release's target is merged, not rebuilt. "rebuild" there would be
        // false, so the composition match has to be able to fail.
        let mut p = plan(OperationIntent::ReleaseEnvironment {
            environment: "dev".into(),
            target: "main".into(),
        });
        p.effects = vec![PlannedEffect::LocalRefUpdate {
            refname: "refs/heads/main".into(),
            old: Some(sha('a')),
            new: sha('f'),
        }];
        let rendered = render_plan(&p);
        assert!(rendered.contains("aaaaaaa → fffffff"), "{rendered}");
        assert!(!rendered.contains("none →"), "{rendered}");
        assert!(!rendered.contains("rebuild"), "{rendered}");
    }

    #[test]
    fn a_first_build_renders_the_absent_predecessor_as_a_word_not_an_empty_column() {
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.effects = vec![PlannedEffect::LocalRefUpdate {
            refname: "refs/heads/dev".into(),
            old: None,
            new: sha('f'),
        }];
        let rendered = render_plan(&p);
        assert!(rendered.contains("none → fffffff"), "{rendered}");
    }

    #[test]
    fn a_ref_renders_as_what_a_person_would_call_it() {
        // `refs/hitch/state/dev` is a live pointer the user has never seen.
        // Printing the path teaches nothing; naming its purpose does.
        assert_eq!(short_ref("refs/hitch/state/dev"), "build record for dev");
        assert_eq!(short_ref("refs/heads/dev"), "dev");
        assert_eq!(short_ref("refs/remotes/origin/dev"), "origin/dev");
        assert_eq!(short_ref("refs/tags/v1"), "tag v1");
        assert_eq!(short_ref("refs/heads/hitch-metadata"), "settings");
        assert_eq!(short_ref("refs/remotes/origin/hitch-metadata"), "settings");
    }

    // ---- unaffected and warnings -----------------------------------------

    #[test]
    fn unaffected_resources_are_listed_and_the_heading_vanishes_when_empty() {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        let without = render_plan(&p);
        assert!(!has_block(&without, "Will not change"), "{without}");

        p.unaffected = vec![
            UnaffectedResource {
                kind: ResourceKind::Environment,
                name: "qa".into(),
            },
            UnaffectedResource {
                kind: ResourceKind::Branch,
                name: "main".into(),
            },
        ];
        let with = render_plan(&p);
        assert!(has_block(&with, "Will not change\n  qa\n  main"), "{with}");
    }

    #[test]
    fn a_blocking_warning_never_renders_like_an_advisory_one() {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.warnings = vec![PlanWarning {
            message: "promote would conflict with an existing promoted branch".into(),
            kind: PlanWarningKind::PolicyRefusal,
            remedy: None,
            nothing_to_do: false,
        }];
        let refused = render_plan(&p);
        assert!(
            has_line(
                &refused,
                "  ⛔ promote would conflict with an existing promoted branch"
            ),
            "{refused}"
        );

        p.warnings = vec![PlanWarning {
            message: "'qa' will not be rebuilt — compatibility check failed".into(),
            kind: PlanWarningKind::Advisory,
            remedy: None,
            nothing_to_do: false,
        }];
        let advisory = render_plan(&p);
        assert!(
            has_line(
                &advisory,
                "  ⚠️ 'qa' will not be rebuilt — compatibility check failed"
            ),
            "{advisory}"
        );
        assert!(!advisory.contains("⛔"), "{advisory}");
    }

    #[test]
    fn a_warning_whose_remedy_is_on_a_second_line_stays_aligned_under_the_text() {
        // This is the shape the release and declaration planners emit: the
        // remedy is part of the message, one line down.
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.warnings = vec![PlanWarning::advisory(
            "'qa' will be left stale until it is rebuilt. To rebuild it:\n  hitch rebuild qa",
        )];
        let rendered = render_plan(&p);
        let remedy = rendered
            .lines()
            .find(|l| l.contains("hitch rebuild qa"))
            .expect("the remedy line survives rendering");
        assert!(
            remedy.starts_with("       hitch rebuild qa"),
            "remedy not aligned under the text: {remedy:?}"
        );
    }

    #[test]
    fn a_confirmation_requirement_does_not_change_how_a_plan_renders() {
        // Whether to ask is `confirm_plan`'s question, not the renderer's. If
        // the renderer reacted to it, then `--dry-run` and a real run would
        // print different plans for the same operation.
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        let ungated = render_plan(&p);
        p.confirmation = ConfirmationRequirement::required("the environment requires approval");
        assert_eq!(ungated, render_plan(&p));
    }

    #[test]
    fn an_approval_gate_is_a_needs_approval_section_not_a_refusal() {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "prod".into(),
            branches: vec!["login".into()],
        });
        p.warnings
            .push(crate::operations::model::PlanWarning::approval_required(
                "'prod' requires approval, so confirming files an approval request.",
            ));
        let rendered = render_plan(&p);
        assert!(rendered.contains("Needs approval\n  ⏳ "), "{rendered}");
        assert!(!rendered.contains("Why this cannot apply"), "{rendered}");
        assert!(!rendered.contains('⛔'), "{rendered}");
    }

    // ---- receipts --------------------------------------------------------

    fn receipt(outcome: OperationOutcome) -> ExecutionReceipt {
        ExecutionReceipt {
            plan_id: "test-plan".into(),
            operation: OperationKind::Promote,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            completed_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
            outcome,
            effects: Vec::new(),
            warnings: Vec::new(),
            resulting_state: None,
        }
    }

    #[test]
    fn an_applied_with_holds_receipt_never_reads_as_applied() {
        let applied = render_receipt(&receipt(OperationOutcome::Applied));
        let held = render_receipt(&receipt(OperationOutcome::AppliedWithHolds));
        assert_eq!(applied.lines().next(), Some("Applied"), "{applied}");
        assert_eq!(
            held.lines().next(),
            Some("Applied, with branches held"),
            "{held}"
        );
        // The specific regression: a held build that renders identically to a
        // clean one is a pipeline that looks green and is not shipping what it
        // declared.
        assert_ne!(applied, held);
    }

    #[test]
    fn each_outcome_has_its_own_headline() {
        for (outcome, headline) in [
            (OperationOutcome::ApprovalRequested, "Waiting for approval"),
            (OperationOutcome::NoChange, "Already up to date"),
        ] {
            assert_eq!(
                render_receipt(&receipt(outcome)).lines().next(),
                Some(headline)
            );
        }
    }

    #[test]
    fn an_owed_effect_is_its_own_section_and_never_a_tick() {
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::LocalRefUpdate {
            refname: "refs/heads/dev".into(),
            old: Some(sha('a')),
            new: sha('f'),
        }];
        r.warnings = vec![
            ExecutionWarning {
                message: "'dev' was published locally but pushing to origin failed. Push manually with: hitch push dev -f".into(),
                owes_effect: true,
            },
            ExecutionWarning {
                message: "'dashboard' is held until its conflict is fixed".into(),
                owes_effect: false,
            },
        ];
        let rendered = render_receipt(&r);

        // The applied work is a tick...
        assert!(has_line(&rendered, "  ✓ dev"), "{rendered}");
        // ...and the owed work is not, anywhere.
        let owed_line = rendered
            .lines()
            .find(|l| l.contains("hitch push dev -f"))
            .expect("the owed effect is named");
        assert!(!owed_line.contains('✓'), "{rendered}");
        assert!(rendered.contains("Still owed"), "{rendered}");
        // And the advisory stays where it was, as a warning.
        assert!(
            has_line(
                &rendered,
                "  ⚠️ 'dashboard' is held until its conflict is fixed"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn a_failed_dependent_rebuild_is_rendered_as_owed_not_as_done() {
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::DependentEnvironmentRebuild {
            environment: "qa".into(),
            outcome: DependentRebuildOutcome::Failed("branch 'auth' no longer exists".into()),
            held: Vec::new(),
            refname: "refs/heads/qa".into(),
        }];
        let rendered = render_receipt(&r);
        assert!(
            has_line(&rendered, "  ⧗ rebuild qa — branch 'auth' no longer exists"),
            "{rendered}"
        );
        assert!(!rendered.contains("✓ rebuild qa"), "{rendered}");
    }

    /// A hold is an anomaly, not an owed effect and not a clean rebuild, so it
    /// gets its own glyph and it names the neighbour — the partner is what
    /// distinguishes "rebase this onto the base" from "reorder the list".
    #[test]
    fn a_rebuild_that_held_branches_is_not_rendered_as_a_clean_rebuild() {
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::DependentEnvironmentRebuild {
            environment: "dev".into(),
            outcome: DependentRebuildOutcome::Rebuilt,
            held: vec![HoldPair {
                branch: "feature/dashboard".into(),
                conflicts_with: "feature/payments".into(),
            }],
            refname: "refs/heads/dev".into(),
        }];
        let rendered = render_receipt(&r);
        assert!(
            has_line(
                &rendered,
                "  ⚠️ rebuild dev — 1 branch held: feature/dashboard (conflicts with feature/payments)"
            ),
            "{rendered}"
        );
        // Not owed: nothing is outstanding, the branch was deliberately held.
        assert!(!rendered.contains("⧗ rebuild dev"), "{rendered}");
        assert!(!rendered.contains("✓ rebuild dev"), "{rendered}");
    }

    #[test]
    fn several_held_branches_are_counted_in_the_plural() {
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::DependentEnvironmentRebuild {
            environment: "dev".into(),
            outcome: DependentRebuildOutcome::Rebuilt,
            held: vec![
                HoldPair {
                    branch: "a".into(),
                    conflicts_with: "b".into(),
                },
                HoldPair {
                    branch: "c".into(),
                    conflicts_with: "d".into(),
                },
            ],
            refname: "refs/heads/dev".into(),
        }];
        let rendered = render_receipt(&r);
        assert!(
            has_line(
                &rendered,
                "  ⚠️ rebuild dev — 2 branches held: a (conflicts with b), c (conflicts with d)"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn a_tag_effect_renders_the_name_that_exists_not_the_one_that_was_intended() {
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::TagCreation {
            name: "hitch-release-dev-to-main-2026-01-01T00-00-00Z-2".into(),
            target_sha: sha('f'),
        }];
        assert!(render_receipt(&r).contains("tag hitch-release-dev-to-main-2026-01-01T00-00-00Z-2"));
    }

    #[test]
    fn a_deleted_ref_reports_the_value_that_was_there() {
        // The value, not just the name: "deleted prev/dev" cannot be
        // distinguished from a delete of a *different* commit of the same
        // branch, and the whole reason the receipt exists is the difference
        // between what was predicted and what is there.
        let mut r = receipt(OperationOutcome::Applied);
        r.effects = vec![AppliedEffect::LocalRefDelete {
            refname: "refs/hitch/backup/dev".into(),
            old: sha('a'),
        }];
        let rendered = render_receipt(&r);
        assert!(has_line(&rendered, "  ✓ delete backup/dev"), "{rendered}");
        assert!(has_line(&rendered, "    was aaaaaaa"), "{rendered}");
    }

    // ---- resulting state -------------------------------------------------

    fn environment(name: &str, health: EnvironmentHealth) -> EnvironmentState {
        EnvironmentState {
            name: name.to_string(),
            base: "main".into(),
            desired: DesiredComposition {
                base: "main".into(),
                base_sha: Some(sha('9')),
                branches: vec![DeclaredBranch {
                    name: "auth".into(),
                    sha: Some(sha('1')),
                    contained_in_base: false,
                }],
            },
            actual: ActualComposition::LegacyUnknown,
            health,
            locked: false,
            approval_policy: ApprovalPolicy {
                required: false,
                min_approvals: 0,
                approvers: Vec::new(),
            },
            locked_by: None,
            locked_at: None,
            rebuilt_at: None,
            released_at: None,
        }
    }

    fn snapshot(health: EnvironmentHealth) -> RepositoryStateSnapshot {
        RepositoryStateSnapshot {
            metadata_sha: Some(sha('9')),
            current_branch: Some("main".into()),
            environments: vec![environment("dev", health)],
            features: Vec::new(),
            captured_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn a_receipt_renders_the_health_the_snapshot_reports_not_a_recomputation() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::Realised));
        assert!(has_line(&render_receipt(&r), "  ✓ dev   realised"));

        r.resulting_state = Some(snapshot(EnvironmentHealth::NeedsRebuild {
            changed_inputs: vec![ChangedInput {
                branch: "auth".into(),
                previous_sha: Some(sha('a')),
                current_sha: Some(sha('b')),
            }],
            added: vec!["login".into()],
            removed: Vec::new(),
        }));
        let rendered = render_receipt(&r);
        assert!(has_line(&rendered, "  ⧗ dev   needs rebuild"), "{rendered}");
        assert!(
            has_line(&rendered, "      auth moved   aaaaaaa → bbbbbbb"),
            "{rendered}"
        );
        assert!(
            has_line(&rendered, "      login added to the declaration"),
            "{rendered}"
        );
    }

    /// A snapshot with nothing in it produces no `Result` section at all.
    ///
    /// `hitch remove dev` is what reaches this, and it is the *only* thing that
    /// does: every other mutation names an environment, so the snapshot has one.
    /// `remove` is defined by making it stop being the case, which means the
    /// `Result` section renders its heading and then has no line to put under
    /// it. A reader who has just been told their environment was removed sees a
    /// heading that looks like the beginning of an answer, and gets none.
    ///
    /// The receipt is still complete: the effect line above carries the whole
    /// fact (`remove environment 'dev' from the declaration`), so nothing is
    /// lost by dropping a section that cannot say anything.
    #[test]
    fn an_empty_snapshot_leaves_no_result_section_to_be_empty() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(RepositoryStateSnapshot {
            environments: Vec::new(),
            ..snapshot(EnvironmentHealth::Realised)
        });
        let rendered = render_receipt(&r);
        assert!(
            !rendered.contains("Result"),
            "a heading with no lines under it reads as a broken render, not as an \
             empty answer:\n{rendered}"
        );
        // And the receipt is still an answer rather than a stub.
        assert_eq!(rendered.lines().next(), Some("Applied"), "{rendered}");
    }

    /// With one environment the section is there, and the heading is not alone.
    ///
    /// The counterpart to the test above, and the reason it is not vacuous: the
    /// section does not vanish for a reason the *receipt* needs — the guard is
    /// on the snapshot's contents, so a receipt carrying a real snapshot still
    /// opens it.
    #[test]
    fn a_snapshot_with_an_environment_still_renders_the_result_section() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::Realised));
        let rendered = render_receipt(&r);
        assert!(rendered.contains("\nResult\n"), "{rendered}");
        assert!(has_line(&rendered, "  ✓ dev   realised"), "{rendered}");
    }

    #[test]
    fn a_changed_input_whose_branch_is_gone_says_so_rather_than_rendering_a_blank() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::NeedsRebuild {
            changed_inputs: vec![ChangedInput {
                branch: "auth".into(),
                previous_sha: Some(sha('a')),
                current_sha: None,
            }],
            added: Vec::new(),
            removed: Vec::new(),
        }));
        let rendered = render_receipt(&r);
        assert!(
            has_line(&rendered, "      auth moved   aaaaaaa → gone"),
            "{rendered}"
        );
    }

    /// The three detail lines used to be an `old → new` arrow, `added to the
    /// declaration`, and `removed from the declaration` — so only two of three
    /// said what had happened, and the arrow read as a ref update rather than as
    /// "this environment's input moved". One grammar, three clauses.
    #[test]
    fn every_detail_line_states_what_happened_rather_than_only_showing_a_pair_of_shas() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::NeedsRebuild {
            changed_inputs: vec![ChangedInput {
                branch: "main".into(),
                previous_sha: Some(sha('a')),
                current_sha: Some(sha('b')),
            }],
            added: vec!["login".into()],
            removed: vec!["auth".into()],
        }));
        let rendered = render_receipt(&r);
        for line in [
            "      main moved   aaaaaaa → bbbbbbb",
            "      login added to the declaration",
            "      auth removed from the declaration",
        ] {
            assert!(
                has_line(&rendered, line),
                "missing {line:?} in:\n{rendered}"
            );
        }
    }

    /// Not a cosmetic dedupe. `health_from_record` walks the *recorded* pins to
    /// build `changed_inputs`, and a branch that has left the declaration
    /// resolves to no current SHA — so it fails that comparison too, and
    /// `removed ⊆ changed_inputs` holds by construction. Printing both said the
    /// same branch twice, and the specific line is the actionable one.
    #[test]
    fn a_removed_branch_is_reported_once_not_twice() {
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::NeedsRebuild {
            // Exactly what `health_from_record` produces for a demoted branch.
            changed_inputs: vec![ChangedInput {
                branch: "auth".into(),
                previous_sha: Some(sha('a')),
                current_sha: None,
            }],
            added: Vec::new(),
            removed: vec!["auth".into()],
        }));
        let rendered = render_receipt(&r);
        assert!(
            has_line(&rendered, "      auth removed from the declaration"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("auth moved"),
            "the removal was also rendered as a moved input:\n{rendered}"
        );
        assert_eq!(
            rendered.matches("auth").count(),
            1,
            "auth appears more than once:\n{rendered}"
        );
    }

    #[test]
    fn an_unknown_build_renders_as_unknown_and_never_with_a_tick() {
        // `LegacyUnknown` is a normal state today (a repo last built by a
        // pre-P2 hitch). Two things have to hold: it is named rather than
        // skipped, and it does not get a ✓ — a tick beside "unknown" is a claim
        // the model explicitly refuses to make.
        let mut r = receipt(OperationOutcome::Applied);
        r.resulting_state = Some(snapshot(EnvironmentHealth::LegacyUnknown));
        let rendered = render_receipt(&r);
        assert!(
            has_line(&rendered, "  ◌ dev   actual unknown"),
            "{rendered}"
        );
        assert!(!rendered.contains("✓ dev"), "{rendered}");
    }

    // ---- the confirmation gate -------------------------------------------

    #[test]
    fn the_gate_refuses_to_prompt_under_json_and_names_the_flag() {
        // Not "ask anyway": a JSON consumer is a program, and a program that
        // blocks on a terminal read is the failure the flag removes.
        assert!(matches!(
            decide_gate(false, true, true),
            GateDecision::Refuse(reason) if reason.contains("--yes")
        ));
    }

    #[test]
    fn yes_proceeds_however_json_and_confirmation_are_set() {
        // `--yes` is the only thing that makes this a no-ask. It must not be
        // conditional on anything else, or a `--json` CI run would fail on a
        // gate it had already answered.
        for json in [false, true] {
            for required in [false, true] {
                assert_eq!(decide_gate(true, json, required), GateDecision::Proceed);
            }
        }
    }

    #[test]
    fn a_gated_plan_is_asked_about_and_an_ungated_one_is_only_shown() {
        assert_eq!(decide_gate(false, false, true), GateDecision::Ask);
        assert_eq!(decide_gate(false, false, false), GateDecision::Proceed);
    }

    /// The question names the plan's own reason, because the reason is the only
    /// statement of what answering would actually *do* — and in the one case
    /// that matters most, the plan's "Will change" section is empty, because
    /// confirming files an approval request rather than editing anything.
    #[test]
    fn the_question_states_the_reason_before_asking() {
        let question = confirmation_question(&ConfirmationRequirement::required(
            "'dev' requires approval, so confirming files an approval request.\n  \
             To review pending requests: hitch approvals list",
        ));
        assert!(question.ends_with("\nApply this plan?"), "{question:?}");
        assert!(
            question.contains("files an approval request"),
            "the reason must be the question's first line, not a detail: {question:?}"
        );
    }

    /// An absent, empty, or blank reason is no reason, and must not become a
    /// blank line above the prompt — which reads as a truncated plan rather
    /// than as a plan with nothing to say.
    #[test]
    fn a_plan_with_nothing_to_say_asks_the_bare_question() {
        for requirement in [
            ConfirmationRequirement::not_required(),
            ConfirmationRequirement::required(""),
            ConfirmationRequirement::required("   \n  "),
        ] {
            assert_eq!(
                confirmation_question(&requirement),
                "Apply this plan?",
                "{requirement:?}"
            );
        }
    }

    // ---- the JSON document -----------------------------------------------

    fn document() -> serde_json::Value {
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.compositions = composition_of(vec![planned(
            "dashboard",
            &sha('1'),
            PlannedBranchState::Held {
                conflicts_with: "payments".into(),
                files: vec!["src/pay.ts".into()],
            },
        )]);
        serde_json::to_value(JsonDocument::preview(&p)).expect("preview serialises")
    }

    #[test]
    fn the_documents_top_level_shape_is_exactly_three_keys() {
        // The wire contract, pinned. `schema_version` is the only sanctioned way
        // to change any of this, so a test that enumerates the keys is what
        // makes the version mean something.
        let value = document();
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("the document is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["plan", "receipt", "schema_version"]);
        assert_eq!(value["schema_version"], serde_json::json!(1));
    }

    #[test]
    fn a_preview_names_the_plan_and_reports_no_receipt() {
        // `null` rather than an omitted key: a consumer reading one field finds
        // either an object or an explicit "there was none", where a missing key
        // would be indistinguishable from a hitch that never emitted one.
        let value = document();
        assert!(value["plan"].is_object());
        assert!(value["receipt"].is_null());
    }

    #[test]
    fn an_applied_document_carries_both_halves() {
        let p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        let r = receipt(OperationOutcome::Applied);
        let value = serde_json::to_value(JsonDocument::applied(&p, &r)).expect("serialises");
        assert!(value["plan"].is_object());
        assert!(value["receipt"].is_object());
        assert_eq!(value["receipt"]["outcome"], serde_json::json!("Applied"));
    }

    #[test]
    fn enums_are_externally_tagged_so_a_consumer_switches_on_a_plain_string() {
        // The *representation* is the wire contract and the derive does not
        // document itself, so it is asserted. Serde's default external tagging
        // gives `"Included"` for a unit variant and `{"Held": {...}}` for one
        // with fields; internally tagged (`#[serde(tag = ...)]`) or adjacently
        // tagged would both break a consumer written against this.
        let value = document();
        let branches = value["plan"]["compositions"][0]["branches"]
            .as_array()
            .expect("branches serialise as an array");
        assert_eq!(branches[0]["state"]["Held"]["conflicts_with"], "payments");
        assert!(branches[0]["state"]["Held"]["files"][0].is_string());

        let simple = serde_json::to_value(PlannedBranchState::Included).expect("serialises");
        assert_eq!(simple, serde_json::json!("Included"));

        let effect = serde_json::to_value(PlannedEffect::TagCreation {
            name: "v1".into(),
            target_sha: sha('f'),
        })
        .expect("serialises");
        assert_eq!(
            effect,
            serde_json::json!({"TagCreation": {"name": "v1", "target_sha": sha('f')}})
        );
    }

    #[test]
    fn the_fingerprint_reaches_the_document_with_its_full_refnames() {
        // Short names are a *display* convention. A consumer correlating a
        // document against the repository needs the ref it can actually look up.
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.fingerprint.metadata_sha = Some(sha('9'));
        p.fingerprint.refs.insert("refs/heads/dev".into(), sha('f'));
        let value = serde_json::to_value(JsonDocument::preview(&p)).expect("serialises");
        assert_eq!(
            value["plan"]["fingerprint"]["refs"]["refs/heads/dev"],
            serde_json::json!(sha('f'))
        );
    }

    #[test]
    fn the_document_never_contains_an_escape_sequence() {
        // The whole point of routing diagnostics to stderr: colour in a JSON
        // string is a parse hazard in some consumers. Asserted on the rendered
        // document, not on the raw renderer, because that is what ships.
        let rendered = serde_json::to_string_pretty(&document()).expect("serialises");
        assert!(!rendered.contains('\u{1b}'), "ESC byte in the document");
    }

    // ---- the negative test ----------------------------------------------

    #[test]
    fn the_renderer_never_says_what_the_model_does_not_say() {
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.compositions = composition_of(vec![planned(
            "auth",
            &sha('1'),
            PlannedBranchState::Included,
        )]);
        let rendered = render_plan(&p);

        // No warnings, no holds, nothing owed — so no warning glyphs. A renderer
        // that invented one would send a user looking for a problem that does
        // not exist.
        for glyph in ["⛔", "⚠️", "⧗"] {
            assert!(!rendered.contains(glyph), "invented {glyph}: {rendered}");
        }
        // Every short SHA shown is a prefix of a SHA the plan carries. A
        // rendered SHA in neither the plan nor a composed commit is a
        // hallucination, and this is the only mechanical check for that.
        let known: Vec<String> = [sha('9'), sha('1'), sha('f'), sha('a')]
            .iter()
            .map(|s| s.chars().take(7).collect())
            .collect();
        for token in rendered.split_whitespace() {
            let looks_like_a_short_sha =
                token.len() == 7 && token.chars().all(|c| c.is_ascii_hexdigit());
            if looks_like_a_short_sha {
                assert!(
                    known.contains(&token.to_string()),
                    "rendered a SHA the plan does not carry: {token} in {rendered}"
                );
            }
        }
    }

    #[test]
    fn rendering_never_ends_in_a_blank_line() {
        // The emitters add the newline; a renderer that also added one would put
        // a blank line between every block of output and between a plan and its
        // prompt.
        let mut p = plan(OperationIntent::PromoteBranches {
            environment: "dev".into(),
            branches: vec!["login".into()],
        });
        p.effects = vec![PlannedEffect::LocalRefUpdate {
            refname: "refs/heads/dev".into(),
            old: None,
            new: sha('f'),
        }];
        let rendered = render_plan(&p);
        assert_eq!(rendered, rendered.trim_end(), "{rendered:?}");
    }

    // ── §13: the environment equation ─────────────────────────────────────

    fn equation(
        environment: &str,
        base: &str,
        branches: &[&str],
        excluded: Vec<ExcludedTerm>,
    ) -> EnvironmentEquation {
        EnvironmentEquation {
            environment: environment.to_string(),
            base: base.to_string(),
            terms: branches
                .iter()
                .map(|b| EquationTerm {
                    branch: b.to_string(),
                    state: EquationTermState::Plain,
                })
                .collect(),
            excluded,
        }
    }

    fn held(branch: &str, conflicts_with: &str, files: &[&str]) -> ExcludedTerm {
        ExcludedTerm {
            branch: branch.to_string(),
            reason: ExclusionReason::Held {
                conflicts_with: conflicts_with.to_string(),
                files: files.iter().map(|f| f.to_string()).collect(),
            },
        }
    }

    #[test]
    fn an_equation_with_no_branches_is_the_base_alone_and_says_only_that() {
        // `dev = main` is the whole composition, not a truncated one, so it
        // carries no apology. The "base only" wording is `render_plan`'s, in its
        // Composition section — putting it here too would print the same remark
        // twice in a plan that shows both.
        assert_eq!(
            render_equation(&equation("dev", "main", &[], vec![])),
            "dev = main"
        );
    }

    #[test]
    fn an_equation_lists_its_branches_in_the_order_it_was_handed_them() {
        assert_eq!(
            render_equation(&equation(
                "dev",
                "main",
                &["auth", "payments", "search"],
                vec![]
            )),
            "dev = main + auth + payments + search"
        );
        // Declaration order is composition order, and a renderer that sorted it
        // would be describing a different build. Reversed input, reversed
        // output — asserted so a future `sort` fails here.
        assert_eq!(
            render_equation(&equation("dev", "main", &["search", "auth"], vec![])),
            "dev = main + search + auth"
        );
    }

    #[test]
    fn a_term_already_in_the_base_says_so_on_the_same_line() {
        let mut eq = equation("dev", "main", &["auth"], vec![]);
        eq.terms[0].state = EquationTermState::InBase;
        assert_eq!(render_equation(&eq), "dev = main + auth (already in base)");
    }

    #[test]
    fn an_excluded_branch_is_listed_beneath_the_equation_not_summed_into_it() {
        let rendered = render_equation(&equation(
            "dev",
            "main",
            &["auth", "payments"],
            vec![held("dashboard", "payments", &["src/ui.rs", "src/api.rs"])],
        ));
        assert_eq!(
            rendered,
            "dev = main + auth + payments\n    dashboard ⛔ held — conflicts with payments\n        src/ui.rs\n        src/api.rs"
        );
    }

    #[test]
    fn an_unaccounted_branch_reads_as_absent_from_the_build_not_as_held() {
        // "Held" is a claim about a conflict, and an excluded branch with no
        // conflict behind it has not been held by anything.
        let rendered = render_equation(&equation(
            "dev",
            "main",
            &["auth"],
            vec![ExcludedTerm {
                branch: "mystery".to_string(),
                reason: ExclusionReason::Unknown,
            }],
        ));
        assert!(rendered.starts_with("dev = main + auth\n"), "{rendered:?}");
        assert!(
            has_line(&rendered, "    mystery ◌ not accounted for"),
            "{rendered:?}"
        );
    }

    /// §13's regression test, and deliberately a *string* comparison rather
    /// than a "both mention the branch" assertion: the requirement is that
    /// these are the same characters, not that they agree about which branches
    /// exist. A substring test would pass against two different vocabularies.
    #[test]
    fn the_plan_and_the_status_equation_are_the_same_characters() {
        let branches = [("auth", &sha('1')[..]), ("payments", &sha('2')[..])];
        let from_plan = render_equation(&EnvironmentEquation::from_projection(&projection(
            "dev", "main", &branches,
        )));
        let from_status = render_equation(&equation("dev", "main", &["auth", "payments"], vec![]));
        assert_eq!(from_plan, from_status);
        assert_eq!(from_plan, "dev = main + auth + payments");
    }

    #[test]
    fn a_plans_equation_never_repeats_its_held_branches() {
        // A plan lists every branch, with a remedy, in its Composition section.
        // Re-listing the held ones inside the equation would print the same fact
        // twice in two vocabularies, which is what §13 argues against.
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.proposed = Some(projection("dev", "main", &[("auth", &sha('1')[..])]));
        p.compositions = vec![CompositionPlan {
            environment: "dev".into(),
            base: pinned("main", &sha('9')),
            branches: vec![planned("auth", &sha('1'), PlannedBranchState::Included)],
            result_sha: sha('f'),
            holds: Vec::new(),
        }];
        let rendered = render_plan(&p);
        let proposed = rendered
            .lines()
            .find(|l| l.contains("dev = main + auth"))
            .expect("a Proposed line");
        assert_eq!(proposed.trim(), "dev = main + auth");
    }

    // ── §12: the status matrix ────────────────────────────────────────────

    fn cell_model(rows: &[(&str, &[MatrixCell])], columns: &[&str]) -> MatrixModel {
        MatrixModel {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            rows: rows
                .iter()
                .map(|(feature, cells)| MatrixRow {
                    feature: feature.to_string(),
                    cells: cells.to_vec(),
                })
                .collect(),
            summaries: Vec::new(),
        }
    }

    fn summary(environment: &str, health: EnvironmentHealth) -> MatrixSummaryRow {
        MatrixSummaryRow {
            environment: environment.to_string(),
            base: "main".to_string(),
            desired: 3,
            realised: 2,
            held: 1,
            needs_rebuild: 0,
            missing: 0,
            actual_unknown: 0,
            locked: false,
            health,
        }
    }

    #[test]
    fn every_cell_state_renders_as_a_glyph_and_a_word() {
        // §12 requires the grid to be understandable without colour, which
        // means the state cannot live in a glyph alone. All seven here, so a new
        // variant added to `MatrixCell` without a label fails this test rather
        // than rendering as an empty cell.
        let cells = [
            MatrixCell::NotDesired,
            MatrixCell::Included,
            MatrixCell::Held,
            MatrixCell::InBase,
            MatrixCell::NeedsRebuild,
            MatrixCell::ActualUnknown,
            MatrixCell::Missing,
        ];
        for cell in cells {
            let model = cell_model(&[("a", &[cell])], &["dev"]);
            let rendered = render_matrix(&model);
            let line = rendered
                .lines()
                .nth(2)
                .unwrap_or_else(|| panic!("no row for {cell:?} in {rendered:?}"));
            assert!(
                line.contains(cell.label()),
                "{cell:?} rendered without its word: {rendered:?}"
            );
            assert!(
                line.contains(cell.glyph()),
                "{cell:?} rendered without its glyph: {rendered:?}"
            );
        }
    }

    #[test]
    fn the_matrix_is_rectangular_when_one_name_is_far_longer_than_another() {
        // A naive `{:width$}` renderer puts the short names in the wide column
        // and the long name's cell ends up a column to the left, which still
        // *looks* plausible. Checking the shape of every line catches it.
        let model = cell_model(
            &[
                ("a", &[MatrixCell::Included, MatrixCell::NotDesired]),
                (
                    "feature/with/a/very/long/name",
                    &[MatrixCell::Held, MatrixCell::NeedsRebuild],
                ),
            ],
            &["dev", "qa"],
        );
        let rendered = render_matrix(&model);
        // The two header lines are not rows, so they are skipped by index
        // rather than by pattern — a name-based filter is exactly the kind of
        // predicate that quietly stops matching when the format changes.
        let glyphs = ['●', '—', '⛔', '↻', '◌', '=', '!'];
        let rows: Vec<Vec<usize>> = rendered
            .lines()
            .skip(2)
            .map(|l| {
                // Sorted by *position*, not by glyph: sorting the pair orders it
                // by codepoint, which happens to look like a misalignment and
                // is none.
                let mut found: Vec<usize> = l
                    .char_indices()
                    .filter(|(_, c)| glyphs.contains(c))
                    .map(|(i, _)| i)
                    .collect();
                found.sort_unstable();
                found
            })
            .collect();
        assert_eq!(rows.len(), 2, "{rendered:?}");
        assert_eq!(rows[0].len(), 2, "row has a cell per column: {rendered:?}");
        assert_eq!(rows[0], rows[1], "cell columns misaligned: {rendered:?}");
    }

    #[test]
    fn a_matrix_with_no_rows_still_renders_its_header() {
        // "No features are promoted" is an answer; an empty screen is not.
        let rendered = render_matrix(&cell_model(&[], &["dev", "qa"]));
        assert!(has_line(&rendered, "Feature  DEV  QA"), "{rendered:?}");
    }

    #[test]
    fn no_matrix_line_has_trailing_whitespace() {
        // Trailing spaces are invisible in a terminal and noisy in a diff, and
        // `pad_left` on a cell that already fills its column is exactly how
        // they get introduced.
        let model = cell_model(
            &[
                ("a", &[MatrixCell::Included]),
                ("feature/with/a/very/long/name", &[MatrixCell::NeedsRebuild]),
            ],
            &["dev"],
        );
        for line in render_matrix(&model).lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace: {line:?}");
        }
    }

    #[test]
    fn environment_summaries_reuse_the_health_label() {
        // One vocabulary of health words, taken from `EnvironmentHealth::label`
        // — the same function the rest of the CLI reads — rather than a second
        // set of words written next to it.
        let rendered = render_environment_summaries(&[
            summary("dev", EnvironmentHealth::PartiallyRealised { held: vec![] }),
            summary("qa", EnvironmentHealth::Realised),
        ]);
        assert!(
            has_line(&rendered, "    partially realised"),
            "{rendered:?}"
        );
        assert!(has_line(&rendered, "    realised"), "{rendered:?}");
        assert!(
            has_line(&rendered, "DEV  desired 3 · actual 2 · 1 held"),
            "{rendered:?}"
        );
    }

    #[test]
    fn a_summary_only_mentions_a_count_that_is_non_zero() {
        // A summary listing "· 0 held · 0 needs rebuild" on every healthy
        // environment is noise that trains a reader to skip the line.
        let mut clean = summary("qa", EnvironmentHealth::Realised);
        clean.held = 0;
        let rendered = render_environment_summaries(&[clean]);
        assert!(!rendered.contains("held"), "{rendered:?}");
        // Two spaces, because `dev`/`qa` are both three characters and the
        // name column is sized to the widest of them — not a fixed four, which
        // is what a longer environment name like `stage` exposes.
        assert!(
            rendered.contains("QA   desired 3 · actual 2"),
            "{rendered:?}"
        );
    }

    mod activity_words {
        use super::*;
        use crate::core::activity::{ActivityEntry, SkippedCommit};
        use chrono::FixedOffset;

        fn s(x: &str) -> String {
            x.to_string()
        }

        fn rebuilt(outcome: RebuildOutcome) -> HitchEvent {
            HitchEvent::Rebuilt {
                environment: s("dev"),
                outcome,
            }
        }

        #[test]
        fn every_event_has_its_sentence() {
            let mut cases: Vec<(HitchEvent, &str)> = vec![
                (
                    HitchEvent::EnvironmentCreated {
                        environment: s("dev"),
                        base: s("main"),
                    },
                    "created environment dev from main",
                ),
                (
                    HitchEvent::EnvironmentRemoved {
                        environment: s("dev"),
                    },
                    "removed environment dev",
                ),
                (
                    HitchEvent::BaseChanged {
                        environment: s("dev"),
                        from: s("main"),
                        to: s("develop"),
                    },
                    "changed dev's base from main to develop",
                ),
                (
                    HitchEvent::Promoted {
                        environment: s("dev"),
                        branch: s("feature/a"),
                    },
                    "added feature/a to dev",
                ),
                (
                    HitchEvent::Demoted {
                        environment: s("dev"),
                        branch: s("feature/a"),
                    },
                    "removed feature/a from dev",
                ),
                (
                    HitchEvent::Locked {
                        environment: s("dev"),
                        by: Some(s("bob")),
                    },
                    "locked dev",
                ),
                (
                    HitchEvent::Unlocked {
                        environment: s("dev"),
                    },
                    "unlocked dev",
                ),
                (rebuilt(RebuildOutcome::Unrecorded), "rebuilt dev"),
                (
                    rebuilt(RebuildOutcome::Clean {
                        included: vec![s("a")],
                    }),
                    "rebuilt dev",
                ),
                (
                    rebuilt(RebuildOutcome::WithHolds {
                        included: vec![],
                        held: vec![HoldPair {
                            branch: s("dashboard"),
                            conflicts_with: s("payments"),
                        }],
                    }),
                    "rebuilt dev, holding dashboard",
                ),
                (
                    HitchEvent::Released {
                        environment: s("dev"),
                    },
                    "released dev",
                ),
            ];
            let mk = |kind: &str, d: ApprovalDirection| {
                let (request_id, environment, branch) = (s("r1"), s("prod"), s("feature/a"));
                match kind {
                    "requested" => HitchEvent::ApprovalRequested {
                        request_id,
                        environment,
                        branch,
                        direction: d,
                    },
                    "voted" => HitchEvent::ApprovalVoted {
                        request_id,
                        environment,
                        branch,
                        approvals: 1,
                        required: 2,
                        direction: d,
                    },
                    "granted" => HitchEvent::ApprovalGranted {
                        request_id,
                        environment,
                        branch,
                        direction: d,
                    },
                    "rejected" => HitchEvent::ApprovalRejected {
                        request_id,
                        environment,
                        branch,
                        direction: d,
                    },
                    "applied" => HitchEvent::ApprovalApplied {
                        request_id,
                        environment,
                        branch,
                        direction: d,
                    },
                    _ => HitchEvent::ApprovalCancelled {
                        request_id,
                        environment,
                        branch,
                        direction: d,
                    },
                }
            };
            use ApprovalDirection::{Demote, Promote};
            let approval_rows = [
                ("requested", Promote, "asked to add feature/a to prod"),
                ("requested", Demote, "asked to remove feature/a from prod"),
                (
                    "voted",
                    Promote,
                    "approved adding feature/a to prod (1 of 2)",
                ),
                (
                    "voted",
                    Demote,
                    "approved removing feature/a from prod (1 of 2)",
                ),
                ("granted", Promote, "adding feature/a to prod is approved"),
                (
                    "granted",
                    Demote,
                    "removing feature/a from prod is approved",
                ),
                ("rejected", Promote, "rejected adding feature/a to prod"),
                ("rejected", Demote, "rejected removing feature/a from prod"),
                (
                    "applied",
                    Promote,
                    "applied the approved change: feature/a to prod",
                ),
                (
                    "applied",
                    Demote,
                    "applied the approved change: feature/a out of prod",
                ),
                (
                    "cancelled",
                    Promote,
                    "cancelled the request to add feature/a to prod",
                ),
                (
                    "cancelled",
                    Demote,
                    "cancelled the request to remove feature/a from prod",
                ),
            ];
            for (kind, d, expected) in approval_rows {
                cases.push((mk(kind, d), expected));
            }
            for (event, expected) in cases {
                assert_eq!(render_event(&event), expected);
            }
        }

        fn now() -> chrono::DateTime<FixedOffset> {
            FixedOffset::east_opt(3600)
                .unwrap()
                .with_ymd_and_hms(2026, 9, 29, 10, 0, 0)
                .unwrap()
        }

        fn entry(
            y: i32,
            mo: u32,
            d: u32,
            h: u32,
            mi: u32,
            events: Vec<HitchEvent>,
        ) -> ActivityEntry {
            ActivityEntry {
                commit: s("abcdef1234567"),
                when: Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap(),
                actor: s("martin"),
                events,
            }
        }

        fn promoted() -> HitchEvent {
            HitchEvent::Promoted {
                environment: s("dev"),
                branch: s("feature/a"),
            }
        }

        fn log(entries: Vec<ActivityEntry>) -> ActivityLog {
            ActivityLog {
                entries,
                skipped: vec![],
                truncated: false,
                branch_filtered: false,
            }
        }

        #[test]
        fn pointer_is_suppressed_when_a_commit_was_skipped() {
            let mut l = log(vec![entry(2026, 9, 29, 8, 0, vec![promoted()])]);
            l.skipped.push(crate::core::activity::SkippedCommit {
                commit: s("deadbeef"),
                reason: s("unreadable"),
            });
            assert!(!render_activity(&l, now(), false).contains("has not been rebuilt"));
        }

        #[test]
        fn pointer_is_suppressed_when_the_log_was_filtered_by_branch() {
            let mut l = log(vec![entry(2026, 9, 29, 8, 0, vec![promoted()])]);
            assert!(render_activity(&l, now(), false).contains("has not been rebuilt"));
            l.branch_filtered = true;
            assert!(!render_activity(&l, now(), false).contains("has not been rebuilt"));
        }

        #[test]
        fn a_vote_and_its_grant_in_one_entry_read_as_one_line() {
            let voted = HitchEvent::ApprovalVoted {
                request_id: s("r1"),
                environment: s("prod"),
                branch: s("search"),
                direction: ApprovalDirection::Promote,
                approvals: 1,
                required: 1,
            };
            let granted = HitchEvent::ApprovalGranted {
                request_id: s("r1"),
                environment: s("prod"),
                branch: s("search"),
                direction: ApprovalDirection::Promote,
            };
            let both = render_activity(
                &log(vec![entry(
                    2026,
                    9,
                    29,
                    8,
                    0,
                    vec![voted.clone(), granted.clone()],
                )]),
                now(),
                false,
            );
            assert!(both.contains(
                "approved adding search to prod (1 of 1), which completes the approval\n"
            ));
            assert!(!both.contains("is approved"));
            let alone = render_activity(
                &log(vec![entry(2026, 9, 29, 8, 0, vec![granted])]),
                now(),
                false,
            );
            assert!(alone.contains("adding search to prod is approved"));
            assert!(!alone.contains("completes"));
        }

        #[test]
        fn a_release_entry_headlines_the_release() {
            let out = render_activity(
                &log(vec![entry(
                    2026,
                    9,
                    29,
                    8,
                    0,
                    vec![
                        HitchEvent::Released {
                            environment: s("dev"),
                        },
                        HitchEvent::Demoted {
                            environment: s("dev"),
                            branch: s("payments"),
                        },
                    ],
                )]),
                now(),
                false,
            );
            assert!(out.contains("  09:00  martin released dev\n"), "{out}");
            assert!(
                out.contains("         removed payments from dev\n"),
                "{out}"
            );
        }

        #[test]
        fn days_group_across_midnight_in_now_offset() {
            // now is UTC+1: 23:30Z on the 28th is 00:30 on the 29th (Today).
            let rendered = render_activity(
                &log(vec![
                    entry(
                        2026,
                        9,
                        28,
                        23,
                        30,
                        vec![rebuilt(RebuildOutcome::Unrecorded)],
                    ),
                    entry(
                        2026,
                        9,
                        28,
                        12,
                        0,
                        vec![rebuilt(RebuildOutcome::Unrecorded)],
                    ),
                    entry(
                        2026,
                        9,
                        27,
                        12,
                        0,
                        vec![rebuilt(RebuildOutcome::Unrecorded)],
                    ),
                    entry(2026, 9, 20, 8, 5, vec![rebuilt(RebuildOutcome::Unrecorded)]),
                ]),
                now(),
                false,
            );
            let expected = "Today\n  00:30  martin rebuilt dev\n\nYesterday\n  13:00  martin rebuilt dev\n\nSun 27 Sep 2026\n  13:00  martin rebuilt dev\n\nSun 20 Sep 2026\n  09:05  martin rebuilt dev\n";
            assert_eq!(rendered, expected);
        }

        #[test]
        fn extra_events_and_holds_are_continuation_lines() {
            let rendered = render_activity(
                &log(vec![entry(
                    2026,
                    9,
                    29,
                    8,
                    0,
                    vec![
                        HitchEvent::Unlocked {
                            environment: s("qa"),
                        },
                        rebuilt(RebuildOutcome::WithHolds {
                            included: vec![],
                            held: vec![HoldPair {
                                branch: s("dashboard"),
                                conflicts_with: s("payments"),
                            }],
                        }),
                    ],
                )]),
                now(),
                false,
            );
            assert_eq!(
                rendered,
                "Today\n  09:00  martin unlocked qa\n         rebuilt dev, holding dashboard\n         dashboard was held \u{2014} it conflicts with payments\n"
            );
        }

        #[test]
        fn footers_and_empty() {
            assert_eq!(
                render_activity(&log(vec![]), now(), false),
                "No activity yet.\n"
            );
            let mut l = log(vec![entry(
                2026,
                9,
                29,
                8,
                0,
                vec![rebuilt(RebuildOutcome::Unrecorded)],
            )]);
            l.skipped = vec![
                SkippedCommit {
                    commit: s("x"),
                    reason: s("bad"),
                },
                SkippedCommit {
                    commit: s("y"),
                    reason: s("bad"),
                },
            ];
            l.truncated = true;
            let rendered = render_activity(&l, now(), false);
            assert!(rendered.ends_with(
                "\n2 changes to hitch's settings could not be read and are not shown.\nOlder activity not shown \u{2014} use --limit to see more.\n"
            ), "{rendered:?}");
            l.skipped.truncate(1);
            l.truncated = false;
            assert!(render_activity(&l, now(), false)
                .contains("1 change to hitch's settings could not be read and is not shown."));
        }

        #[test]
        fn verbose_adds_the_commit_and_plain_never_does() {
            let l = log(vec![entry(
                2026,
                9,
                29,
                8,
                0,
                vec![rebuilt(RebuildOutcome::Unrecorded)],
            )]);
            assert!(
                render_activity(&l, now(), true).contains("rebuilt dev  (metadata commit abcdef1)")
            );
            assert!(!render_activity(&l, now(), false).contains("commit"));
        }

        #[test]
        fn not_rebuilt_since_pointer_present_and_absent() {
            let with = render_activity(
                &log(vec![entry(2026, 9, 29, 8, 0, vec![promoted()])]),
                now(),
                false,
            );
            assert!(with
                .contains("         dev has not been rebuilt since \u{2014} see hitch status\n"));

            let later_rebuild = render_activity(
                &log(vec![
                    entry(2026, 9, 29, 9, 0, vec![rebuilt(RebuildOutcome::Unrecorded)]),
                    entry(2026, 9, 29, 8, 0, vec![promoted()]),
                ]),
                now(),
                false,
            );
            assert!(!later_rebuild.contains("has not been rebuilt"));

            let same_entry = render_activity(
                &log(vec![entry(
                    2026,
                    9,
                    29,
                    8,
                    0,
                    vec![promoted(), rebuilt(RebuildOutcome::Unrecorded)],
                )]),
                now(),
                false,
            );
            assert!(!same_entry.contains("has not been rebuilt"));
        }

        #[test]
        fn pointer_covers_demote_and_base_change_and_ignores_other_envs_and_locks() {
            let demoted = HitchEvent::Demoted {
                environment: s("dev"),
                branch: s("feature/a"),
            };
            let out = render_activity(
                &log(vec![entry(2026, 9, 29, 8, 0, vec![demoted])]),
                now(),
                false,
            );
            assert!(out.contains("dev has not been rebuilt since"));

            let base = HitchEvent::BaseChanged {
                environment: s("dev"),
                from: s("main"),
                to: s("develop"),
            };
            let out = render_activity(
                &log(vec![entry(2026, 9, 29, 8, 0, vec![base])]),
                now(),
                false,
            );
            assert!(out.contains("dev has not been rebuilt since"));

            let other = HitchEvent::Rebuilt {
                environment: s("qa"),
                outcome: RebuildOutcome::Unrecorded,
            };
            let out = render_activity(
                &log(vec![
                    entry(2026, 9, 29, 9, 0, vec![other]),
                    entry(2026, 9, 29, 8, 0, vec![promoted()]),
                ]),
                now(),
                false,
            );
            assert!(out.contains("dev has not been rebuilt since"));
            assert!(!out.contains("qa has not been rebuilt"));

            let lock = HitchEvent::Locked {
                environment: s("dev"),
                by: None,
            };
            let out = render_activity(
                &log(vec![
                    entry(2026, 9, 29, 9, 0, vec![lock]),
                    entry(2026, 9, 29, 8, 0, vec![promoted()]),
                ]),
                now(),
                false,
            );
            assert!(out.contains("dev has not been rebuilt since"));
        }

        #[test]
        fn each_footer_alone_is_exact() {
            let base = entry(2026, 9, 29, 8, 0, vec![rebuilt(RebuildOutcome::Unrecorded)]);
            let mut l = log(vec![base.clone()]);
            l.truncated = true;
            assert_eq!(
                render_activity(&l, now(), false),
                "Today\n  09:00  martin rebuilt dev\n\nOlder activity not shown \u{2014} use --limit to see more.\n"
            );
            let mut l = log(vec![base]);
            l.skipped = vec![SkippedCommit {
                commit: s("x"),
                reason: s("bad"),
            }];
            assert_eq!(
                render_activity(&l, now(), false),
                "Today\n  09:00  martin rebuilt dev\n\n1 change to hitch's settings could not be read and is not shown.\n"
            );
        }

        #[test]
        fn an_event_less_entry_still_carries_the_verbose_suffix() {
            let l = log(vec![entry(2026, 9, 29, 8, 0, vec![])]);
            assert_eq!(
                render_activity(&l, now(), true),
                "Today\n  09:00  martin  (metadata commit abcdef1)\n"
            );
        }
    }
}
