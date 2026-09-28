//! One read-only view of what hitch has been told and what hitch has built.
//!
//! Two questions, kept deliberately apart:
//!
//! - **Desired** is what `hitch-metadata` currently *declares*. It is a fact
//!   about a file, and it is always available.
//! - **Actual** is what the last published build record says hitch actually
//!   put in the environment branch. It is a fact about an *observed build*,
//!   and it is frequently unavailable — `hitch release` lands a branch without
//!   composing one, and so do both of `hitch resolve`'s publish paths.
//!
//! Keeping them apart is the point of the module. The alternative — one
//! "state" field blending what was asked for with what happened — cannot
//! answer either question honestly, and its most common failure is inventing
//! an Actual for a build hitch never observed. Here, absence is spelled
//! [`ActualComposition::LegacyUnknown`] and is a normal state, not a defect.
//!
//! # Staleness is decided by SHAs, never by timestamps
//!
//! The layer this replaces compared *commit timestamps* against a *wall-clock*
//! `rebuilt_at`. That is wrong for every rebased or cherry-picked branch and
//! for any skewed-clock commit, and it is wrong silently: it just reports "up
//! to date" when it is not. So a build record's pinned input SHAs are compared
//! against live refs instead, which makes the answer exact *and* explainable —
//! the difference is a specific `2a42d1c → 7c931af` rather than a bare verdict.
//! Timestamps survive here for presentation only.
//!
//! # Read-only, and offline
//!
//! [`build_state_snapshot`] writes nothing, takes no lock, and never touches
//! the network. In particular it resolves branch refs with
//! `rev_parse_opt("refs/heads/<b>")` falling back to
//! `rev_parse_opt("refs/remotes/origin/<b>")` — both object-database reads
//! against already-fetched remote-tracking refs — rather than
//! `branch_exists_anywhere`, which shells out to `git ls-remote --heads origin`
//! once per branch and put a network round trip inside a status command.

use crate::commands::global_context::GlobalContext;
use crate::types::Environment;
use crate::utils::build_record::{
    read_state, EnvironmentBuildRecord, EnvironmentBuildState, PinnedBranch, ResolutionUse,
};
use crate::utils::git_operations::GitOperations;
use crate::utils::prelude::{access_metadata_read_only, CompatibilityConflict};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

/// Everything hitch knows about the repository's declared and built state, at
/// one instant.
// `PartialEq`/`Eq` so a whole snapshot can be compared — `crate::operations`'s
// `ExecutionReceipt` carries one, and the plan-vs-apply tests compare what
// hitch says the state is against what the repository actually is. The one
// field that is not obviously a value is `captured_at`; it is a
// `DateTime<Utc>`, so it is a value too, and a test comparing snapshots
// should compare the environments rather than the capture time anyway.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct RepositoryStateSnapshot {
    /// Tip of `hitch-metadata` as read locally, if it resolves.
    ///
    /// Reported for correlation only. It is **not** a staleness input and
    /// nothing here compares it to anything — see
    /// [`crate::utils::build_record::EnvironmentBuildRecord::metadata_sha`]
    /// for why it cannot be.
    pub metadata_sha: Option<String>,
    pub current_branch: Option<String>,
    pub environments: Vec<EnvironmentState>,
    /// Feature×environment membership, as a first-class query rather than
    /// something each caller re-derives from the declaration.
    pub features: Vec<FeatureState>,
    pub captured_at: DateTime<Utc>,
}

/// One environment's declared composition, its built composition, and the
/// single verdict that relates them.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentState {
    pub name: String,
    pub base: String,
    pub desired: DesiredComposition,
    pub actual: ActualComposition,
    pub health: EnvironmentHealth,
    pub locked: bool,
    pub approval_policy: ApprovalPolicy,
    // Presentation only. Kept here, rather than left for a renderer to go read
    // `hitch.json` a second time, so that the whole view comes from one read:
    // two config reads in one command can disagree, and the disagreement would
    // be invisible. Nothing below compares any of these to anything — see the
    // module header.
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub rebuilt_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
}

impl EnvironmentState {
    /// One branch's standing in this environment, from the record alone.
    ///
    /// This cannot answer [`ActualMembership::AlreadyInBase`], which is a live
    /// git fact needing a `GitOperations` handle; the snapshot's `features`
    /// view applies that check and is the fuller answer. See
    /// `membership_within` for the order of authority.
    pub fn membership_of(&self, branch: &str) -> ActualMembership {
        // A record that speaks about this branch settles it, even if the ref
        // has since been deleted: a build that consumed a commit contained it,
        // and "the branch is gone" is a separate, separately-reportable fact.
        if let ActualComposition::FromRecord(actual) = &self.actual {
            let from_record = actual.membership_of(branch);
            if from_record != ActualMembership::Unknown {
                return from_record;
            }
        }

        let declared = self.desired.branches.iter().find(|b| b.name == branch);

        match declared {
            // Declared with no resolvable ref. Nothing else can be true of it.
            Some(DeclaredBranch { sha: None, .. }) => ActualMembership::Missing,
            _ => ActualMembership::Unknown,
        }
    }
}

/// Whether changes to this environment need sign-off. Carried as its own
/// struct rather than three loose fields so "the approval policy" is something
/// a caller can pass around intact.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPolicy {
    pub required: bool,
    pub min_approvals: usize,
    pub approvers: Vec<String>,
}

/// What `hitch-metadata` currently declares, resolved against live refs.
///
/// A declared branch whose ref resolves nowhere is kept with `sha: None`
/// rather than dropped: the *declaration* is a fact about `hitch.json` and does
/// not become untrue because the branch is missing.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct DesiredComposition {
    pub base: String,
    pub base_sha: Option<String>,
    /// In declaration order, which is promotion order and is semantic —
    /// composition is sequential. Never sorted.
    pub branches: Vec<DeclaredBranch>,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct DeclaredBranch {
    pub name: String,
    /// `None` when the ref resolves neither locally nor on the cached
    /// remote-tracking ref.
    pub sha: Option<String>,
}

/// What the last published build record says is in this environment.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActualComposition {
    /// A record was found and read.
    FromRecord(Box<RecordActual>),
    /// No record at all. A normal state: the environment was last published by
    /// a hitch that does not write records, or by `release`/`resolve`, which
    /// deliberately do not because hitch has no truthful input for one.
    LegacyUnknown,
    /// A record exists but cannot be trusted. The reason is for display.
    Unreadable { reason: String },
}

impl ActualComposition {
    /// The record, when there is a trustworthy one.
    pub fn record(&self) -> Option<&EnvironmentBuildRecord> {
        self.actual().map(|a| &a.record)
    }

    /// The read record's derived contents, when there is a trustworthy one.
    ///
    /// Distinct from [`Self::record`] because the two answer different
    /// questions: the *declared* facts (what was declared, what base, what
    /// metadata commit) live on `EnvironmentBuildRecord`, and the *resolved*
    /// ones (which branches actually made it in, which conflicted, which
    /// resolutions were replayed) live on `RecordActual`. A caller that wanted
    /// the second and reached for the first would silently see empty lists.
    pub fn actual(&self) -> Option<&RecordActual> {
        match self {
            ActualComposition::FromRecord(a) => Some(a),
            _ => None,
        }
    }
}

/// The contents of a read build record, kept whole so callers never re-read
/// the ref to answer a second question about the same build.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct RecordActual {
    /// The environment branch's tip when the record was read. `None` means the
    /// record describes a branch that is now gone.
    pub tip: Option<String>,
    pub base_sha: String,
    pub included: Vec<PinnedBranch>,
    pub held: Vec<CompatibilityConflict>,
    pub replayed_resolutions: Vec<ResolutionUse>,
    pub built_at: DateTime<Utc>,
    pub record: EnvironmentBuildRecord,
}

impl RecordActual {
    /// Where one branch sits in the build this record describes.
    ///
    /// `Unknown` for a branch the record never mentions — which includes every
    /// branch promoted since the build ran, and is exactly why the declaration
    /// delta is reported separately as `added` / `removed` on
    /// [`EnvironmentHealth::NeedsRebuild`].
    pub fn membership_of(&self, branch: &str) -> ActualMembership {
        if self.included.iter().any(|b| b.branch == branch) {
            return ActualMembership::Included;
        }
        if self.held.iter().any(|c| c.branch == branch) {
            return ActualMembership::Held;
        }
        ActualMembership::Unknown
    }
}

/// Where one branch stands in one environment.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActualMembership {
    /// In the last build's composition.
    Included,
    /// Declared, but excluded from the last build because it conflicted.
    Held,
    /// Already reachable from the base branch, so promoting it again is a
    /// no-op. A live git fact, not a record fact — it holds with or without a
    /// record.
    AlreadyInBase,
    /// Declared, but no ref resolves for it.
    Missing,
    /// Not established: either no record describes this branch, or the record
    /// predates its promotion.
    Unknown,
}

/// An input that moved since the build described by the current record.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ChangedInput {
    pub branch: String,
    /// The SHA this build consumed.
    pub previous_sha: Option<String>,
    /// The SHA now. `None` when the branch has since disappeared — a deletion
    /// is a change too, and hiding it behind a missing SHA would make it
    /// invisible.
    pub current_sha: Option<String>,
}

impl ChangedInput {
    /// 7-character prefixes for the spec §11.2 `2a42d1c → 7c931af` form.
    /// An absent side reads `gone`, which is the honest rendering of a
    /// branch that no longer resolves.
    pub fn short(&self) -> (String, String) {
        let shorten = |s: &Option<String>| match s {
            Some(sha) => sha.chars().take(7).collect(),
            None => "gone".to_string(),
        };
        (shorten(&self.previous_sha), shorten(&self.current_sha))
    }
}

/// The single verdict for an environment, replacing four separate timestamp
/// comparisons that used to answer this question.
///
/// # Precedence
///
/// `MissingBranch` is decided first, before any record is read: a record
/// describing a branch that does not exist is not a state worth summarising,
/// and reporting one anyway would be reporting on the past.
///
/// Then `NeedsRebuild` outranks `PartiallyRealised`. `PartiallyRealised` is a
/// durable claim about the build that produced the current tip; `NeedsRebuild`
/// means that build is *superseded*. Answering "partially realised" for a
/// build whose inputs have since moved would state something true about
/// history while implying something false about now.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentHealth {
    /// The build is current with the declaration and included everything.
    Realised,
    /// The build is current, but deliberately excluded these branches.
    PartiallyRealised { held: Vec<String> },
    /// The build is behind. `changed_inputs` are branches whose tip moved;
    /// `added` / `removed` are declaration changes no SHA comparison can see,
    /// such as a `--no-rebuild` promotion.
    NeedsRebuild {
        changed_inputs: Vec<ChangedInput>,
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// Never built: no record, and no `rebuilt_at` either.
    NeverBuilt,
    /// A build exists but hitch cannot say what it contained — no record, an
    /// unreadable one, or one that no longer matches the branch.
    LegacyUnknown,
    /// The environment branch does not exist locally.
    MissingBranch,
}

impl EnvironmentHealth {
    /// Whether there is pending work a caller can name. The summary counters
    /// call this rather than re-deciding the question with their own
    /// `matches!`, which is how a second verdict gets invented.
    pub fn is_actionable(&self) -> bool {
        matches!(
            self,
            EnvironmentHealth::NeedsRebuild { .. }
                | EnvironmentHealth::NeverBuilt
                | EnvironmentHealth::MissingBranch
        )
    }

    /// A short label for the variant, for renderers that want a noun rather
    /// than a match.
    pub fn label(&self) -> &'static str {
        match self {
            EnvironmentHealth::Realised => "realised",
            EnvironmentHealth::PartiallyRealised { .. } => "partially realised",
            EnvironmentHealth::NeedsRebuild { .. } => "needs rebuild",
            EnvironmentHealth::NeverBuilt => "never built",
            EnvironmentHealth::LegacyUnknown => "actual unknown",
            EnvironmentHealth::MissingBranch => "branch missing",
        }
    }
}

/// One feature branch, and everywhere it is declared.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct FeatureState {
    pub name: String,
    /// One entry per environment that declares this branch, in environment
    /// name order. Environments that do not declare it are absent rather than
    /// present-and-`Unknown`; "not desired" is the caller's absence, not a
    /// membership verdict.
    pub memberships: Vec<FeatureMembership>,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct FeatureMembership {
    pub environment: String,
    /// Whether the environment declares this branch.
    pub desired: bool,
    pub actual: ActualMembership,
}

/// Resolve a branch to a commit, locally first and then from the cached
/// remote-tracking ref.
///
/// Both are object-database reads. This deliberately does **not** use
/// `branch_exists_anywhere`, which shells out to `ls-remote` and would put a
/// network round trip per declared branch inside a read-only snapshot.
fn live_sha(git: &GitOperations, branch: &str) -> Option<String> {
    git.rev_parse_opt(&format!("refs/heads/{}", branch))
        .ok()
        .flatten()
        .or_else(|| {
            git.rev_parse_opt(&format!("refs/remotes/origin/{}", branch))
                .ok()
                .flatten()
        })
}

/// Read the whole repository's declared and built state.
pub fn build_state_snapshot(context: &GlobalContext) -> Result<RepositoryStateSnapshot> {
    let git = context.git();
    let config = access_metadata_read_only(context, |c| Ok(c.clone()))?;

    let current_branch = git.get_current_branch().ok();
    let metadata_sha = git
        .rev_parse_opt("refs/heads/hitch-metadata")
        .ok()
        .flatten();

    let mut environments = Vec::with_capacity(config.environments.len());
    // Inverted from the declaration during the same pass, so the feature view
    // is derived from the same read rather than rebuilt from the config after.
    let mut promoted_to: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (name, env) in &config.environments {
        for branch in &env.branches {
            promoted_to
                .entry(branch.clone())
                .or_default()
                .push(name.clone());
        }
        environments.push(build_environment_state(git, name, env));
    }
    environments.sort_by(|a, b| a.name.cmp(&b.name));

    let mut already_in_base: HashMap<(String, String), bool> = HashMap::new();
    let features = promoted_to
        .into_iter()
        .map(|(branch, env_names)| {
            let mut memberships: Vec<FeatureMembership> = env_names
                .into_iter()
                .filter_map(|env_name| {
                    let env_state = environments.iter().find(|e| e.name == env_name)?;
                    let actual = membership_within(git, &mut already_in_base, env_state, &branch);
                    Some(FeatureMembership {
                        environment: env_name,
                        desired: true,
                        actual,
                    })
                })
                .collect();
            memberships.sort_by(|a, b| a.environment.cmp(&b.environment));
            FeatureState {
                name: branch,
                memberships,
            }
        })
        .collect();

    Ok(RepositoryStateSnapshot {
        metadata_sha,
        current_branch,
        environments,
        features,
        captured_at: Utc::now(),
    })
}

/// Is `branch` already reachable from `base`? Memoised per `(branch, base)`
/// pair, because environments usually share a base and the underlying call
/// spawns a subprocess.
fn in_base_cached(
    git: &GitOperations,
    cache: &mut HashMap<(String, String), bool>,
    branch: &str,
    base: &str,
) -> bool {
    if let Some(hit) = cache.get(&(branch.to_string(), base.to_string())) {
        return *hit;
    }
    // `merge-base --is-ancestor` needs both sides to resolve; a missing ref is
    // a plain false, not an error worth propagating into a status view.
    let answer = git.is_branch_merged_into(branch, base).unwrap_or(false);
    cache.insert((branch.to_string(), base.to_string()), answer);
    answer
}

/// The full membership verdict for one branch in one environment, in a fixed
/// order of authority. This is what the snapshot's `features` view reports.
///
/// [`EnvironmentState::membership_of`] is steps 1–2 and 4 only; this adds the
/// live base check, which needs a `GitOperations` handle and so cannot live on
/// a plain data type. The two answer slightly different questions and the
/// difference is deliberate: a record that names the branch is describing a
/// build that demonstrably consumed it, which outranks a live question about
/// where the branch's commits can be reached from.
fn membership_within(
    git: &GitOperations,
    already_in_base: &mut HashMap<(String, String), bool>,
    env: &EnvironmentState,
    branch: &str,
) -> ActualMembership {
    match env.membership_of(branch) {
        // 1. The record speaks: it is describing a build, and a build is a
        //    fact. This holds even if the branch has since been deleted.
        from_record @ (ActualMembership::Included | ActualMembership::Held) => from_record,
        // 2. Declared, but nothing resolves it. Nothing else can be true.
        ActualMembership::Missing => ActualMembership::Missing,
        // 3. The record is silent, but a live fact can still answer — the
        //    branch may already be reachable from the base, in which case
        //    promoting it again is a no-op.
        ActualMembership::Unknown => {
            let declared_resolves = env
                .desired
                .branches
                .iter()
                .any(|b| b.name == branch && b.sha.is_some());
            if declared_resolves && in_base_cached(git, already_in_base, branch, &env.base) {
                ActualMembership::AlreadyInBase
            } else {
                ActualMembership::Unknown
            }
        }
        // Unreachable: `membership_of` has no base check to return this. Kept
        // so adding a new membership variant fails to compile here rather than
        // silently falling through a wildcard.
        ActualMembership::AlreadyInBase => ActualMembership::AlreadyInBase,
    }
}

fn build_environment_state(git: &GitOperations, name: &str, env: &Environment) -> EnvironmentState {
    let desired = DesiredComposition {
        base: env.base.clone(),
        base_sha: live_sha(git, &env.base),
        branches: env
            .branches
            .iter()
            .map(|b| DeclaredBranch {
                name: b.clone(),
                sha: live_sha(git, b),
            })
            .collect(),
    };

    let record_state = read_state(git, name).unwrap_or(EnvironmentBuildState::Unreadable {
        reason: "build record could not be read".to_string(),
    });

    // Local-only, matching `read_state`'s own definition of the environment
    // branch's live tip. An environment branch that exists only on origin has
    // not been built here, and resolving it through the remote-tracking ref
    // would make the snapshot disagree with the record reader on purpose.
    let env_exists = git
        .rev_parse_opt(&format!("refs/heads/{}", name))
        .ok()
        .flatten()
        .is_some();

    let (actual, health) = classify(env, &desired, env_exists, record_state);

    EnvironmentState {
        name: name.to_string(),
        base: env.base.clone(),
        desired,
        actual,
        health,
        locked: env.is_locked(),
        approval_policy: ApprovalPolicy {
            required: env.requires_approval,
            min_approvals: env.min_approvals,
            approvers: env.approvers.clone(),
        },
        locked_by: env.locked_by.clone(),
        locked_at: env.locked_at,
        rebuilt_at: env.rebuilt_at,
        released_at: env.released_at,
    }
}

/// Split the record state into an Actual and a verdict, in one place, so the
/// two can never be derived by different code.
fn classify(
    env: &Environment,
    desired: &DesiredComposition,
    env_exists: bool,
    record_state: EnvironmentBuildState,
) -> (ActualComposition, EnvironmentHealth) {
    // Checked before the record is even read: a record describing a branch
    // that is not there is not a state worth summarising.
    if !env_exists {
        return (
            ActualComposition::LegacyUnknown,
            EnvironmentHealth::MissingBranch,
        );
    }

    match record_state {
        EnvironmentBuildState::LegacyUnknown => (
            ActualComposition::LegacyUnknown,
            if env.rebuilt_at.is_some() {
                EnvironmentHealth::LegacyUnknown
            } else {
                EnvironmentHealth::NeverBuilt
            },
        ),
        EnvironmentBuildState::Unreadable { reason } => (
            ActualComposition::Unreadable { reason },
            EnvironmentHealth::LegacyUnknown,
        ),
        EnvironmentBuildState::Known(record) => {
            let actual = record_actual(&record, Some(record.result_sha.clone()));
            (
                ActualComposition::FromRecord(Box::new(actual.clone())),
                health_from_record(desired, &actual),
            )
        }
        EnvironmentBuildState::ResultMismatch { record, live_tip } => {
            // The record still describes a build that happened; it just no
            // longer describes the branch. Its pinned SHAs remain the right
            // baseline for "what has changed since", so the verdict stays
            // computable and specific instead of collapsing to unknown.
            let actual = record_actual(&record, live_tip);
            (
                ActualComposition::FromRecord(Box::new(actual.clone())),
                health_from_record(desired, &actual),
            )
        }
    }
}

fn record_actual(record: &EnvironmentBuildRecord, tip: Option<String>) -> RecordActual {
    RecordActual {
        tip,
        base_sha: record.base_sha.clone(),
        included: record.included_branches.clone(),
        held: record.held.clone(),
        replayed_resolutions: record.replayed_resolutions.clone(),
        built_at: record.built_at,
        record: record.clone(),
    }
}

fn health_from_record(desired: &DesiredComposition, actual: &RecordActual) -> EnvironmentHealth {
    let mut changed_inputs: Vec<ChangedInput> = Vec::new();

    // The base's current SHA is already resolved into `desired`; re-resolving
    // it here would be a second ref read for a value we are holding.
    if let Some(current) = &desired.base_sha {
        if *current != actual.base_sha {
            changed_inputs.push(ChangedInput {
                branch: desired.base.clone(),
                previous_sha: Some(actual.base_sha.clone()),
                current_sha: Some(current.clone()),
            });
        }
    } else {
        // The base itself no longer resolves. That is a change, and dropping it
        // would report a build as current when its own foundation is gone.
        changed_inputs.push(ChangedInput {
            branch: desired.base.clone(),
            previous_sha: Some(actual.base_sha.clone()),
            current_sha: None,
        });
    }

    for pinned in &actual.record.desired_branches {
        let current = desired
            .branches
            .iter()
            .find(|d| d.name == pinned.branch)
            .and_then(|d| d.sha.clone());
        if current.as_deref() != Some(pinned.sha.as_str()) {
            changed_inputs.push(ChangedInput {
                branch: pinned.branch.clone(),
                previous_sha: Some(pinned.sha.clone()),
                current_sha: current,
            });
        }
    }

    let recorded_names = actual.record.desired_branch_names();
    let added: Vec<String> = desired
        .branches
        .iter()
        .map(|b| b.name.clone())
        .filter(|n| !recorded_names.contains(&n.as_str()))
        .collect();
    let removed: Vec<String> = recorded_names
        .iter()
        .filter(|n| !desired.branches.iter().any(|d| &d.name == *n))
        .map(|n| n.to_string())
        .collect();

    if !changed_inputs.is_empty() || !added.is_empty() || !removed.is_empty() {
        return EnvironmentHealth::NeedsRebuild {
            changed_inputs,
            added,
            removed,
        };
    }

    if actual.held.is_empty() {
        EnvironmentHealth::Realised
    } else {
        EnvironmentHealth::PartiallyRealised {
            held: actual.held.iter().map(|c| c.branch.clone()).collect(),
        }
    }
}
