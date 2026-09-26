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
use crate::core::state::{EnvironmentHealth, RepositoryStateSnapshot};
use crate::operations::model::{
    AppliedEffect, ConfirmationRequirement, EnvironmentProjection, ExecutionReceipt,
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

    for (title, projection) in [("Current", &plan.current), ("Proposed", &plan.proposed)] {
        out.push('\n');
        heading(&mut out, title);
        out.push_str(&format!("  {}\n", describe_projection(projection)));
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
        for effect in anchors {
            annotated(
                &mut out,
                "·",
                &format!(
                    "{} — created now, removed once the composed commit is reachable from a branch",
                    short_ref(&effect.refname())
                ),
            );
        }
    }

    if !plan.unaffected.is_empty() {
        out.push('\n');
        heading(&mut out, "Will not change");
        for resource in &plan.unaffected {
            out.push_str(&format!("  {}\n", resource.name));
        }
    }

    if !plan.warnings.is_empty() {
        out.push('\n');
        heading(&mut out, "Needs your decision");
        for warning in &plan.warnings {
            // By *kind*, never by matching the message: a renderer that
            // string-matched would silently reclassify every warning whose
            // wording changed, and a *blocking* warning rendered as advisory
            // is a plan that looks harmless.
            let glyph = if warning.is_blocking() {
                "⛔"
            } else {
                "⚠️"
            };
            annotated(&mut out, glyph, &warning.message);
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

/// `dev = main + auth + search`, in declaration order.
///
/// The order is composition order and is load-bearing
/// (`EnvironmentProjection::branches` says so), so this renders the list as it
/// is and never sorts it. A renderer that alphabetised the list would be
/// rendering a *different build* than the one it was handed.
fn describe_projection(projection: &EnvironmentProjection) -> String {
    let mut line = projection.base.clone();
    for branch in &projection.branches {
        line.push_str(" + ");
        line.push_str(&branch.branch);
    }
    format!("{} = {line}", projection.environment)
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
        PlannedEffect::TagCreation { name, target_sha } => {
            format!("tag {name} at {}", short(target_sha))
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
            ..
        } => {
            let glyph = if outcome.owes_effect() { "⧗" } else { "✓" };
            match outcome.reason() {
                Some(reason) => {
                    out.push_str(&format!("  {glyph} rebuild {environment} — {reason}\n"))
                }
                None => out.push_str(&format!("  {glyph} rebuild {environment}\n")),
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
fn render_resulting_state(out: &mut String, snapshot: &RepositoryStateSnapshot) {
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
            for input in changed_inputs {
                out.push_str(&format!(
                    "      {}   {} → {}\n",
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
                out.push_str(&format!("      {branch}   added to the declaration\n"));
            }
            for branch in removed {
                out.push_str(&format!("      {branch}   removed from the declaration\n"));
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
}

impl<'a, I> JsonDocument<'a, I> {
    /// The document for a command that planned and did not apply — a
    /// `--dry-run`, or a plan the apply refused before writing anything.
    pub fn preview(plan: &'a OperationPlan<I>) -> Self {
        Self {
            schema_version: JSON_SCHEMA_VERSION,
            plan: Some(plan),
            receipt: None,
        }
    }

    /// The document for a command that planned and applied.
    pub fn applied(plan: &'a OperationPlan<I>, receipt: &'a ExecutionReceipt) -> Self {
        Self {
            schema_version: JSON_SCHEMA_VERSION,
            plan: Some(plan),
            receipt: Some(receipt),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{
        ActualComposition, ApprovalPolicy, ChangedInput, DeclaredBranch, DesiredComposition,
        EnvironmentState,
    };
    use crate::operations::model::{
        CompositionPlan, ConfirmationRequirement, DependentRebuildOutcome, ExecutionReceipt,
        ExecutionWarning, OperationIntent, OperationKind, PlanFingerprint, PlanWarning,
        PlanWarningKind, ResourceKind, UnaffectedResource,
    };
    use crate::utils::build_record::PinnedBranch;
    use chrono::{TimeZone, Utc};

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
            current: projection("dev", "main", &[]),
            proposed: projection("dev", "main", &[]),
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
        ];
        for (intent, expected) in cases {
            assert_eq!(plan_headline(&plan(intent)), expected);
        }
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
        p.current = projection("dev", "main", &[("auth", &sha('1')), ("search", &sha('2'))]);
        p.proposed = projection(
            "dev",
            "main",
            &[
                ("auth", &sha('1')),
                ("search", &sha('2')),
                ("login", &sha('3')),
            ],
        );
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
        p.proposed = projection("dev", "main", &[("zebra", &sha('1')), ("apple", &sha('2'))]);
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
        let mut p = plan(OperationIntent::RebuildEnvironment {
            environment: "dev".into(),
        });
        p.proposed = projection("dev", "main", &[]);
        assert!(has_line(&render_plan(&p), "  dev = main"));
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
        assert_eq!(lines.len(), 6, "{rendered}");
        for line in &lines {
            let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                [
                    // The model's own words, passed through.
                    "hitch-metadata add login into 'dev' (now: auth, login)",
                    // A local ref update of the environment this plan composes is
                    // a rebuild, and the reader is told why the SHA moved.
                    "dev rebuild from 1 branch · aaaaaaa → fffffff",
                    "origin/dev publish aaaaaaa → fffffff",
                    // `refs/tags/` is both the resource *and* what the effect
                    // does to it, so the name appears twice. That is redundancy
                    // in a table, not a bug, and collapsing it would mean the
                    // renderer was deciding what the resource is.
                    "tag hitch-release-dev-to-main-2026-01-01T00-00-00Z tag hitch-release-dev-to-main-2026-01-01T00-00-00Z at fffffff",
                    "qa rebuild qa — it is built on 'dev', which was rebuilt",
                    "hitch-metadata prune login, search from qa",
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
            refname: "refs/heads/qa".into(),
        }];
        let rendered = render_receipt(&r);
        assert!(
            has_line(&rendered, "  ⧗ rebuild qa — branch 'auth' no longer exists"),
            "{rendered}"
        );
        assert!(!rendered.contains("✓ rebuild qa"), "{rendered}");
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
            has_line(&rendered, "      auth   aaaaaaa → bbbbbbb"),
            "{rendered}"
        );
        assert!(
            has_line(&rendered, "      login   added to the declaration"),
            "{rendered}"
        );
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
            has_line(&rendered, "      auth   aaaaaaa → gone"),
            "{rendered}"
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
}
