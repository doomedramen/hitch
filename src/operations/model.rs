//! The types a plan is made of, and the types a receipt is made of.
//!
//! See the `super` module header for why a plan is a decision rather than a
//! recipe. This file is deliberately declarative: it holds the vocabulary
//! every operation shares, and the planners in sibling modules decide what
//! fills it in.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

use crate::core::state::{ChangedInput, RepositoryStateSnapshot};
use crate::utils::build_record::PinnedBranch;
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::CompatibilityConflict;

/// Which operation a plan or receipt describes.
///
/// The variant set grows as operations migrate onto the plan/apply path; it is
/// not a closed enumeration of the CLI's commands. An operation with no
/// variant here has not been migrated, and a receipt can therefore only ever
/// name an operation that actually went through a planner — which is the
/// point. An unmigrated operation is *absent* from the model, never
/// mislabelled as something it was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationKind {
    Rebuild,
}

impl OperationKind {
    /// The command a user would type to re-run this operation, used to end
    /// error messages. Kept as a method rather than a field so a kind can
    /// never carry a remedy string that disagrees with itself.
    pub fn command_hint(self, argument: &str) -> String {
        match self {
            OperationKind::Rebuild => format!("hitch rebuild {}", argument),
        }
    }
}

impl fmt::Display for OperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationKind::Rebuild => write!(f, "rebuild"),
        }
    }
}

/// What the user asked for, in structured form.
///
/// The spec's renderer wants a headline like `Promote feature/login → dev`.
/// That sentence is *derived* from this enum, and deriving it in one place is
/// the whole reason this type exists: if the plan carried prose instead, every
/// renderer would re-derive it, and they would eventually disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationIntent {
    /// Regenerate `<environment>` from its declared base plus promoted
    /// branches.
    RebuildEnvironment { environment: String },
}

impl fmt::Display for OperationIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperationIntent::RebuildEnvironment { environment } => {
                write!(f, "Rebuild '{}' from its declared inputs", environment)
            }
        }
    }
}

/// What hitch knows about one environment's branch composition at one moment.
///
/// A rebuild plan is single-environment and predicts exactly three things: the
/// base it composes onto, the branches it folds in (in order), and where the
/// environment branch points. This is a narrow projection rather than a
/// general state model on purpose — [`crate::core::state::RepositoryStateSnapshot`]
/// is the authority on state, and a plan does not get a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentProjection {
    pub environment: String,
    pub base: String,
    /// Promoted branches in **declaration order**, each pinned to the SHA this
    /// plan consumed. Order is composition order and is load-bearing: never
    /// sort this list, in the planner or in anything downstream.
    pub branches: Vec<PinnedBranch>,
    /// `refs/heads/<environment>` as observed at read time. `None` means the
    /// branch does not exist yet, which is the ordinary first-build case and
    /// not an error.
    pub branch_sha: Option<String>,
}

/// What the composition did with one declared branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedBranchState {
    /// Folded into the composition.
    Included,
    /// Could not be folded in; the running composition did not advance past
    /// it. A hold is a *fact about this composition*, not a prediction about
    /// the next one.
    Held {
        conflicts_with: String,
        files: Vec<String>,
    },
    /// Folded in, but from a recorded human resolution replayed over the
    /// conflict rather than from a fresh merge.
    ///
    /// A replayed branch is also `Included` — it is in the result. It is a
    /// separate variant rather than a flag on `Included` because it is
    /// materially different information for a reader: the content of this
    /// merge was authored by a person and is replayable, and that is worth a
    /// line in a receipt.
    ReplayedResolution { resolution_id: String },
    /// Folded in, and produced no tree change — the merge was a no-op against
    /// the running composition. Distinct from `Included` because only the
    /// branches that actually advanced the composition get their own commit;
    /// conflating the two is how a plan ends up claiming N branches landed
    /// when N−1 commits exist.
    AlreadyInBase,
    /// Declared in the environment but absent from both the included and the
    /// held list. Cannot happen with today's planner; modelled explicitly so
    /// that a future planner bug renders as "this branch was accounted for
    /// nowhere" instead of silently reading as `Included`.
    Missing,
}

/// One declared branch, and what the plan does with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBranch {
    pub branch: String,
    /// The SHA this plan consumed. The plan is a decision about *these*
    /// objects, not about the branch name.
    pub sha: String,
    pub state: PlannedBranchState,
}

/// The composition a plan describes.
///
/// Generic in spirit (the spec anticipates approvals and dependent rebuilds),
/// narrowed here to the one shape rebuild produces, because a model with
/// variants no planner fills in is a model that lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionPlan {
    pub environment: String,
    pub base: PinnedBranch,
    /// Declaration order preserved. See [`EnvironmentProjection::branches`].
    pub branches: Vec<PlannedBranch>,
    /// The commit this plan will land as `refs/heads/<environment>`. Already
    /// exists in the object database — see the module header.
    pub result_sha: String,
    pub holds: Vec<CompatibilityConflict>,
}

impl CompositionPlan {
    /// Branches actually in the result, in composition order. A replayed
    /// branch counts: it is in the result, and callers reconstructing this
    /// list by hand is exactly how the build record came to drop a branch
    /// (`src/utils/prelude.rs`, the `replayed` fix-up).
    pub fn included(&self) -> impl Iterator<Item = &str> {
        self.branches
            .iter()
            .filter(|b| {
                matches!(
                    b.state,
                    PlannedBranchState::Included
                        | PlannedBranchState::ReplayedResolution { .. }
                        | PlannedBranchState::AlreadyInBase
                )
            })
            .map(|b| b.branch.as_str())
    }

    /// Branches held out of the result, in composition order.
    pub fn held(&self) -> impl Iterator<Item = &str> {
        self.branches
            .iter()
            .filter(|b| matches!(b.state, PlannedBranchState::Held { .. }))
            .map(|b| b.branch.as_str())
    }
}

/// Something the plan will change.
///
/// The variant set is per-operation and is the **allow-list a receipt is
/// checked against**: a missing variant does not mean "no change here", it
/// means *this operation cannot cause that kind of change*. A new effect type
/// therefore requires a new variant in both this enum and
/// [`AppliedEffect`], and the §30.3 invariant — every planned effect is
/// applied or explained — is checked by construction once they match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedEffect {
    /// A ref under `refs/hitch/` or on `hitch-metadata`: the build record, the
    /// `rebuilt_at` stamp, an anchor.
    MetadataChange {
        refname: String,
        description: String,
    },
    /// A `refs/heads/*` ref in the local repository.
    LocalRefUpdate {
        refname: String,
        /// The ref's current value, or `None` when it does not exist yet
        /// (a first build).
        old: Option<String>,
        new: String,
    },
    /// A ref on the remote, reached via a push. Predicted only when the
    /// command will actually attempt one.
    RemoteRefUpdate {
        refname: String,
        old: Option<String>,
        new: String,
    },
}

impl PlannedEffect {
    /// The ref this effect acts on, for matching an applied effect back to its
    /// prediction.
    pub fn refname(&self) -> &str {
        match self {
            PlannedEffect::MetadataChange { refname, .. }
            | PlannedEffect::LocalRefUpdate { refname, .. }
            | PlannedEffect::RemoteRefUpdate { refname, .. } => refname,
        }
    }

    /// The value this effect claims will be there afterwards. `None` for a
    /// [`PlannedEffect::MetadataChange`], whose `description` is human text
    /// and has no single ref value to predict.
    pub fn predicted_value(&self) -> Option<&str> {
        match self {
            PlannedEffect::MetadataChange { .. } => None,
            PlannedEffect::LocalRefUpdate { new, .. }
            | PlannedEffect::RemoteRefUpdate { new, .. } => Some(new),
        }
    }
}

/// What kind of resource the plan read and will not write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Branch,
    Environment,
    Remote,
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceKind::Branch => write!(f, "branch"),
            ResourceKind::Environment => write!(f, "environment"),
            ResourceKind::Remote => write!(f, "remote"),
        }
    }
}

/// A resource the plan consumed as a read and promises not to change.
///
/// Filled from the *same* pinned inputs the composition consumed, so "will not
/// change" cannot name a branch the plan never actually read — a promise about
/// a branch the plan did not look at is not a promise, it is a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaffectedResource {
    pub kind: ResourceKind,
    pub name: String,
}

/// A fact the user should see about the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWarning {
    pub message: String,
    /// `true` means **the plan refuses to apply as written** — a halt-policy
    /// conflict, a missing input. `false` means the user should know but the
    /// operation proceeds (a held branch: real, visible, not fatal).
    ///
    /// The distinction is load-bearing. Collapsing the two is how a warning
    /// starts being ignored, and a reader cannot tell which warnings are safe
    /// to skim unless the type says so.
    pub blocking: bool,
}

/// Whether and why a human must confirm before this plan is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationRequirement {
    pub required: bool,
    pub reason: Option<String>,
}

impl ConfirmationRequirement {
    pub fn not_required() -> Self {
        Self {
            required: false,
            reason: None,
        }
    }

    pub fn required(reason: impl Into<String>) -> Self {
        Self {
            required: true,
            reason: Some(reason.into()),
        }
    }
}

/// Everything this plan depends on, as observed when it was built.
///
/// **This is a whitelist of dependencies, not a snapshot of the repository.** A
/// ref that exists in the live repository and is absent from here is not a
/// change, and comparing the two is exactly backwards: treating an unrelated
/// ref as a dependency would refuse the apply every time anyone else pushed
/// anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanFingerprint {
    /// Tip of `hitch-metadata` the declaration was read from. The
    /// declaration is the *intent*; without this a plan could outlive a
    /// promote into this environment and still claim to be current.
    pub metadata_sha: Option<String>,
    /// Fully-qualified refname → SHA. Fully qualified because `refs/heads/dev`
    /// and `refs/remotes/origin/dev` are different dependencies with
    /// different reasons to go stale, and a bare name cannot tell them apart.
    pub refs: BTreeMap<String, String>,
    /// Refname → observed value, where the value may legitimately be absent
    /// (a remote branch hitch has not fetched). `None` and "no such entry" are
    /// different facts, which is why this is an `Option` *value* rather than
    /// the entry simply being omitted.
    pub remote_refs: BTreeMap<String, Option<String>>,
    /// Content-addressed identities of the resolutions this plan replayed.
    ///
    /// Keys are content hashes (`utils::resolutions::resolution_key`), so a
    /// key match **is** an identical-content match. Recording the blobs
    /// themselves would add nothing; recording the branch names instead would
    /// be wrong, because the same branch replayed against a later conflict
    /// resolves under a different key.
    pub resolution_keys: Vec<String>,
}

impl PlanFingerprint {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a ref the plan read, along with the SHA it read.
    pub fn track_ref(&mut self, refname: impl Into<String>, sha: impl Into<String>) {
        self.refs.insert(refname.into(), sha.into());
    }

    /// Record a remote ref the plan read. `None` records "observed, absent" —
    /// distinct from not recording it at all, because a plan that saw no
    /// remote branch and a plan that never looked are different plans.
    pub fn track_remote_ref(&mut self, refname: impl Into<String>, sha: Option<String>) {
        self.remote_refs.insert(refname.into(), sha);
    }

    /// Record a replayed resolution, normalising order and duplicates so two
    /// plans that replayed the same set fingerprint identically regardless of
    /// the order the composition happened to report them in.
    pub fn track_resolution(&mut self, key: impl Into<String>) {
        self.resolution_keys.push(key.into());
        self.resolution_keys.sort();
        self.resolution_keys.dedup();
    }

    /// A stable identity for this set of dependencies.
    ///
    /// Hashed with `git hash-object` over a hand-rolled canonical encoding —
    /// the same mechanism `utils::resolutions::resolution_key` uses, for the
    /// same reason: no new dependency, and a value that is a normal git
    /// object id.
    ///
    /// The canonical encoding is length-prefixed rather than separator-delimited
    /// because refnames and branch names legitimately contain spaces and `\0`
    /// is illegal in both, and the lengths are emitted so a truncated pair
    /// cannot be made to collide with a shorter one. `Option::None` encodes as
    /// the literal `-`, which a real SHA never is, so "absent" cannot collide
    /// with any observed value.
    ///
    /// Takes `&GitOperations` rather than returning a digest unconditionally
    /// so that this cannot grow a second hashing dependency by accident, and
    /// so the fingerprint's value is provably a git object id.
    pub fn digest(&self, git: &GitOperations) -> anyhow::Result<String> {
        let mut canonical = String::new();
        let mut field = |key: &str, value: Option<&str>| {
            canonical.push_str(&format!("{}\0{}\0", key.len(), key));
            match value {
                Some(v) => canonical.push_str(&format!("{}\0{}\n", v.len(), v)),
                None => canonical.push_str("0\0-\n"),
            }
        };
        field("metadata_sha", self.metadata_sha.as_deref());
        for (refname, sha) in &self.refs {
            field(refname, Some(sha));
        }
        for (refname, sha) in &self.remote_refs {
            field(refname, sha.as_deref());
        }
        for key in &self.resolution_keys {
            field(key, Some("resolution"));
        }
        git.hash_object_bytes(canonical.as_bytes())
    }
}

/// An immutable description of a decision hitch has already made.
///
/// Generic over the per-operation detail (`RebuildPlanDetail` for rebuild) so
/// shared code can carry any operation's plan without knowing its shape, and
/// so a list of plans stays homogeneous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPlan<I> {
    /// Identity, for logs and for correlating a plan with its receipt.
    ///
    /// **Not a freshness proof.** Spec §7.4 is explicit that the plan ID alone
    /// proves nothing, and it is correct: the ID names *this* plan, and
    /// whether it is still current is [`fingerprint`]'s question alone.
    pub id: String,
    pub kind: OperationKind,
    pub intent: OperationIntent,
    pub fingerprint: PlanFingerprint,
    /// What is true now.
    pub current: EnvironmentProjection,
    /// What will be true after this plan applies.
    pub proposed: EnvironmentProjection,
    pub compositions: Vec<CompositionPlan>,
    pub effects: Vec<PlannedEffect>,
    pub unaffected: Vec<UnaffectedResource>,
    pub warnings: Vec<PlanWarning>,
    pub confirmation: ConfirmationRequirement,
    pub detail: I,
}

impl<I> OperationPlan<I> {
    /// The plan's `result_sha` for a single-environment operation, which is
    /// the only shape rebuild has.
    pub fn result_sha(&self) -> &str {
        self.detail_result_sha()
    }

    fn detail_result_sha(&self) -> &str {
        self.compositions
            .first()
            .map(|c| c.result_sha.as_str())
            .unwrap_or_default()
    }

    /// Every blocking warning, if any. A plan with one of these does not apply.
    pub fn blocked(&self) -> Option<&PlanWarning> {
        self.warnings.iter().find(|w| w.blocking)
    }
}

/// How an operation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutcome {
    /// Everything the plan predicted was applied.
    Applied,
    /// Everything the plan predicted was applied, *and* some declared branches
    /// are held out of the result.
    ///
    /// This must never be collapsed into [`OperationOutcome::Applied`]. The
    /// two call for different urgency and carry different CI meaning — a
    /// rebuild that exits 0 with branches held is a pipeline that looks green
    /// and is not shipping what it declared. For `hitch rebuild` this variant
    /// is what the exit-code-2 contract is made of.
    AppliedWithHolds,
    /// The operation stopped to ask a human, and nothing was applied. Carried
    /// by the model so an operation that can request approval has a first-class
    /// "asked and stopped" outcome rather than an error string.
    ApprovalRequested,
    /// The operation succeeded and the repository is already in the requested
    /// state. A *successful* outcome, not a degenerate or failed one: a
    /// rebuild whose inputs already match its result has done its job.
    NoChange,
}

impl OperationOutcome {
    pub fn is_success(self) -> bool {
        !matches!(self, OperationOutcome::ApprovalRequested)
    }
}

/// A change that actually happened.
///
/// Mirrors [`PlannedEffect`] and is read back from the repository *after* the
/// transaction, never copied from the plan. The `new` value here is an
/// observation; the `new` value in the plan was a prediction, and the entire
/// value of a receipt is the difference between the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedEffect {
    MetadataChange {
        refname: String,
        description: String,
    },
    LocalRefUpdate {
        refname: String,
        old: Option<String>,
        new: String,
    },
    RemoteRefUpdate {
        refname: String,
        old: Option<String>,
        new: String,
    },
}

impl AppliedEffect {
    pub fn refname(&self) -> &str {
        match self {
            AppliedEffect::MetadataChange { refname, .. }
            | AppliedEffect::LocalRefUpdate { refname, .. }
            | AppliedEffect::RemoteRefUpdate { refname, .. } => refname,
        }
    }
}

/// A fact learned *while* applying, which the plan could not have known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionWarning {
    pub message: String,
    /// True when the operation is complete but something is still owed — a
    /// push that has not landed, for instance. An owed effect is not a failure
    /// and not a success; reporting it as either is the bug.
    pub owes_effect: bool,
}

/// What actually happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt {
    /// The [`OperationPlan::id`] this receipt answers.
    pub plan_id: String,
    pub operation: OperationKind,
    /// Wall clock, for display and for correlating a receipt with a CI log.
    ///
    /// **Presentation only.** The same rule the build record's `rebuilt_at`
    /// follows: never read a timestamp for a verdict. Staleness is decided by
    /// comparing SHAs — see `core::state`.
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub outcome: OperationOutcome,
    pub effects: Vec<AppliedEffect>,
    pub warnings: Vec<ExecutionWarning>,
    /// The state of the repository after the operation, read from the single
    /// authority (`core::state::build_state_snapshot`) rather than re-derived
    /// from what the plan predicted. `None` when the operation failed before
    /// reaching a state worth describing.
    pub resulting_state: Option<RepositoryStateSnapshot>,
}

impl ExecutionReceipt {
    pub fn duration(&self) -> chrono::Duration {
        self.completed_at - self.started_at
    }

    /// True when the operation is complete but still owes the user something.
    pub fn has_owed_effects(&self) -> bool {
        self.warnings.iter().any(|w| w.owes_effect)
    }
}

/// Why an apply did not happen.
///
/// Every message ends with the exact command to run next. That is not
/// decoration: this is the one place the error is a human's only clue about
/// what went wrong, and a message without a next step is a strictly worse
/// experience than the rest of the CLI.
#[derive(Debug, thiserror::Error)]
pub enum PlanApplyError {
    #[error(
        "The plan for '{environment}' is no longer current:\n  {changed}\n\
         Nothing was changed. Re-run to calculate a fresh plan:\n  {remedy}"
    )]
    StalePlan {
        environment: String,
        changed: String,
        remedy: String,
    },

    #[error(
        "The plan for '{environment}' was refused by policy:\n  {reason}\n\
         Nothing was changed. To override this environment's conflict policy:\n  {remedy}"
    )]
    PolicyBlocked {
        environment: String,
        reason: String,
        remedy: String,
    },

    #[error(
        "The plan for '{environment}' hit a conflict:\n  {conflict}\n\
         Nothing was changed. Resolve the conflict, then rebuild:\n  {remedy}"
    )]
    Conflict {
        environment: String,
        conflict: String,
        remedy: String,
    },

    #[error(
        "'{branch}' moved while the plan was being applied:\n  {detail}\n\
         Nothing was changed. Re-run to calculate a fresh plan:\n  {remedy}"
    )]
    PublishRace {
        branch: String,
        detail: String,
        remedy: String,
    },

    #[error(
        "'{branch}' was rebuilt locally but could not be pushed:\n  {detail}\n\
         The local branch is published; the remote is not. To push it:\n  {remedy}"
    )]
    RemotePushFailed {
        branch: String,
        detail: String,
        remedy: String,
    },
}

impl PlanApplyError {
    /// Turn this into the `anyhow::Error` the rest of the CLI propagates.
    ///
    /// A typed error survives the conversion, which is what makes the
    /// distinction usable downstream: `err.downcast_ref::<PlanApplyError>()`
    /// still yields the `StalePlan` with its `changed` list, so a caller that
    /// wants to render the *structure* can, while `hitch`'s normal error path
    /// still prints the `Display` string a user reads. Erasing the type to a
    /// `String` at the boundary would throw away exactly the information the
    /// model exists to carry.
    pub fn into_anyhow(self) -> anyhow::Error {
        anyhow::Error::new(self)
    }

    /// Build the `changed` clause of a [`PlanApplyError::StalePlan`] from
    /// P3's own input-comparison type.
    ///
    /// P3 already renders `ChangedInput` as `2a42d1c → 7c931af` and already
    /// decided that an absent side reads `gone`, because a deletion is a
    /// change too. Re-deriving either here would be a second opinion about
    /// the same fact, which is the class of bug the state model was built to
    /// remove.
    pub fn stale_plan(environment: &str, changed: &[ChangedInput]) -> Self {
        let rendered = changed
            .iter()
            .map(|c| {
                let (previous, current) = c.short();
                format!("{}: {} → {}", c.branch, previous, current)
            })
            .collect::<Vec<_>>()
            .join("\n  ");
        PlanApplyError::StalePlan {
            environment: environment.to_string(),
            changed: rendered,
            remedy: format!("hitch rebuild {}", environment),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// Test-only plain-git helper, mirroring the one in
    /// `src/utils/build_record.rs`: a unit test in `src/` cannot reach the
    /// integration harness in `tests/test_framework/`, and must not spawn git
    /// with an inherited terminal stdin.
    fn run_git(repo: &Path, args: &[&str]) {
        #[allow(clippy::disallowed_methods)]
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("failed to run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A throwaway repo, only because [`PlanFingerprint::digest`] hashes
    /// through git. Nothing about the tests below depends on its contents —
    /// `hash-object` is a pure function of the bytes handed to it — so this is
    /// the smallest thing that provides a `GitOperations`.
    fn scratch() -> (tempfile::TempDir, GitOperations) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        run_git(repo, &["init"]);
        fs::write(repo.join("README.md"), "hello").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
        let git = GitOperations::new_at_path(&repo.to_string_lossy()).expect("open repo");
        (dir, git)
    }

    /// A digest is only useful if two dependency sets that differ never share
    /// one, so every component of the fingerprint gets its own case.
    fn digest_of(fingerprint: &PlanFingerprint, git: &GitOperations) -> String {
        fingerprint.digest(git).expect("hash-object")
    }

    fn base_fingerprint() -> PlanFingerprint {
        let mut fp = PlanFingerprint::new();
        fp.metadata_sha = Some("a".repeat(40));
        fp.track_ref("refs/heads/main", "b".repeat(40));
        fp.track_ref("refs/heads/feature", "c".repeat(40));
        fp.track_remote_ref("refs/remotes/origin/dev", Some("d".repeat(40)));
        fp.track_resolution("e".repeat(40));
        fp
    }

    #[test]
    fn the_digest_is_stable_for_the_same_dependencies() {
        let (_dir, git) = scratch();
        assert_eq!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&base_fingerprint(), &git)
        );
    }

    #[test]
    fn the_digest_changes_when_the_metadata_moves() {
        let (_dir, git) = scratch();
        let mut moved = base_fingerprint();
        moved.metadata_sha = Some("f".repeat(40));
        assert_ne!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&moved, &git)
        );
    }

    #[test]
    fn the_digest_changes_when_a_local_ref_moves() {
        let (_dir, git) = scratch();
        let mut moved = base_fingerprint();
        moved.track_ref("refs/heads/feature", "1".repeat(40));
        assert_ne!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&moved, &git)
        );
    }

    #[test]
    fn the_digest_changes_when_a_remote_ref_moves() {
        let (_dir, git) = scratch();
        let mut moved = base_fingerprint();
        moved.track_remote_ref("refs/remotes/origin/dev", Some("1".repeat(40)));
        assert_ne!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&moved, &git)
        );
    }

    #[test]
    fn the_digest_changes_when_a_resolution_is_replayed() {
        let (_dir, git) = scratch();
        let mut extra = base_fingerprint();
        extra.track_resolution("1".repeat(40));
        assert_ne!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&extra, &git)
        );
    }

    #[test]
    fn an_absent_remote_ref_does_not_collide_with_an_observed_one() {
        let (_dir, git) = scratch();
        let mut present = base_fingerprint();
        present.track_remote_ref("refs/remotes/origin/qa", Some("-".to_string()));
        let mut absent = base_fingerprint();
        absent.track_remote_ref("refs/remotes/origin/qa", None);
        assert_ne!(digest_of(&present, &git), digest_of(&absent, &git));
    }

    #[test]
    fn replaying_the_same_resolution_twice_fingerprints_once() {
        let (_dir, git) = scratch();
        let mut duplicated = base_fingerprint();
        duplicated.track_resolution("e".repeat(40));
        assert_eq!(
            digest_of(&base_fingerprint(), &git),
            digest_of(&duplicated, &git)
        );
    }

    #[test]
    fn every_error_ends_with_the_command_to_run_next() {
        let remedies = [
            PlanApplyError::stale_plan("dev", &[]).to_string(),
            PlanApplyError::PolicyBlocked {
                environment: "dev".into(),
                reason: "on-conflict is halt".into(),
                remedy: "hitch rebuild dev --force".into(),
            }
            .to_string(),
            PlanApplyError::Conflict {
                environment: "dev".into(),
                conflict: "feature/x conflicts with main".into(),
                remedy: "hitch rebuild dev".into(),
            }
            .to_string(),
            PlanApplyError::PublishRace {
                branch: "dev".into(),
                detail: "expected 1a2b3c, found 4d5e6f".into(),
                remedy: "hitch rebuild dev".into(),
            }
            .to_string(),
            PlanApplyError::RemotePushFailed {
                branch: "dev".into(),
                detail: "non-fast-forward".into(),
                remedy: "hitch push dev -f".into(),
            }
            .to_string(),
        ];
        for message in remedies {
            let last_line = message.lines().next_back().expect("non-empty");
            assert!(
                last_line.trim_start().starts_with("hitch "),
                "last line is not a command:\n{}",
                message
            );
        }
    }

    #[test]
    fn a_stale_plan_names_what_changed_with_both_shas() {
        let message = PlanApplyError::stale_plan(
            "dev",
            &[ChangedInput {
                branch: "feature/x".into(),
                previous_sha: Some("2a42d1c00000000000000000000000000000000".into()),
                current_sha: Some("7c931af00000000000000000000000000000000".into()),
            }],
        )
        .to_string();
        assert!(message.contains("feature/x"), "{}", message);
        assert!(message.contains("2a42d1c"), "{}", message);
        assert!(message.contains("7c931af"), "{}", message);
        assert!(message.contains("hitch rebuild dev"), "{}", message);
    }

    #[test]
    fn a_deleted_dependency_reads_as_gone_rather_than_vanishing() {
        let message = PlanApplyError::stale_plan(
            "dev",
            &[ChangedInput {
                branch: "feature/gone".into(),
                previous_sha: Some("2a42d1c00000000000000000000000000000000".into()),
                current_sha: None,
            }],
        )
        .to_string();
        assert!(message.contains("gone"), "{}", message);
    }

    #[test]
    fn a_replayed_branch_counts_as_included() {
        let composition = CompositionPlan {
            environment: "dev".into(),
            base: PinnedBranch {
                branch: "main".into(),
                sha: "0".repeat(40),
            },
            branches: vec![
                PlannedBranch {
                    branch: "a".into(),
                    sha: "1".repeat(40),
                    state: PlannedBranchState::Included,
                },
                PlannedBranch {
                    branch: "b".into(),
                    sha: "2".repeat(40),
                    state: PlannedBranchState::ReplayedResolution {
                        resolution_id: "3".repeat(40),
                    },
                },
                PlannedBranch {
                    branch: "c".into(),
                    sha: "4".repeat(40),
                    state: PlannedBranchState::AlreadyInBase,
                },
                PlannedBranch {
                    branch: "d".into(),
                    sha: "5".repeat(40),
                    state: PlannedBranchState::Held {
                        conflicts_with: "a".into(),
                        files: vec!["src/lib.rs".into()],
                    },
                },
            ],
            result_sha: "6".repeat(40),
            holds: vec![],
        };
        assert_eq!(
            composition.included().collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(composition.held().collect::<Vec<_>>(), vec!["d"]);
    }
}
