//! Why one branch or environment is in the state it is — the model behind
//! `hitch why` (spec §14).
//!
//! # The rule this module exists to enforce
//!
//! An explanation is only worth reading if it is *the same answer* the rest of
//! the CLI gives. So this module has no opinion of its own about what a
//! membership is: every membership it reports comes from
//! [`MatrixCell::classify`](crate::core::status::MatrixCell::classify), the one
//! classifier the matrix uses. A `why` that grew a second classifier would be
//! free to disagree with the grid, and a reader who checks one against the other
//! and finds a contradiction has learned that neither can be trusted.
//!
//! [`WhyMembership`] is a distinct type from
//! [`ActualMembership`](crate::core::state::ActualMembership) rather than a
//! re-export for one reason: `ActualMembership::Unknown` deliberately conflates
//! "this branch was promoted after the last build" with "no record describes the
//! last build", and §14 needs those as two different answers with two different
//! remedies. `MatrixCell` already draws that line; `WhyMembership` is that line,
//! named.
//!
//! # What this module does not do
//!
//! It opens no repository, spawns no git, and reads no clock. It is a pure
//! function of the [`RepositoryStateSnapshot`], which means every arm below is
//! testable from a hand-built value and none of them can be right by accident
//! because the repository happened to be in a convenient state.

use crate::core::render::EnvironmentEquation;
use crate::core::state::{ChangedInput, EnvironmentHealth, RepositoryStateSnapshot};
use crate::core::status::{classify_from_snapshot, has_record_for, MatrixCell};
use anyhow::bail;

/// What the caller has decided `hitch why` is being asked about.
///
/// Resolved by the *command*, not here: deciding whether a name is an
/// environment or a branch needs a `rev_parse_opt`, and this module is
/// deliberately the part that never touches git. See `commands/why.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhySubject {
    /// A branch, across every environment.
    Feature(String),
    /// A branch, as it stands in one environment.
    FeatureIn(String, String),
    /// An environment, as a whole.
    Environment(String),
}

/// What a `why` is explaining. Three forms, because "a branch", "a branch in a
/// place", and "a place" are three different questions and a single struct
/// would force fields that are meaningless for two of them into existence.
///
/// `tag = "form"` rather than serde's default externally-tagged representation:
/// the default nests the payload under a *variant name*, which means renaming
/// the `FeatureInEnvironment` variant — a Rust-side change with no effect on
/// behaviour — silently breaks every consumer of the `--json` document. An
/// internally-tagged form makes the discriminator a field the document owns.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum WhyExplanation {
    /// `hitch why <branch>` — the branch's standing everywhere.
    Feature(WhyFeatureExplanation),
    /// `hitch why <branch> <environment>` — one pair, in full.
    FeatureInEnvironment(WhyFeatureInEnvironment),
    /// `hitch why <environment>` — desired against actual.
    Environment(WhyEnvironmentExplanation),
}

/// Where a branch stands, in the vocabulary §14 needs.
///
/// A one-to-one shadow of [`MatrixCell`], and the conversion between them is the
/// only way a value of this type is ever constructed outside a test. `Unknown`
/// appears nowhere in it on purpose: the two things it stood for have been split
/// into [`WhyMembership::NeedsRebuild`] and
/// [`WhyMembership::ActualUnknown`], and this is the type that makes that split
/// unre-mergeable.
///
/// `snake_case` for the same reason [`MatrixCell`] has it: this reaches
/// `hitch why --json`, where the string is a consumer's only handle on the
/// value, and serde's default would freeze `NotDesired` — a Rust type name —
/// into that contract.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhyMembership {
    NotDesired,
    Included,
    Held,
    InBase,
    NeedsRebuild,
    ActualUnknown,
    Missing,
}

impl From<MatrixCell> for WhyMembership {
    fn from(cell: MatrixCell) -> WhyMembership {
        match cell {
            MatrixCell::NotDesired => WhyMembership::NotDesired,
            MatrixCell::Included => WhyMembership::Included,
            MatrixCell::Held => WhyMembership::Held,
            MatrixCell::InBase => WhyMembership::InBase,
            MatrixCell::NeedsRebuild => WhyMembership::NeedsRebuild,
            MatrixCell::ActualUnknown => WhyMembership::ActualUnknown,
            MatrixCell::Missing => WhyMembership::Missing,
        }
    }
}

impl WhyMembership {
    /// The word. The renderer's, not the model's — this exists so a caller that
    /// needs to *branch* on the membership does not match on a string.
    pub fn is_in_the_build(&self) -> bool {
        matches!(self, WhyMembership::Included | WhyMembership::InBase)
    }
}

/// A fact explaining why the membership is what it is.
///
/// Every variant is something read out of the snapshot. None of them is a
/// prediction, and none of them is a guess: `WhyReason` cannot express
/// "probably", because the one command that could (§14's `why`) must not be the
/// one that says so.
///
/// `snake_case`, as on [`WhyMembership`]: a reason reaches `hitch why --json`,
/// and the externally-tagged representation puts the variant name in the
/// document.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhyReason {
    /// Excluded from the last build because it conflicted. Carries the partner
    /// and the files because a reason that cannot be acted on is not a reason.
    HeldAgainst {
        conflicts_with: String,
        files: Vec<String>,
    },
    /// The environment is behind: this branch's tip moved since the build.
    ChangedSinceBuild { from: String, to: String },
    /// Promoted into this environment after the last build ran.
    PromotedSinceBuild,
    /// Removed from this environment's declaration after the last build ran, so
    /// the build still contains it.
    DemotedSinceBuild,
    /// Declared, but no ref resolves for it.
    NoRef,
    /// A build exists and hitch cannot describe what was in it.
    NoBuildRecord,
    /// The environment branch itself does not exist.
    EnvironmentBranchMissing,
    /// The environment's base moved after the build.
    BaseMoved { from: String, to: String },
    /// The branch's tip is already reachable from the environment's base, so
    /// building it would fold in nothing. Carried as its own reason rather than
    /// left to the environment-level `NoBuildRecord`, because "hitch has no
    /// record" does not explain an `In base` membership — this membership is
    /// knowable *without* any record, and it is the one cell where that is so.
    AlreadyInBase,
    /// The environment is behind for a reason that is not about this branch.
    /// Carried so a cell and its explanation cannot drift into saying a branch
    /// is fine when the environment it sits in is not.
    EnvironmentBehind,
}

/// The next thing to do, as a *choice* rather than a string.
///
/// The model decides which action; the renderer decides the words. A model that
/// emitted `"hitch rebuild dev"` would make the next action untestable without
/// matching on prose, and prose is exactly what a display change is allowed to
/// rewrite.
///
/// `snake_case`, as on [`WhyMembership`] — and this one is the most consequential
/// of the four, because a consumer that wants to *act* on a `Next` has to match
/// on the variant string, so it is the field most likely to be written down
/// somewhere outside this repository.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    Rebuild(String),
    /// Only ever emitted for a [`WhyMembership::Held`] branch: `hitch resolve`
    /// is the only command that operates on a held branch.
    Resolve {
        environment: String,
        branch: String,
    },
    /// Declared nowhere and promotable somewhere.
    Promote {
        branch: String,
        environment: String,
    },
    /// In the build, unwanted, and no reason to be.
    Demote {
        environment: String,
        branch: String,
    },
    /// Deliberate: "nothing to do" and "there is nothing I can suggest" are
    /// different claims, and only the second is worth making.
    None {
        reason: String,
    },
}

/// `hitch why <branch>`.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WhyFeatureExplanation {
    pub branch: String,
    /// One entry per environment, in the snapshot's own (name) order.
    pub environments: Vec<WhyEnvironmentMembership>,
    /// The one-line closing prose of §14.3. `Some` even for a branch promoted
    /// nowhere: "not promoted to any environment" is an answer, and leaving it
    /// `None` would leave the section missing rather than empty.
    pub summary: String,
    /// Only populated for a branch that hitch does not know as a feature — it
    /// may still be a real ref, which is a fact the *command* contributed
    /// because it is the one holding `GitOperations`.
    pub resolves_to_a_ref: Option<bool>,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WhyEnvironmentMembership {
    /// Always present, including in the `Feature` form where the branch is
    /// constant across every entry. Carrying it means the environment form can
    /// name each row without the renderer being handed a parallel list of
    /// branch names that could disagree with the memberships beside it.
    pub branch: String,
    pub environment: String,
    pub membership: WhyMembership,
    pub reason: Option<WhyReason>,
    pub next_action: Option<NextAction>,
}

/// `hitch why <branch> <environment>`.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WhyFeatureInEnvironment {
    pub branch: String,
    pub environment: String,
    /// The declaration, for this environment.
    pub desired_equation: EnvironmentEquation,
    /// The last build, or `None` when there is nothing to describe. Rendered as
    /// no `Actual` section at all rather than as an empty one.
    pub actual_equation: Option<EnvironmentEquation>,
    pub membership: WhyMembership,
    pub reason: Option<WhyReason>,
    /// The environment's verdict and human lock.
    ///
    /// Carried here as well as on `WhyEnvironmentExplanation` because this is
    /// the form a user runs *before* a `hitch promote` — the moment the lock is
    /// the thing that would stop them, and the moment knowing the environment
    /// is behind is what makes a "included" answer mean something. A form that
    /// answered "● Included" while staying silent about both would be answering
    /// a different question from `hitch why <environment>`.
    pub health: EnvironmentHealth,
    pub locked: bool,
    /// §14.2's "What Hitch did", as facts from the record. The sentences are the
    /// renderer's; these are the inputs it renders.
    pub what_hitch_did: Vec<WhatHitchDid>,
    pub next_action: Option<NextAction>,
}

/// One recorded fact about the build, for §14.2's "What Hitch did".
///
/// `snake_case`, as on [`WhyMembership`].
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhatHitchDid {
    /// Took this branch into the build.
    Included { branch: String },
    /// Left it out, and said why — carried so the renderer can name the partner.
    Held {
        branch: String,
        conflicts_with: String,
    },
    /// A recorded resolution was replayed to take it in.
    ReplayedResolution { branch: String, key: String },
}

/// `hitch why <environment>`.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WhyEnvironmentExplanation {
    pub environment: String,
    pub desired_equation: EnvironmentEquation,
    pub actual_equation: Option<EnvironmentEquation>,
    /// Carried whole rather than reduced to words: the words are the renderer's,
    /// and a renderer that re-derived the verdict would be able to disagree with
    /// the snapshot it was handed.
    pub health: EnvironmentHealth,
    pub locked: bool,
    /// One entry per declared branch, in declaration order.
    pub branches: Vec<WhyEnvironmentMembership>,
    pub next_action: Option<NextAction>,
}

/// Build the explanation for an already-resolved subject.
///
/// The one error is a subject that names an environment hitch does not have, or
/// a branch promoted to an environment that does not exist — a judgement about
/// the *question*, not about the repository, which is why it belongs here rather
/// than being swallowed into `Option`.
pub fn build_why(
    snapshot: &RepositoryStateSnapshot,
    subject: &WhySubject,
) -> anyhow::Result<WhyExplanation> {
    match subject {
        WhySubject::Feature(branch) => Ok(WhyExplanation::Feature(why_feature(snapshot, branch))),
        WhySubject::FeatureIn(branch, environment) => {
            let state = environment_state(snapshot, environment)?;
            Ok(WhyExplanation::FeatureInEnvironment(why_feature_in(
                snapshot, branch, state,
            )))
        }
        WhySubject::Environment(environment) => {
            let state = environment_state(snapshot, environment)?;
            Ok(WhyExplanation::Environment(why_environment(
                snapshot, state,
            )))
        }
    }
}

fn environment_state<'a>(
    snapshot: &'a RepositoryStateSnapshot,
    name: &str,
) -> anyhow::Result<&'a crate::core::state::EnvironmentState> {
    match snapshot.environments.iter().find(|e| e.name == name) {
        Some(state) => Ok(state),
        None => {
            let mut known: Vec<&str> = snapshot
                .environments
                .iter()
                .map(|e| e.name.as_str())
                .collect();
            known.sort_unstable();
            bail!(
                "No environment named '{}'.\n\
                 Configured environments: {}\n\n\
                 Run 'hitch status' to see what hitch knows about.",
                name,
                if known.is_empty() {
                    "(none)".to_string()
                } else {
                    known.join(", ")
                }
            )
        }
    }
}

fn why_feature(snapshot: &RepositoryStateSnapshot, branch: &str) -> WhyFeatureExplanation {
    let environments: Vec<WhyEnvironmentMembership> = snapshot
        .environments
        .iter()
        .map(|state| membership_for(snapshot, branch, state))
        .collect();

    // "Has this branch ever been part of an environment?" — and the answer has
    // to include a branch that was *demoted*: the declaration no longer names
    // it, so every membership is `NotDesired`, but the last build still contains
    // it and the environment needs a rebuild. Reading that as "not promoted to
    // any environment" would have the same explanation say both "not promoted
    // anywhere" and "the build still contains it", which is the contradiction
    // this is here to prevent. A demotion is a fact about this branch's
    // history, not an absence.
    let promoted_anywhere = environments.iter().any(|m| {
        m.membership != WhyMembership::NotDesired || m.reason == Some(WhyReason::DemotedSinceBuild)
    });

    let summary = if promoted_anywhere {
        feature_summary(branch, &environments)
    } else {
        format!(
            "{branch} is not promoted to any environment.\n\
             Promoting it would add it to whichever environment you choose: \
             'hitch promote {branch} <environment>'."
        )
    };

    WhyFeatureExplanation {
        branch: branch.to_string(),
        environments,
        summary,
        resolves_to_a_ref: None,
    }
}

/// §14.3's closing sentence: one line, and the truth about the whole set.
///
/// Built by *composing* clauses rather than by branching on which groups are
/// empty. The first version was a `match` on four booleans with a guard, and it
/// shipped `"feature/payments is declared but not built yet in . hitch cannot
/// say what dev contains."` — an arm reached only when `pending` was *empty*
/// interpolated into a sentence that began "not built yet in". A format string
/// cannot tell you about its own holes; a clause list cannot be built with a
/// hole in it, because a clause with an empty list is never constructed.
///
/// So: one clause per state, in decreasing seriousness, each optional.
fn feature_summary(branch: &str, environments: &[WhyEnvironmentMembership]) -> String {
    let names_with = |wanted: WhyMembership| -> Vec<&str> {
        environments
            .iter()
            .filter(|m| m.membership == wanted)
            .map(|m| m.environment.as_str())
            .collect()
    };

    let built: Vec<&str> = environments
        .iter()
        .filter(|m| m.membership.is_in_the_build())
        .map(|m| m.environment.as_str())
        .collect();
    let held = names_with(WhyMembership::Held);
    let pending = names_with(WhyMembership::NeedsRebuild);
    let unknown = names_with(WhyMembership::ActualUnknown);
    let missing = names_with(WhyMembership::Missing);
    let undeclared = names_with(WhyMembership::NotDesired);

    // Each entry is a complete clause or nothing at all. `None` is filtered
    // below; that is the whole mechanism, and it is why there is no way to
    // reach this function's output with an empty substitution in it.
    let mut clauses: Vec<Option<String>> = Vec::new();

    // A build is a clause of its own, *except* when a hold names it as the
    // counterpoint — "built into dev but held in qa" is one clause, not two, and
    // splitting it would read as two unrelated statements. Without the plain
    // form below, the only environments this sentence could ever name were the
    // ones with something wrong with them: a branch cleanly built into dev and
    // merely undeclared in qa would answer "not declared in qa" as though nothing
    // had been built anywhere.
    clauses.push(if held.is_empty() {
        None
    } else {
        Some(if built.is_empty() {
            format!("held in {}", held.join(", "))
        } else {
            format!(
                "built into {} but held in {}",
                built.join(", "),
                held.join(", ")
            )
        })
    });

    clauses.push(if pending.is_empty() {
        None
    } else {
        Some(format!("awaiting a rebuild in {}", pending.join(", ")))
    });

    clauses.push(if unknown.is_empty() {
        None
    } else {
        Some(format!(
            "not something hitch can describe in {}",
            unknown.join(", ")
        ))
    });

    clauses.push(if missing.is_empty() {
        None
    } else {
        Some(format!("missing a branch ref in {}", missing.join(", ")))
    });

    // Not an error — a branch is often declared in one environment and not
    // another, and saying so is part of the answer rather than a caveat to it.
    clauses.push(if undeclared.is_empty() {
        None
    } else {
        Some(format!("not declared in {}", undeclared.join(", ")))
    });

    let mut clauses: Vec<String> = clauses.into_iter().flatten().collect();

    // Nothing qualifies it. Reached only when there is a build to talk about —
    // the caller has already answered the "promoted nowhere" case, so an empty
    // clause list here means every environment that declares this branch has it
    // in the build, and that is the one thing worth saying as a whole sentence
    // rather than as a list of one. (`declared but not built anywhere yet` is
    // *not* reachable: it would need every membership to be `NotDesired`, which
    // is the case the caller intercepts. It used to be here.)
    if clauses.is_empty() {
        return format!(
            "{branch} is already in every environment that declares it ({}).",
            built.join(", ")
        );
    }

    if held.is_empty() && !built.is_empty() {
        clauses.insert(0, format!("built into {}", built.join(", ")));
    }

    format!("{branch} is {}.", clauses.join("; "))
}

fn why_feature_in(
    snapshot: &RepositoryStateSnapshot,
    branch: &str,
    state: &crate::core::state::EnvironmentState,
) -> WhyFeatureInEnvironment {
    let membership = membership_for(snapshot, branch, state);
    let in_base = |name: &str| {
        matches!(
            membership_of_in(snapshot, name, &state.name),
            crate::core::state::ActualMembership::AlreadyInBase
        )
    };

    WhyFeatureInEnvironment {
        branch: branch.to_string(),
        environment: state.name.clone(),
        desired_equation: EnvironmentEquation::from_declaration(state),
        actual_equation: EnvironmentEquation::from_build(state, &in_base),
        membership: membership.membership,
        reason: membership.reason,
        health: state.health.clone(),
        locked: state.locked,
        what_hitch_did: what_hitch_did(state),
        next_action: membership.next_action,
    }
}

fn why_environment(
    snapshot: &RepositoryStateSnapshot,
    state: &crate::core::state::EnvironmentState,
) -> WhyEnvironmentExplanation {
    let in_base = |name: &str| {
        matches!(
            membership_of_in(snapshot, name, &state.name),
            crate::core::state::ActualMembership::AlreadyInBase
        )
    };

    // Declaration order, not name order: this is the composition the environment
    // is *meant* to be, and the order is what a build would consume it in.
    let branches: Vec<WhyEnvironmentMembership> = state
        .desired
        .branches
        .iter()
        .map(|declared| membership_for(snapshot, &declared.name, state))
        .collect();

    WhyEnvironmentExplanation {
        environment: state.name.clone(),
        desired_equation: EnvironmentEquation::from_declaration(state),
        actual_equation: EnvironmentEquation::from_build(state, &in_base),
        health: state.health.clone(),
        locked: state.locked,
        next_action: environment_next_action(state),
        branches,
    }
}

/// §14.2's "What Hitch did", read out of the record.
///
/// A held branch appears here as well as in the equation's `excluded` list, and
/// that is not duplication: the equation makes the arithmetic honest, this says
/// what the build *did*, in the past tense, with the partner named. A reader who
/// sees only the equation knows a branch is missing from the sum; a reader who
/// sees only this knows the build tried and failed.
fn what_hitch_did(state: &crate::core::state::EnvironmentState) -> Vec<WhatHitchDid> {
    let Some(record) = state.actual.actual() else {
        return Vec::new();
    };
    let mut did: Vec<WhatHitchDid> = record
        .included
        .iter()
        .map(|b| WhatHitchDid::Included {
            branch: b.branch.clone(),
        })
        .collect();
    did.extend(record.held.iter().map(|conflict| WhatHitchDid::Held {
        branch: conflict.branch.clone(),
        conflicts_with: conflict.conflicts_with.clone(),
    }));
    did.extend(
        record
            .replayed_resolutions
            .iter()
            .map(|use_| WhatHitchDid::ReplayedResolution {
                branch: use_.branch.clone(),
                key: use_.resolution_key.clone(),
            }),
    );
    did
}

/// The one membership decision, made once for the whole program.
///
/// `classify_from_snapshot` is [`crate::core::status`]'s, so this cannot
/// disagree with the grid; the reason is derived from the same state the
/// membership came from, which is what keeps a `Held` cell from carrying a
/// `ChangedSinceBuild` reason for the same branch.
fn membership_for(
    snapshot: &RepositoryStateSnapshot,
    branch: &str,
    state: &crate::core::state::EnvironmentState,
) -> WhyEnvironmentMembership {
    // The membership comes from `classify_from_snapshot` rather than from the
    // snapshot's own per-environment cell, and the reason is load-bearing: a
    // feature with no membership entry for this environment at all is a *real*
    // state — a branch that exists but is not declared here — and it still has
    // to be classified (`NotDesired`, or `Missing` if its ref is gone). Reading
    // the cell directly would leave that case with nothing to explain.
    let feature = snapshot.features.iter().find(|f| f.name == branch);
    let membership = feature
        .and_then(|f| f.memberships.iter().find(|m| m.environment == state.name))
        .map(|m| classify_from_snapshot(snapshot, &state.name, Some(m)))
        .unwrap_or_else(|| classify_from_snapshot(snapshot, &state.name, None));

    let reason = reason_for(membership, branch, state);
    let next_action = next_action_for(membership, branch, state);

    WhyEnvironmentMembership {
        branch: branch.to_string(),
        environment: state.name.clone(),
        membership: WhyMembership::from(membership),
        reason,
        next_action,
    }
}

/// One reason for one membership, in a fixed order of authority.
///
/// The order is the substance here, not an implementation detail. A reason has
/// to *explain the membership printed above it*, and a reader told "hitch has
/// no build record" under a `! missing` cell has been told something true and
/// useless: the ref itself is gone, and that is the thing to act on. So:
///
/// 1. **The branch itself.** `NoRef` is the most specific fact available and
///    nothing general outranks it — a branch with no ref cannot be held, cannot
///    be in the build, and cannot be read off a record that never mentioned it.
/// 2. **A hold**, named against its partner. Second only because a hold is
///    already fully explained and `NoRef` is a precondition for it.
/// 3. **The environment's own state** — a missing environment branch, or a
///    build hitch cannot describe. Both explain *every* branch in the
///    environment at once, so they come after anything specific to this branch.
/// 4. **Staleness**, most specific first: this branch moved, then it was
///    promoted or demoted, then the base moved under it.
fn reason_for(
    membership: MatrixCell,
    branch: &str,
    state: &crate::core::state::EnvironmentState,
) -> Option<WhyReason> {
    match membership {
        // Checked before the environment's health, deliberately: an environment
        // with no record also reports `LegacyUnknown`, so `NoBuildRecord` would
        // be the *cause* here — but the cause cannot be acted on, whereas the
        // missing ref can. See the doc comment.
        MatrixCell::Missing => return Some(WhyReason::NoRef),
        // Likewise first, and for the sharper reason: `In base` is a *live* fact
        // about reachability, so an explanation about records is explaining
        // something else. The absent `Actual` section above already says the
        // build is undescribable; the reason has to answer this cell.
        MatrixCell::InBase => return Some(WhyReason::AlreadyInBase),
        MatrixCell::Held => {
            if let Some(conflict) = state
                .actual
                .actual()
                .and_then(|record| record.held.iter().find(|c| c.branch == branch))
            {
                return Some(WhyReason::HeldAgainst {
                    conflicts_with: conflict.conflicts_with.clone(),
                    files: conflict.conflicted_files.clone(),
                });
            }
            // A `Held` cell whose conflict the record no longer describes. The
            // membership is a fact and the partner is gone, so there is no
            // honest reason left to give, and `None` is the answer.
            return None;
        }
        _ => {}
    }

    // `NotDesired` is a statement about the *declaration*, and the only thing
    // that can qualify it is the record disagreeing with the declaration. An
    // environment-level reason cannot: "the environment branch does not exist"
    // says nothing about whether this environment declares this branch, and
    // attaching it to a `NotDesired` cell is worse than no reason at all, because
    // it reads as an explanation for the absence and is about something else
    // entirely. (This arm is why the environment-level ones below are reached only
    // by cells that *are* declared here.)
    if membership == MatrixCell::NotDesired {
        return match &state.health {
            EnvironmentHealth::NeedsRebuild { removed, .. }
                if removed.iter().any(|b| b == branch) =>
            {
                Some(WhyReason::DemotedSinceBuild)
            }
            _ => None,
        };
    }

    match &state.health {
        EnvironmentHealth::MissingBranch => return Some(WhyReason::EnvironmentBranchMissing),
        EnvironmentHealth::LegacyUnknown => return Some(WhyReason::NoBuildRecord),
        EnvironmentHealth::NeedsRebuild {
            changed_inputs,
            added,
            removed,
        } => {
            // This branch's own movement first — the most specific explanation
            // available, and it beats a general one.
            if let Some(changed) = changed_inputs.iter().find(|c| c.branch == branch) {
                let (from, to) = changed.short();
                return Some(WhyReason::ChangedSinceBuild { from, to });
            }
            if added.iter().any(|b| b == branch) {
                return Some(WhyReason::PromotedSinceBuild);
            }
            if removed.iter().any(|b| b == branch) {
                return Some(WhyReason::DemotedSinceBuild);
            }
            // Then the base. A branch the record says is included *is* included —
            // that is a fact and nothing here may soften it — but the build it
            // sits in was composed on an older base, and a reader looking at
            // "● Included" with no explanation is being shown a true half of the
            // story. The base is carried in `changed_inputs` under its own name,
            // which is why this is a separate lookup rather than a fallthrough.
            if let Some(base) = changed_inputs.iter().find(|c| c.branch == state.base) {
                let (from, to) = base.short();
                return Some(WhyReason::BaseMoved { from, to });
            }
        }
        _ => {}
    }

    // `NeedsRebuild` reached without any of the four staleness causes naming
    // this branch. That is a real state, not a gap: the environment is behind
    // for a reason this branch's row cannot name, and reporting nothing would
    // read as "nothing is wrong", which is the one thing the cell above it
    // contradicts.
    if matches!(membership, MatrixCell::NeedsRebuild) && has_record_for(state) {
        return Some(WhyReason::EnvironmentBehind);
    }

    None
}

/// Whether the next action is a rebuild, a resolve, or nothing — a *choice*, not
/// a sentence.
fn next_action_for(
    membership: MatrixCell,
    branch: &str,
    state: &crate::core::state::EnvironmentState,
) -> Option<NextAction> {
    match membership {
        // `hitch resolve` is the only command that operates on a held branch.
        MatrixCell::Held => Some(NextAction::Resolve {
            environment: state.name.clone(),
            branch: branch.to_string(),
        }),
        MatrixCell::NeedsRebuild | MatrixCell::InBase => {
            Some(NextAction::Rebuild(state.name.clone()))
        }
        // A missing branch gets no action. `hitch resolve` would fail on a ref
        // that does not resolve, and `hitch rebuild` would hold it again — so
        // the only truthful next step is a human restoring the ref, and there is
        // no hitch command for that.
        MatrixCell::Missing => Some(NextAction::None {
            reason: format!(
                "{branch} has no branch ref, so there is nothing for hitch to build or resolve"
            ),
        }),
        // Two different situations share this cell and they need opposite
        // answers. A branch the last build still *carries* was demoted: the
        // declaration is right and the build is stale, so the action that makes
        // the two agree is a rebuild. A branch no build mentions has never been
        // here, and the action is to declare it. Offering `hitch promote` for the
        // first would have hitch guess at a reversal the user may not want —
        // and it would be acting on the cell while the reason on the same row
        // says the build still has it.
        MatrixCell::NotDesired => {
            let demoted = matches!(
                &state.health,
                EnvironmentHealth::NeedsRebuild { removed, .. } if removed.iter().any(|b| b == branch)
            );
            if demoted {
                Some(NextAction::Rebuild(state.name.clone()))
            } else {
                Some(NextAction::Promote {
                    branch: branch.to_string(),
                    environment: state.name.clone(),
                })
            }
        }
        // Both of these are "as declared", which is the whole point of them.
        MatrixCell::Included | MatrixCell::ActualUnknown => None,
    }
}

fn environment_next_action(state: &crate::core::state::EnvironmentState) -> Option<NextAction> {
    match &state.health {
        EnvironmentHealth::Realised => None,
        // §14.4 shows a `Next` for a partially-realised environment, and the
        // only thing to do about a hold is resolve it. Naming the *first* held
        // branch is a choice: the record lists them in composition order, so
        // this is the earliest one that would have been composed, and resolving
        // it can change the answer for the ones after it.
        EnvironmentHealth::PartiallyRealised { held } => {
            held.first().map(|branch| NextAction::Resolve {
                environment: state.name.clone(),
                branch: branch.clone(),
            })
        }
        EnvironmentHealth::LegacyUnknown => Some(NextAction::None {
            reason: "hitch cannot describe the last build, so it will not guess at one".to_string(),
        }),
        EnvironmentHealth::NeedsRebuild { .. } | EnvironmentHealth::NeverBuilt => {
            Some(NextAction::Rebuild(state.name.clone()))
        }
        EnvironmentHealth::MissingBranch => Some(NextAction::None {
            reason: format!(
                "the '{}' branch does not exist, so there is nothing to rebuild",
                state.name
            ),
        }),
    }
}

/// `ActualMembership` for one pair, via the snapshot's fuller answer.
fn membership_of_in(
    snapshot: &RepositoryStateSnapshot,
    branch: &str,
    environment: &str,
) -> crate::core::state::ActualMembership {
    match snapshot.features.iter().find(|f| f.name == branch) {
        Some(f) => f
            .memberships
            .iter()
            .find(|m| m.environment == environment)
            .map(|m| m.actual)
            .unwrap_or(crate::core::state::ActualMembership::Unknown),
        None => crate::core::state::ActualMembership::Unknown,
    }
}

/// The `2a42d1c → 7c931af` form, re-exported so a caller building a reason by
/// hand and a caller building one from the snapshot spell a change identically.
pub fn changed_input_pair(changed: &ChangedInput) -> (String, String) {
    changed.short()
}
