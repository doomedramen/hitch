//! `hitch why`, and the cell/membership agreement that makes it explainable.
//!
//! # Why these are hand-built snapshots
//!
//! Everything here constructs a `RepositoryStateSnapshot` by hand rather than
//! running a repository, and that is the point rather than a shortcut. The
//! claims under test are claims *over a classification*, and the states needed
//! to reach every arm of `MatrixCell::classify` are awkward to provoke one
//! command at a time — `In base` needs a branch whose tip is reachable from the
//! base *and* a record that is silent about it; `Missing` needs a
//! `--no-rebuild` promotion followed by a `git update-ref -d`. The integration
//! tests in `tests/integration/why_tests.rs` and
//! `tests/integration/state_model_tests.rs` cover the states real history
//! produces; these cover the states *all* of them together, cheaply, and in one
//! place.
//!
//! # The load-bearing test
//!
//! [`the_matrix_cell_and_the_why_membership_never_disagree`] is what makes "a
//! `why` explains the cell it sits next to" a checked property rather than an
//! aspiration. `hitch status` and `hitch why` are separate commands with
//! separate renderers, and nothing in the type system stops someone from
//! classifying a branch one way for the grid and another way for the prose. The
//! only thing that stops it is this.

use chrono::{TimeZone, Utc};

use hitch::core::render::{next_action_command, render_why, NextStep};
use hitch::core::state::{
    ActualComposition, ActualMembership, ApprovalPolicy, ChangedInput, DeclaredBranch,
    DesiredComposition, EnvironmentHealth, EnvironmentState, FeatureMembership, FeatureState,
    RecordActual, RepositoryStateSnapshot,
};
use hitch::core::status::{build_matrix_model, MatrixCell};
use hitch::core::why::{
    build_why, NextAction, WhatHitchDid, WhyEnvironmentExplanation, WhyEnvironmentMembership,
    WhyExplanation, WhyFeatureExplanation, WhyFeatureInEnvironment, WhyMembership, WhyReason,
    WhySubject,
};
use hitch::utils::build_record::{EnvironmentBuildRecord, PinnedBranch, ResolutionUse};
use hitch::utils::prelude::CompatibilityConflict;

// ── Fixture builders ───────────────────────────────────────────────────────

fn sha(seed: char) -> String {
    seed.to_string().repeat(40)
}

/// The short form `ChangedInput::short` produces, so a fixture that asserts on
/// a reason's `from`/`to` spells it the way the renderer will.
fn short(seed: char) -> String {
    sha(seed)[..7].to_string()
}

/// A declaration, in declaration order. Never sorted — see
/// `DesiredComposition::branches`.
fn declared(base: &str, names: &[&str]) -> DesiredComposition {
    DesiredComposition {
        base: base.to_string(),
        base_sha: Some(sha('0')),
        branches: names
            .iter()
            .map(|n| DeclaredBranch {
                name: n.to_string(),
                sha: Some(sha('1')),
                contained_in_base: false,
            })
            .collect(),
    }
}

fn changed(branch: &str, from: char, to: char) -> ChangedInput {
    ChangedInput {
        branch: branch.to_string(),
        previous_sha: Some(sha(from)),
        current_sha: Some(sha(to)),
    }
}

fn conflict(branch: &str, conflicts_with: &str) -> CompatibilityConflict {
    CompatibilityConflict {
        branch: branch.to_string(),
        conflicts_with: conflicts_with.to_string(),
        conflicted_files: vec!["src/payments/api.rs".to_string()],
    }
}

/// A build record.
///
/// `included` is what made it in, `held` is what did not (with its partner), and
/// `replayed` is which recorded resolution was used instead. `held` and
/// `replayed` may name the same branch: that is exactly what a build that
/// resolved a conflict and then failed on a later one looks like.
fn record(
    environment: &str,
    base_name: &str,
    included: &[&str],
    held: &[(&str, &str)],
    replayed: &[(&str, &str)],
) -> RecordActual {
    let built_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let pinned = |names: &[&str]| -> Vec<PinnedBranch> {
        names
            .iter()
            .map(|b| PinnedBranch {
                branch: b.to_string(),
                sha: sha('1'),
            })
            .collect()
    };
    // `desired` is declared, not delivered, so it is the union of the two.
    let mut desired: Vec<&str> = included.to_vec();
    for (branch, _) in held {
        if !desired.contains(branch) {
            desired.push(branch);
        }
    }

    RecordActual {
        tip: Some(sha('2')),
        base_sha: sha('0'),
        included: pinned(included),
        held: held
            .iter()
            .map(|(branch, with)| conflict(branch, with))
            .collect(),
        replayed_resolutions: replayed
            .iter()
            .map(|(branch, key)| ResolutionUse {
                branch: branch.to_string(),
                resolution_key: key.to_string(),
            })
            .collect(),
        built_at,
        record: EnvironmentBuildRecord {
            schema_version: 1,
            environment: environment.to_string(),
            metadata_sha: sha('3'),
            base_name: base_name.to_string(),
            base_sha: sha('0'),
            desired_branches: pinned(&desired),
            included_branches: pinned(included),
            held: held
                .iter()
                .map(|(branch, with)| conflict(branch, with))
                .collect(),
            replayed_resolutions: replayed
                .iter()
                .map(|(branch, key)| ResolutionUse {
                    branch: branch.to_string(),
                    resolution_key: key.to_string(),
                })
                .collect(),
            result_sha: sha('2'),
            built_at,
            hitch_version: "test".to_string(),
        },
    }
}

struct EnvironmentSpec<'a> {
    name: &'a str,
    base: &'a str,
    desired: &'a [&'a str],
    actual: ActualComposition,
    health: EnvironmentHealth,
    locked: bool,
}

fn environment(spec: EnvironmentSpec<'_>) -> EnvironmentState {
    EnvironmentState {
        name: spec.name.to_string(),
        base: spec.base.to_string(),
        desired: declared(spec.base, spec.desired),
        actual: spec.actual,
        health: spec.health,
        locked: spec.locked,
        approval_policy: ApprovalPolicy {
            required: false,
            min_approvals: 0,
            approvers: vec![],
        },
        locked_by: None,
        locked_at: None,
        rebuilt_at: None,
        released_at: None,
    }
}

/// The common shape: a built environment whose record exists.
fn built(
    name: &str,
    base: &str,
    desired: &[&str],
    health: EnvironmentHealth,
    included: &[&str],
    held: &[(&str, &str)],
) -> EnvironmentState {
    environment(EnvironmentSpec {
        name,
        base,
        desired,
        actual: ActualComposition::FromRecord(Box::new(record(name, base, included, held, &[]))),
        health,
        locked: false,
    })
}

/// The common shape: a build hitch cannot describe.
fn legacy(name: &str, base: &str, desired: &[&str]) -> EnvironmentState {
    environment(EnvironmentSpec {
        name,
        base,
        desired,
        actual: ActualComposition::LegacyUnknown,
        health: EnvironmentHealth::LegacyUnknown,
        locked: false,
    })
}

fn feature(name: &str, memberships: &[(&str, bool, ActualMembership)]) -> FeatureState {
    FeatureState {
        name: name.to_string(),
        memberships: memberships
            .iter()
            .map(|(environment, desired, actual)| FeatureMembership {
                environment: environment.to_string(),
                desired: *desired,
                actual: *actual,
            })
            .collect(),
    }
}

fn snapshot(
    environments: Vec<EnvironmentState>,
    features: Vec<FeatureState>,
) -> RepositoryStateSnapshot {
    RepositoryStateSnapshot {
        metadata_sha: Some(sha('3')),
        current_branch: Some("main".to_string()),
        environments,
        features,
        captured_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
    }
}

// ── Owned accessors ────────────────────────────────────────────────────────

/// Owned rather than borrowed out of the `WhyExplanation`, because
/// `build_why(..).unwrap()` is a temporary and a borrow of it does not outlive
/// the statement. Each also asserts the *form*, so a subject that resolved to
/// the wrong shape fails here rather than somewhere downstream.
fn feature_why(snapshot: &RepositoryStateSnapshot, branch: &str) -> WhyFeatureExplanation {
    match build_why(snapshot, &WhySubject::Feature(branch.to_string())).unwrap() {
        WhyExplanation::Feature(e) => e,
        other => panic!("expected the feature form, got {other:?}"),
    }
}

fn feature_in_why(
    snapshot: &RepositoryStateSnapshot,
    branch: &str,
    environment: &str,
) -> WhyFeatureInEnvironment {
    match build_why(
        snapshot,
        &WhySubject::FeatureIn(branch.to_string(), environment.to_string()),
    )
    .unwrap()
    {
        WhyExplanation::FeatureInEnvironment(e) => e,
        other => panic!("expected the feature-in-environment form, got {other:?}"),
    }
}

fn environment_why(
    snapshot: &RepositoryStateSnapshot,
    environment: &str,
) -> WhyEnvironmentExplanation {
    match build_why(snapshot, &WhySubject::Environment(environment.to_string())).unwrap() {
        WhyExplanation::Environment(e) => e,
        other => panic!("expected the environment form, got {other:?}"),
    }
}

fn membership_in<'a>(
    feature: &'a WhyFeatureExplanation,
    environment: &str,
) -> &'a WhyEnvironmentMembership {
    feature
        .environments
        .iter()
        .find(|m| m.environment == environment)
        .unwrap_or_else(|| panic!("no membership for {environment}"))
}

/// Whether a run of consecutive lines matches `block` exactly.
///
/// Line-exact rather than substring for the same reason `render_why` trims its
/// trailing newline: a substring assertion on a block that happens to be last
/// depends on a detail the renderer is allowed to change.
fn has_block(rendered: &str, block: &str) -> bool {
    let lines: Vec<&str> = rendered.lines().collect();
    let expected: Vec<&str> = block.lines().collect();
    lines
        .windows(expected.len())
        .any(|window| window == expected.as_slice())
}

fn has_line(rendered: &str, expected: &str) -> bool {
    rendered.lines().any(|line| line == expected)
}

const BRANCH: &str = "feature/alpha";

/// Seven environments, one branch, seven different cells.
///
/// Deliberately in this order, which is neither sorted nor reverse-sorted: a
/// fixture that happened to be sorted would let a sort slip through unnoticed.
fn seven_states() -> RepositoryStateSnapshot {
    snapshot(
        vec![
            // ● included
            built(
                "dev",
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[BRANCH],
                &[],
            ),
            // ⛔ held
            built(
                "qa",
                "main",
                &[BRANCH],
                EnvironmentHealth::PartiallyRealised {
                    held: vec![BRANCH.to_string()],
                },
                &[],
                &[("feature/alpha", "feature/payments")],
            ),
            // = in base
            built(
                "staging",
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[],
                &[],
            ),
            // ↻ needs rebuild — promoted after the build
            built(
                "prod",
                "main",
                &[BRANCH],
                EnvironmentHealth::NeedsRebuild {
                    changed_inputs: vec![],
                    added: vec![BRANCH.to_string()],
                    removed: vec![],
                },
                &[],
                &[],
            ),
            // ? actual unknown — no record at all
            legacy("legacy", "main", &[BRANCH]),
            // ! missing — declared, and no ref resolves
            legacy("edge", "main", &[BRANCH]),
            // — not desired
            built("shadow", "main", &[], EnvironmentHealth::Realised, &[], &[]),
        ],
        vec![feature(
            BRANCH,
            &[
                ("dev", true, ActualMembership::Included),
                ("qa", true, ActualMembership::Held),
                ("staging", true, ActualMembership::AlreadyInBase),
                ("prod", true, ActualMembership::Unknown),
                ("legacy", true, ActualMembership::Unknown),
                ("edge", true, ActualMembership::Missing),
            ],
        )],
    )
}

// ── The agreement property ─────────────────────────────────────────────────

/// The matrix cell and the `why` membership are the same fact, so a `why`
/// cannot explain a different state than the grid shows beside it.
///
/// The real constraint on the program, in one test: `hitch status` and
/// `hitch why` are two commands with two renderers over one snapshot, and only
/// one of them is named in the spec's exit criteria for legibility. If they can
/// disagree, every "the `why` explains the cell" claim in the program is
/// describing two code paths rather than one.
#[test]
fn the_matrix_cell_and_the_why_membership_never_disagree() {
    let snapshot = seven_states();
    let matrix = build_matrix_model(&snapshot);
    let feature = feature_why(&snapshot, BRANCH);

    assert_eq!(matrix.rows.len(), 1, "one declared feature, one row");
    assert_eq!(
        feature.environments.len(),
        snapshot.environments.len(),
        "every environment gets an answer, declared or not"
    );

    for (column, environment) in matrix.columns.iter().enumerate() {
        let cell = matrix.rows[0].cells[column];
        assert_eq!(
            WhyMembership::from(cell),
            membership_in(&feature, environment).membership,
            "cell for {environment} is {cell:?}"
        );
    }
}

/// The same property from the other direction: for every cell the model
/// produced, the `why` form carries the same membership.
///
/// A one-directional check would pass if `why` read the snapshot and the matrix
/// did not — a real failure mode, because the matrix is built from
/// `feature.memberships` and the `why` from `classify_from_snapshot`, and those
/// are not the same expression.
#[test]
fn every_cell_the_matrix_produces_has_a_why_of_the_same_state() {
    let snapshot = seven_states();
    let matrix = build_matrix_model(&snapshot);

    for row in &matrix.rows {
        let feature = feature_why(&snapshot, &row.feature);
        assert_eq!(feature.environments.len(), matrix.columns.len());
        for (index, environment) in matrix.columns.iter().enumerate() {
            assert_eq!(
                WhyMembership::from(row.cells[index]),
                membership_in(&feature, environment).membership,
                "row {} column {environment}",
                row.feature
            );
        }
    }
}

/// All seven cells, once each, and no others — so a fixture that stops reaching
/// a state fails here rather than letting a whole file of `why` assertions pass
/// vacuously on a narrower world.
#[test]
fn the_fixture_reaches_every_cell_state_exactly_once() {
    let snapshot = seven_states();
    let cells: Vec<MatrixCell> = build_matrix_model(&snapshot).rows[0].cells.clone();

    for wanted in [
        MatrixCell::NotDesired,
        MatrixCell::Included,
        MatrixCell::Held,
        MatrixCell::InBase,
        MatrixCell::NeedsRebuild,
        MatrixCell::ActualUnknown,
        MatrixCell::Missing,
    ] {
        assert_eq!(
            cells.iter().filter(|c| **c == wanted).count(),
            1,
            "exactly one {wanted:?} cell, in {cells:?}"
        );
    }
    assert_eq!(cells.len(), 7, "and no state is left over");

    assert_eq!(
        cells
            .into_iter()
            .map(WhyMembership::from)
            .collect::<Vec<_>>(),
        vec![
            WhyMembership::Included,
            WhyMembership::Held,
            WhyMembership::InBase,
            WhyMembership::NeedsRebuild,
            WhyMembership::ActualUnknown,
            WhyMembership::Missing,
            WhyMembership::NotDesired,
        ]
    );
}

// ── Membership, in the feature form ────────────────────────────────────────

#[test]
fn a_branch_is_answered_for_every_environment_including_the_ones_that_do_not_declare_it() {
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);

    let pairs: Vec<(&str, WhyMembership)> = feature
        .environments
        .iter()
        .map(|m| (m.environment.as_str(), m.membership))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("dev", WhyMembership::Included),
            ("qa", WhyMembership::Held),
            ("staging", WhyMembership::InBase),
            ("prod", WhyMembership::NeedsRebuild),
            ("legacy", WhyMembership::ActualUnknown),
            ("edge", WhyMembership::Missing),
            ("shadow", WhyMembership::NotDesired),
        ]
    );
    assert_eq!(feature.branch, BRANCH);
}

#[test]
fn a_branch_promoted_nowhere_is_answered_rather_than_reported_as_missing() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[],
            EnvironmentHealth::Realised,
            &[],
            &[],
        )],
        vec![],
    );
    let feature = feature_why(&snapshot, "feature/new");

    assert_eq!(feature.environments.len(), 1);
    assert_eq!(
        feature.environments[0].membership,
        WhyMembership::NotDesired
    );
    assert!(feature.summary.contains("not promoted to any environment"));
    assert!(feature
        .summary
        .contains("hitch promote feature/new <environment>"));
}

// ── `reason_for`'s order of authority ──────────────────────────────────────

#[test]
fn a_missing_ref_is_explained_by_the_ref_and_not_by_the_missing_record() {
    // `edge` and `legacy` both have `LegacyUnknown` health, so the
    // environment-level reason available is `NoBuildRecord`. `edge`'s branch is
    // missing a ref; the ref wins, because "hitch has no record" is true and
    // does not tell a reader what to do.
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);

    assert_eq!(
        membership_in(&feature, "edge").reason,
        Some(WhyReason::NoRef)
    );
    assert_eq!(
        membership_in(&feature, "legacy").reason,
        Some(WhyReason::NoBuildRecord)
    );
}

#[test]
fn an_in_base_branch_is_explained_by_reachability_even_with_no_record() {
    // `In base` is knowable with no record at all — it is a live fact about the
    // base. An explanation about records would be explaining something else.
    let snapshot = snapshot(
        vec![legacy("dev", "main", &[BRANCH])],
        vec![feature(
            BRANCH,
            &[("dev", true, ActualMembership::AlreadyInBase)],
        )],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::InBase
    );
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::AlreadyInBase)
    );
}

#[test]
fn a_hold_is_explained_by_its_partner_and_its_files() {
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);

    assert_eq!(
        membership_in(&feature, "qa").reason,
        Some(WhyReason::HeldAgainst {
            conflicts_with: "feature/payments".to_string(),
            files: vec!["src/payments/api.rs".to_string()],
        })
    );
}

/// A `Held` cell whose conflict the record no longer describes gets `None`, not
/// an invented partner. The membership is a fact and the partner is gone, so
/// there is no honest reason left to give — and a wrong one is worse than none.
#[test]
fn a_hold_whose_partner_the_record_forgot_has_no_reason_rather_than_a_wrong_one() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[BRANCH],
            EnvironmentHealth::PartiallyRealised {
                held: vec![BRANCH.to_string()],
            },
            &[],
            // The record says the environment is partially realised, but names
            // no conflict. So does a record whose `held` list was pruned.
            &[],
        )],
        vec![feature(BRANCH, &[("dev", true, ActualMembership::Held)])],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::Held
    );
    assert_eq!(membership_in(&feature, "dev").reason, None);

    // The action is unaffected: a hold is still the one thing `hitch resolve`
    // operates on, partner or no partner.
    assert_eq!(
        membership_in(&feature, "dev").next_action,
        Some(NextAction::Resolve {
            environment: "dev".to_string(),
            branch: BRANCH.to_string(),
        })
    );
}

/// An included branch in a stale build is a *true* statement about the last
/// build, so the cell stays `Included` — and the base's movement is still named,
/// because a reader shown `● Included` with no explanation is being shown half
/// the story.
#[test]
fn an_included_branch_whose_base_moved_is_still_named() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[BRANCH],
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![changed("main", '4', '5')],
                added: vec![],
                removed: vec![],
            },
            &[BRANCH],
            &[],
        )],
        vec![feature(
            BRANCH,
            &[("dev", true, ActualMembership::Included)],
        )],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::Included
    );
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::BaseMoved {
            from: short('4'),
            to: short('5'),
        })
    );
}

/// This branch's own movement outranks the base's. Both are in
/// `changed_inputs` here, so the order is the only thing under test.
#[test]
fn a_branch_that_moved_is_named_before_its_base() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[BRANCH],
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![changed(BRANCH, '6', '7'), changed("main", '4', '5')],
                added: vec![],
                removed: vec![],
            },
            &[],
            &[],
        )],
        vec![feature(BRANCH, &[("dev", true, ActualMembership::Unknown)])],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::ChangedSinceBuild {
            from: short('6'),
            to: short('7'),
        })
    );
}

#[test]
fn a_promotion_after_the_build_is_named_as_a_promotion() {
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "prod").reason,
        Some(WhyReason::PromotedSinceBuild)
    );
}

/// A demoted branch: the cell says `NotDesired` and the *reason* says the
/// build still contains it.
///
/// This is the one place the cell and the reason answer different questions on
/// purpose. The cell is "is this branch declared here?" — no, it is not, and
/// that is a true statement about the declaration today. The reason is "why are
/// you telling me about a branch I did not ask about?" — because the last build
/// contains it and the environment is out of date, so `Not desired` with no
/// explanation would leave a branch sitting in a deployed environment described
/// as merely undeclared.
///
/// The two are not in conflict: `NotDesired` is a property of the declaration
/// and `DemotedSinceBuild` is a property of the build, and the health below
/// says the environment needs a rebuild. A reader who acts on the cell
/// (`hitch promote`) and a reader who acts on the reason (`hitch rebuild`) are
/// both being told something true, so the row offers `Rebuild` rather than
/// `Promote` — promoting it back would be hitch guessing at a reversal.
#[test]
fn a_demoted_branch_is_not_desired_and_still_names_the_build_that_holds_it() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[],
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![],
                added: vec![],
                removed: vec![BRANCH.to_string()],
            },
            // The build still contains it.
            &[BRANCH],
            &[],
        )],
        // Absent from `memberships`, because the declaration no longer names it.
        vec![feature(BRANCH, &[])],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::NotDesired,
        "the declaration is the fact the cell reports"
    );
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::DemotedSinceBuild),
        "and the reason names the build the cell does not describe"
    );
    assert_eq!(
        membership_in(&feature, "dev").next_action,
        Some(NextAction::Rebuild("dev".to_string())),
        "a rebuild is what makes the cell and the build agree"
    );

    // Rendered, the pair reads as one explanation rather than as a
    // contradiction — and specifically it does *not* read as "not promoted to
    // any environment", which is the sentence a demoted branch used to get,
    // contradicting the reason printed directly above it.
    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Feature(BRANCH.to_string())).unwrap());
    assert!(has_line(&rendered, "  dev  — Not desired"), "{rendered}");
    assert!(
        has_line(
            &rendered,
            "       it was demoted after the last build ran, so the build still contains it"
        ),
        "{rendered}"
    );
    assert!(
        !rendered.contains("not promoted to any environment"),
        "one answer cannot say both:\n{rendered}"
    );
    assert!(
        feature.summary.contains("not declared in dev"),
        "a demoted branch is not declared, and that is the clause it gets: {}",
        feature.summary
    );
}

/// `EnvironmentBehind`: a real state, not a gap. The environment is behind for
/// a reason that is not about this branch, and reporting nothing would read as
/// "nothing is wrong" directly under a cell that says otherwise.
#[test]
fn an_environment_behind_for_another_reason_still_explains_itself() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[BRANCH],
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![changed("feature/unrelated", '8', '9')],
                added: vec![],
                removed: vec![],
            },
            &[],
            &[],
        )],
        vec![feature(BRANCH, &[("dev", true, ActualMembership::Unknown)])],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::NeedsRebuild
    );
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::EnvironmentBehind)
    );
}

#[test]
fn an_environment_missing_its_own_branch_says_so() {
    let snapshot = snapshot(
        vec![environment(EnvironmentSpec {
            name: "dev",
            base: "main",
            desired: &[BRANCH],
            actual: ActualComposition::Unreadable {
                reason: "environment branch missing".to_string(),
            },
            health: EnvironmentHealth::MissingBranch,
            locked: false,
        })],
        vec![feature(BRANCH, &[("dev", true, ActualMembership::Unknown)])],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").reason,
        Some(WhyReason::EnvironmentBranchMissing)
    );
}

#[test]
fn a_realised_environment_explains_nothing_because_nothing_is_wrong() {
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(membership_in(&feature, "dev").reason, None);
    assert_eq!(membership_in(&feature, "dev").next_action, None);
}

// ── Next actions ───────────────────────────────────────────────────────────

/// The mapping is one cell, one action, and a wrong entry here is a wrong
/// instruction printed to a user.
#[test]
fn each_cell_gets_the_action_that_can_actually_advance_it() {
    let snapshot = seven_states();
    let feature = feature_why(&snapshot, BRANCH);

    let cases = [
        // Included and realised: as declared, so nothing owed.
        ("dev", None),
        // A hold is the one thing only `hitch resolve` can move.
        (
            "qa",
            Some(NextAction::Resolve {
                environment: "qa".to_string(),
                branch: BRANCH.to_string(),
            }),
        ),
        // Declared but not in the build yet.
        ("prod", Some(NextAction::Rebuild("prod".to_string()))),
        // No ref: `hitch resolve` would fail on it and `hitch rebuild` would
        // hold it again, so advice rather than a command that cannot work.
        (
            "edge",
            Some(NextAction::None {
                reason: format!(
                    "{BRANCH} has no branch ref, so there is nothing for hitch to build or resolve"
                ),
            }),
        ),
        // A build hitch cannot describe: nothing to recommend, and specifically
        // not a rebuild that would overwrite a build it cannot explain.
        ("legacy", None),
        // Not declared: the only useful next step is to declare it.
        (
            "shadow",
            Some(NextAction::Promote {
                branch: BRANCH.to_string(),
                environment: "shadow".to_string(),
            }),
        ),
    ];

    for (environment, expected) in cases {
        assert_eq!(
            membership_in(&feature, environment).next_action,
            expected,
            "action for {environment}"
        );
    }
}

#[test]
fn every_next_action_renders_to_the_command_that_does_it() {
    // The `hitch resolve` argument order is the load-bearing detail here: it is
    // `<environment> --branch <branch>`, and getting it backwards produces a
    // command that errors when pasted.
    assert_eq!(
        next_action_command(&NextAction::Rebuild("dev".to_string())),
        NextStep::Command("hitch rebuild dev".to_string())
    );
    assert_eq!(
        next_action_command(&NextAction::Resolve {
            environment: "qa".to_string(),
            branch: BRANCH.to_string(),
        }),
        NextStep::Command(format!("hitch resolve qa --branch {BRANCH}"))
    );
    assert_eq!(
        next_action_command(&NextAction::Promote {
            branch: BRANCH.to_string(),
            environment: "qa".to_string(),
        }),
        NextStep::Command(format!("hitch promote {BRANCH} qa"))
    );
    assert_eq!(
        next_action_command(&NextAction::Demote {
            environment: "qa".to_string(),
            branch: BRANCH.to_string(),
        }),
        NextStep::Command(format!("hitch demote {BRANCH} qa"))
    );
    // Advice, never a command: "there is nothing to run" and "here is a command"
    // are different claims, and rendering the second as the first would send a
    // user looking for a command that does not exist.
    assert_eq!(
        next_action_command(&NextAction::None {
            reason: "nothing to run here".to_string(),
        }),
        NextStep::Advice("nothing to run here".to_string())
    );
}

#[test]
fn an_environment_suggests_resolve_for_its_first_held_branch() {
    // The record lists held branches in composition order, so the first is the
    // earliest a build would have reached, and resolving it can change the
    // answer for the ones after it.
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[BRANCH, "feature/second", "feature/third"],
            EnvironmentHealth::PartiallyRealised {
                held: vec!["feature/second".to_string(), "feature/third".to_string()],
            },
            &[],
            &[
                ("feature/second", BRANCH),
                ("feature/third", "feature/second"),
            ],
        )],
        vec![],
    );

    let environment = environment_why(&snapshot, "dev");
    assert_eq!(
        environment.next_action,
        Some(NextAction::Resolve {
            environment: "dev".to_string(),
            branch: "feature/second".to_string(),
        })
    );
}

#[test]
fn an_environment_promises_nothing_it_cannot_deliver() {
    // Every health variant, through the environment form's own next action. The
    // interesting column is the last: `LegacyUnknown` and `MissingBranch` both
    // produce advice, not a command, because neither has a command that would
    // work.
    let cases: Vec<(EnvironmentHealth, Option<NextAction>)> = vec![
        (EnvironmentHealth::Realised, None),
        (
            EnvironmentHealth::PartiallyRealised {
                held: vec![BRANCH.to_string()],
            },
            Some(NextAction::Resolve {
                environment: "dev".to_string(),
                branch: BRANCH.to_string(),
            }),
        ),
        (
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![],
                added: vec![],
                removed: vec![],
            },
            Some(NextAction::Rebuild("dev".to_string())),
        ),
        (
            EnvironmentHealth::NeverBuilt,
            Some(NextAction::Rebuild("dev".to_string())),
        ),
        (
            EnvironmentHealth::LegacyUnknown,
            Some(NextAction::None {
                reason: "hitch cannot describe the last build, so it will not guess at one"
                    .to_string(),
            }),
        ),
        (
            EnvironmentHealth::MissingBranch,
            Some(NextAction::None {
                reason: "the 'dev' branch does not exist, so there is nothing to rebuild"
                    .to_string(),
            }),
        ),
    ];

    for (health, expected) in cases {
        let snapshot = snapshot(
            vec![built("dev", "main", &[BRANCH], health.clone(), &[], &[])],
            vec![],
        );
        let explanation = environment_why(&snapshot, "dev");
        assert_eq!(
            explanation.next_action,
            expected,
            "next action for {}",
            health.label()
        );
    }
}

// ── The summary sentence ───────────────────────────────────────────────────

/// The regression for a bug that shipped: a `match` on four booleans with a
/// guard reached an arm only when `pending` was *empty*, interpolating an empty
/// list into a sentence that began "not built yet in", producing
/// `"feature/payments is declared but not built yet in . hitch cannot say what
/// dev contains."`.
///
/// The clause-list rewrite makes it unrepresentable — a clause with an empty list
/// is never constructed — and this is what stops a later rewrite putting it back.
#[test]
fn a_summary_is_composed_of_clauses_and_never_contains_an_empty_hole() {
    // One environment per membership kind, each named so the join is checkable.
    // The names are chosen so that a wrong join (`.join(", ")` on a list of the
    // wrong group) produces a different string, not the same one.
    /// `(memberships, joined names, names that must NOT appear)`
    type SummaryCase = (
        Vec<(&'static str, WhyMembership)>,
        Vec<&'static str>,
        Vec<&'static str>,
    );
    let cases: Vec<SummaryCase> = vec![
        // Promoted nowhere. This one does not reach the clause list at all —
        // "not declared in dev" would be a true and useless sentence, so
        // `why_feature` answers it before the composer runs. The case is here
        // because it is the arm most likely to be lost to a refactor that
        // generalises the composer, and because the two-sentence shape (a
        // statement, then the command that changes it) is the one case the
        // composer's "one line" contract does not cover.
        (
            vec![("dev", WhyMembership::NotDesired)],
            vec![
                "feature/alpha is not promoted to any environment.",
                "'hitch promote feature/alpha <environment>'",
            ],
            vec!["in .", ", .", "not declared in"],
        ),
        (
            vec![
                ("dev", WhyMembership::Included),
                ("qa", WhyMembership::Held),
            ],
            vec!["feature/alpha is built into dev but held in qa."],
            vec!["in .", ", ."],
        ),
        // Held with nothing built: the "held in" clause must not have acquired a
        // "built into" half from the other arm of the same conditional.
        (
            vec![("qa", WhyMembership::Held)],
            vec!["feature/alpha is held in qa."],
            vec!["built into", "but held"],
        ),
        (
            vec![
                ("dev", WhyMembership::NeedsRebuild),
                ("qa", WhyMembership::ActualUnknown),
            ],
            vec![
                "awaiting a rebuild in dev",
                "not something hitch can describe in qa",
            ],
            vec!["in .", ", ."],
        ),
        (
            vec![("dev", WhyMembership::Missing)],
            vec!["feature/alpha is missing a branch ref in dev."],
            vec!["in ."],
        ),
        (
            vec![
                ("dev", WhyMembership::Included),
                ("qa", WhyMembership::NotDesired),
            ],
            vec!["built into dev", "not declared in qa"],
            vec!["in .", ", ."],
        ),
        // Every group at once: four clauses, joined in decreasing seriousness,
        // and each naming only its own environments.
        (
            vec![
                ("dev", WhyMembership::Included),
                ("qa", WhyMembership::Held),
                ("prod", WhyMembership::NeedsRebuild),
                ("stage", WhyMembership::ActualUnknown),
                ("edge", WhyMembership::Missing),
                ("shadow", WhyMembership::NotDesired),
            ],
            vec![
                "built into dev but held in qa",
                "awaiting a rebuild in prod",
                "not something hitch can describe in stage",
                "missing a branch ref in edge",
                "not declared in shadow",
            ],
            vec!["in .", ", ."],
        ),
    ];

    for (wanted, contains, absent) in cases {
        let memberships: Vec<WhyEnvironmentMembership> = wanted
            .iter()
            .map(|(environment, membership)| WhyEnvironmentMembership {
                branch: BRANCH.to_string(),
                environment: environment.to_string(),
                membership: *membership,
                reason: None,
                next_action: None,
            })
            .collect();

        // The clause composer is private, so the same list is rebuilt through
        // `build_why` — which means the assertion is also that `build_why` maps
        // the snapshot onto these memberships rather than onto its own idea of
        // them. The fixture is therefore checked against itself first.
        let snapshot = snapshot_for(&wanted);
        let feature = feature_why(&snapshot, BRANCH);
        let got: Vec<(&str, WhyMembership)> = feature
            .environments
            .iter()
            .map(|m| (m.environment.as_str(), m.membership))
            .collect();
        assert_eq!(
            got, wanted,
            "fixture drift: the snapshot did not classify as asked"
        );

        let summary = feature.summary.clone();
        for needle in contains {
            assert!(
                summary.contains(needle),
                "summary {summary:?} should contain {needle:?}"
            );
        }
        for needle in absent {
            assert!(
                !summary.contains(needle),
                "summary {summary:?} should not contain {needle:?}"
            );
        }
        // The general claim, stated once for every case: the summary is about
        // this branch, and it is prose rather than a fragment. The promoted-
        // nowhere case is two sentences, so the test is on the first and last
        // lines rather than on the whole string.
        let first = summary.lines().next().unwrap();
        let last = summary.lines().last().unwrap();
        assert!(
            first.starts_with(BRANCH),
            "summary {summary:?} names the branch"
        );
        assert!(
            last.ends_with('.'),
            "summary {summary:?} ends as a sentence"
        );
        // Every environment name the summary mentions is a real one, and at most
        // once — a duplicated or dropped name shows up as a count mismatch.
        for (environment, _) in &wanted {
            let count = summary.matches(environment).count();
            assert!(
                count <= 1,
                "summary {summary:?} mentions {environment} {count} times"
            );
        }

        // And the hand-built list gives the same sentence, which is the claim
        // that the composer reads memberships and nothing else.
        assert_eq!(
            summary,
            summarize(memberships),
            "the hand-built list and the snapshot disagree"
        );
    }
}

/// A snapshot whose classification is exactly `wanted`.
///
/// A closure per membership rather than a derived environment, because the
/// states are not independent: `Missing` needs `LegacyUnknown` health (the
/// snapshot has no record to contradict the missing ref), `Held` needs the
/// record to name the hold, and `Included` needs the record to name the
/// inclusion. Deriving one from the other would encode a guess about which
/// combinations are legal.
fn snapshot_for(wanted: &[(&str, WhyMembership)]) -> RepositoryStateSnapshot {
    let environments: Vec<EnvironmentState> = wanted
        .iter()
        .map(|(name, membership)| match membership {
            WhyMembership::Included => built(
                name,
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[BRANCH],
                &[],
            ),
            WhyMembership::Held => built(
                name,
                "main",
                &[BRANCH],
                EnvironmentHealth::PartiallyRealised {
                    held: vec![BRANCH.to_string()],
                },
                &[],
                &[(BRANCH, "feature/other")],
            ),
            WhyMembership::InBase => built(
                name,
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[],
                &[],
            ),
            WhyMembership::NeedsRebuild => built(
                name,
                "main",
                &[BRANCH],
                EnvironmentHealth::NeedsRebuild {
                    changed_inputs: vec![changed("feature/other", '8', '9')],
                    added: vec![],
                    removed: vec![],
                },
                &[],
                &[],
            ),
            // A declared branch with no ref, in an environment hitch cannot
            // describe: the only real combination that yields `Missing`, because
            // a recorded build would have named the branch either way.
            WhyMembership::ActualUnknown | WhyMembership::Missing => {
                legacy(name, "main", &[BRANCH])
            }
            WhyMembership::NotDesired => {
                built(name, "main", &[], EnvironmentHealth::Realised, &[], &[])
            }
        })
        .collect();

    let memberships: Vec<(&str, bool, ActualMembership)> = wanted
        .iter()
        .filter(|(_, membership)| *membership != WhyMembership::NotDesired)
        .map(|(name, membership)| {
            let actual = match membership {
                WhyMembership::Included => ActualMembership::Included,
                WhyMembership::Held => ActualMembership::Held,
                WhyMembership::InBase => ActualMembership::AlreadyInBase,
                // A declared branch with no ref anywhere, in an environment hitch
                // cannot describe. The membership is the *snapshot's* own reading
                // of the refs, not something the declaration implies, so it has
                // to be set here rather than derived from the environment.
                WhyMembership::Missing => ActualMembership::Missing,
                // `LegacyUnknown` with a branch no build mentions.
                WhyMembership::NeedsRebuild | WhyMembership::ActualUnknown => {
                    ActualMembership::Unknown
                }
                WhyMembership::NotDesired => ActualMembership::Unknown,
            };
            (*name, true, actual)
        })
        .collect();

    snapshot(environments, vec![feature(BRANCH, &memberships)])
}

/// The clause composer, transcribed.
///
/// This is the one piece of private logic in `core::why` that a test
/// transcribes, and it is transcribed because the alternative — testing only
/// through `build_why` — cannot tell "the composer is right" from "the
/// classifier arranged the inputs so the composer's bugs did not show". Asserting
/// the two agree catches a change to either that the other has not absorbed.
///
/// The one arm transcribed here that is *not* the composer's is
/// `why_feature`'s promoted-nowhere sentence, which runs before the composer is
/// called. It is folded in because the test asserts the whole summary, and
/// leaving it out would make every promoted-nowhere case disagree on a
/// difference that has nothing to do with the clauses.
///
/// If `feature_summary` grows an arm, this goes stale and the test fails, which
/// is the intent: the transcription is the specification of the sentences.
fn summarize(environments: Vec<WhyEnvironmentMembership>) -> String {
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

    // With the demotion exception, which the transcribed memberships do not carry
    // (a `NotDesired` row with a `DemotedSinceBuild` reason is what a demotion
    // looks like, and the fixture builder does not build one — the demoted case
    // has its own test, which asserts the rendered sentences directly).
    if !environments
        .iter()
        .any(|m| m.membership != WhyMembership::NotDesired)
    {
        return format!(
            "{BRANCH} is not promoted to any environment.\n\
             Promoting it would add it to whichever environment you choose: \
             'hitch promote {BRANCH} <environment>'."
        );
    }

    let mut clauses: Vec<Option<String>> = Vec::new();
    clauses.push(if held.is_empty() {
        None
    } else if built.is_empty() {
        Some(format!("held in {}", held.join(", ")))
    } else {
        Some(format!(
            "built into {} but held in {}",
            built.join(", "),
            held.join(", ")
        ))
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
    clauses.push(if undeclared.is_empty() {
        None
    } else {
        Some(format!("not declared in {}", undeclared.join(", ")))
    });

    let mut clauses: Vec<String> = clauses.into_iter().flatten().collect();
    if clauses.is_empty() {
        return format!(
            "{BRANCH} is already in every environment that declares it ({}).",
            built.join(", ")
        );
    }
    if held.is_empty() && !built.is_empty() {
        clauses.insert(0, format!("built into {}", built.join(", ")));
    }
    format!("{BRANCH} is {}.", clauses.join("; "))
}

/// The one case with nothing left to qualify, which is a different sentence and
/// not an empty one.
#[test]
fn a_branch_in_every_environment_it_declares_has_nothing_to_qualify() {
    let snapshot = snapshot(
        vec![
            built(
                "dev",
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[BRANCH],
                &[],
            ),
            built(
                "qa",
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[BRANCH],
                &[],
            ),
        ],
        vec![feature(
            BRANCH,
            &[
                ("dev", true, ActualMembership::Included),
                ("qa", true, ActualMembership::Included),
            ],
        )],
    );

    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        feature.summary,
        format!("{BRANCH} is already in every environment that declares it (dev, qa).")
    );
}

/// `In base` counts as in the build. It is the one "in" cell that is not
/// "included", and treating it as pending would put a `rebuild` on an environment
/// that already contains the branch.
#[test]
fn an_in_base_branch_counts_as_built_in_the_summary() {
    let snapshot = snapshot_for(&[("dev", WhyMembership::InBase)]);
    let feature = feature_why(&snapshot, BRANCH);
    assert_eq!(
        membership_in(&feature, "dev").membership,
        WhyMembership::InBase
    );
    assert_eq!(
        feature.summary,
        format!("{BRANCH} is already in every environment that declares it (dev).")
    );
}

// ── Equations ──────────────────────────────────────────────────────────────

/// The `Actual` equation describes *that build*, so it names the base the build
/// consumed. Substituting today's base would describe a composition nobody
/// performed.
#[test]
fn the_actual_equation_names_the_base_the_build_actually_consumed() {
    let snapshot = snapshot(
        vec![environment(EnvironmentSpec {
            name: "dev",
            // The declaration has been re-based since the build.
            base: "release/1.0",
            desired: &[BRANCH],
            actual: ActualComposition::FromRecord(Box::new(record(
                "dev",
                "main",
                &[BRANCH],
                &[],
                &[],
            ))),
            health: EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![changed("main", '4', '5')],
                added: vec![],
                removed: vec![],
            },
            locked: false,
        })],
        vec![feature(
            BRANCH,
            &[("dev", true, ActualMembership::Included)],
        )],
    );

    let form = feature_in_why(&snapshot, BRANCH, "dev");
    assert_eq!(form.desired_equation.base, "release/1.0");
    assert_eq!(
        form.actual_equation.as_ref().unwrap().base,
        "main",
        "the equation describes the build, so it names the base the build used"
    );
}

#[test]
fn a_legacy_environment_has_no_actual_equation_rather_than_an_empty_one() {
    let snapshot = snapshot(
        vec![legacy("dev", "main", &[BRANCH])],
        vec![feature(BRANCH, &[("dev", true, ActualMembership::Unknown)])],
    );

    let form = feature_in_why(&snapshot, BRANCH, "dev");
    // Present when there is a declaration, absent when there is no build.
    assert!(form.actual_equation.is_none());
    assert_eq!(form.desired_equation.terms.len(), 1);

    let environment = environment_why(&snapshot, "dev");
    assert!(environment.actual_equation.is_none());
}

/// Declaration order is composition order. An environment that sorts its
/// branches describes a different build.
#[test]
fn the_environment_form_lists_branches_in_declaration_order_and_never_sorted() {
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &["feature/zebra", "feature/alpha", "feature/middle"],
            EnvironmentHealth::Realised,
            &[],
            &[],
        )],
        vec![],
    );

    let environment = environment_why(&snapshot, "dev");
    let branches: Vec<&str> = environment
        .branches
        .iter()
        .map(|b| b.branch.as_str())
        .collect();
    assert_eq!(
        branches,
        vec!["feature/zebra", "feature/alpha", "feature/middle"]
    );

    // The equation agrees, because both come from `from_declaration`.
    let terms: Vec<&str> = environment
        .desired_equation
        .terms
        .iter()
        .map(|t| t.branch.as_str())
        .collect();
    assert_eq!(terms, branches);
}

#[test]
fn what_hitch_did_names_every_recorded_fact_including_the_partner_and_the_key() {
    let snapshot = snapshot(
        vec![environment(EnvironmentSpec {
            name: "dev",
            base: "main",
            desired: &[BRANCH, "feature/payments", "feature/third"],
            actual: ActualComposition::FromRecord(Box::new(record(
                "dev",
                "main",
                &[BRANCH, "feature/payments"],
                &[("feature/third", "feature/payments")],
                // Replayed *and* held: a resolution that let the build proceed
                // past a conflict the record still records as held. Both facts
                // are carried, because both are things the build observed.
                &[("feature/third", "abc123")],
            ))),
            health: EnvironmentHealth::Realised,
            locked: false,
        })],
        vec![],
    );

    let form = feature_in_why(&snapshot, BRANCH, "dev");
    assert_eq!(
        form.what_hitch_did,
        vec![
            WhatHitchDid::Included {
                branch: BRANCH.to_string()
            },
            WhatHitchDid::Included {
                branch: "feature/payments".to_string()
            },
            WhatHitchDid::Held {
                branch: "feature/third".to_string(),
                conflicts_with: "feature/payments".to_string(),
            },
            WhatHitchDid::ReplayedResolution {
                branch: "feature/third".to_string(),
                key: "abc123".to_string(),
            },
        ]
    );
}

#[test]
fn what_hitch_did_is_empty_rather_than_absent_when_there_is_no_record() {
    let snapshot = snapshot(vec![legacy("dev", "main", &[BRANCH])], vec![]);
    let form = feature_in_why(&snapshot, BRANCH, "dev");
    // Absent, not "hitch did nothing" — a build hitch cannot describe is not a
    // build that did nothing.
    assert!(form.what_hitch_did.is_empty());
}

// ── Errors ─────────────────────────────────────────────────────────────────

#[test]
fn an_unknown_environment_is_an_error_naming_the_ones_that_exist() {
    let snapshot = seven_states();
    let error = build_why(&snapshot, &WhySubject::Environment("nope".to_string()))
        .expect_err("an environment that is not there is an error");
    let message = error.to_string();

    assert!(message.contains("No environment named 'nope'"), "{message}");
    // Sorted, because the list is a set and its order must not be a second thing
    // that can differ between two runs over the same repository.
    assert!(
        message.contains("dev, edge, legacy, prod, qa, shadow, staging"),
        "{message}"
    );
    assert!(message.contains("hitch status"), "{message}");

    // The two-argument form resolves the environment the same way, so it shares
    // the error.
    let error = build_why(
        &snapshot,
        &WhySubject::FeatureIn(BRANCH.to_string(), "nope".to_string()),
    )
    .expect_err("same error for the two-argument form");
    assert!(error.to_string().contains("No environment named 'nope'"));
}

#[test]
fn an_unknown_environment_in_a_repository_with_none_says_none() {
    let snapshot = snapshot(vec![], vec![]);
    let error = build_why(&snapshot, &WhySubject::Environment("dev".to_string()))
        .expect_err("no environments at all is still an error");
    assert!(error.to_string().contains("(none)"), "{}", error);
}

// ── Rendering, through the one word-chooser ────────────────────────────────

#[test]
fn the_feature_form_renders_the_membership_the_reason_and_the_summary() {
    let snapshot = seven_states();
    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Feature(BRANCH.to_string())).unwrap());

    // The name is the headline, not a heading with decoration around it.
    assert!(rendered.starts_with(&format!("{BRANCH}\n")), "{rendered}");

    // One line per environment, the name padded to the widest so the memberships
    // line up. `staging` is the widest at seven characters.
    assert!(has_line(&rendered, "  dev      ● Included"), "{rendered}");
    assert!(has_line(&rendered, "  qa       ⛔ Held"), "{rendered}");
    assert!(has_line(&rendered, "  staging  = In base"), "{rendered}");
    assert!(
        has_line(&rendered, "  prod     ↻ Needs rebuild"),
        "{rendered}"
    );
    assert!(
        has_line(&rendered, "  legacy   ? Actual unknown"),
        "{rendered}"
    );
    assert!(has_line(&rendered, "  edge     ! Missing"), "{rendered}");
    assert!(
        has_line(&rendered, "  shadow   — Not desired"),
        "{rendered}"
    );

    // Each reason hangs off its own environment, indented to the same column
    // the membership labels start at, so the eye pairs them without counting
    // spaces. `staging` is the widest at seven characters, and the label starts
    // at `2 + 7 + 2`, so the reason sits at eleven.
    assert!(
        has_line(&rendered, "           it conflicts with feature/payments"),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            "           no branch by that name exists, locally or on origin"
        ),
        "{rendered}"
    );

    // A membership with no reason has no continuation line, so a reader cannot
    // mistake a missing reason for a dropped one. `dev` is the only realised
    // environment in the fixture and therefore the only un-explained row.
    let lines: Vec<&str> = rendered.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if *line == "  dev      ● Included" {
            let next = lines.get(index + 1);
            assert!(
                next.is_none_or(|l| l.starts_with("  ")),
                "an unexplained membership has a continuation line: {next:?}"
            );
        }
    }

    // The summary closes it, and it is a sentence about the branch.
    let last = rendered.lines().last().unwrap();
    assert!(last.starts_with(BRANCH), "{rendered}");
    assert!(last.ends_with('.'), "{rendered}");
}

/// A hold appears three times on purpose: the membership says the state, the
/// reason says against what, and the summary says the same thing about the whole
/// set. None of the three is redundant with the others — one is a cell, one is an
/// explanation, one is a verdict.
#[test]
fn a_held_branch_is_named_three_times_in_three_different_roles() {
    let snapshot = snapshot(
        vec![
            built(
                "dev",
                "main",
                &[BRANCH],
                EnvironmentHealth::Realised,
                &[BRANCH],
                &[],
            ),
            built(
                "qa",
                "main",
                &[BRANCH],
                EnvironmentHealth::PartiallyRealised {
                    held: vec![BRANCH.to_string()],
                },
                &[],
                &[(BRANCH, "feature/payments")],
            ),
        ],
        vec![feature(
            BRANCH,
            &[
                ("dev", true, ActualMembership::Included),
                ("qa", true, ActualMembership::Held),
            ],
        )],
    );

    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Feature(BRANCH.to_string())).unwrap());
    assert!(has_line(&rendered, "  dev  ● Included"), "{rendered}");
    assert!(has_line(&rendered, "  qa   ⛔ Held"), "{rendered}");
    assert!(
        has_line(&rendered, "       it conflicts with feature/payments"),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            &format!("{BRANCH} is built into dev but held in qa.")
        ),
        "{rendered}"
    );
}

#[test]
fn the_feature_form_says_so_plainly_when_there_are_no_environments_at_all() {
    let snapshot = snapshot(vec![], vec![]);
    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Feature("x".to_string())).unwrap());
    assert!(
        has_line(&rendered, "  Not promoted to any environment."),
        "{rendered}"
    );
    assert!(
        rendered.contains("hitch promote x <environment>"),
        "{rendered}"
    );
}

#[test]
fn the_feature_in_environment_form_renders_desired_actual_verdict_lock_and_next() {
    let snapshot = snapshot(
        vec![environment(EnvironmentSpec {
            name: "dev",
            base: "main",
            desired: &[BRANCH, "feature/payments"],
            actual: ActualComposition::FromRecord(Box::new(record(
                "dev",
                "main",
                &[BRANCH],
                &[("feature/payments", BRANCH)],
                &[],
            ))),
            health: EnvironmentHealth::PartiallyRealised {
                held: vec!["feature/payments".to_string()],
            },
            locked: true,
        })],
        vec![feature(
            BRANCH,
            &[("dev", true, ActualMembership::Included)],
        )],
    );

    let rendered = render_why(
        &build_why(
            &snapshot,
            &WhySubject::FeatureIn(BRANCH.to_string(), "dev".to_string()),
        )
        .unwrap(),
    );

    assert!(
        rendered.starts_with(&format!("{BRANCH} → dev\n")),
        "{rendered}"
    );
    // The same equation renderer `hitch tree` and the plans use, which is what
    // stops `hitch why dev` describing the environment differently from a plan
    // about it.
    assert!(
        has_block(
            &rendered,
            "Desired\n  dev = main + feature/alpha + feature/payments\n\nActual\n  dev = main + feature/alpha\n    feature/payments ⛔ held — conflicts with feature/alpha\n        src/payments/api.rs"
        ),
        "{rendered}"
    );
    // The verdict, as a sentence about the environment.
    assert!(
        has_line(
            &rendered,
            "dev is partially realised: feature/payments held out of the build."
        ),
        "{rendered}"
    );
    // The lock, and what it will refuse.
    assert!(
        has_line(
            &rendered,
            "dev is locked, so it will refuse a promote until 'hitch unlock dev'."
        ),
        "{rendered}"
    );
    // What the build actually did, from the record and nowhere else.
    assert!(has_line(&rendered, "What Hitch did"), "{rendered}");
    assert!(
        has_line(&rendered, &format!("  Included {BRANCH} in the build")),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            "  Excluded feature/payments — it conflicts with feature/alpha"
        ),
        "{rendered}"
    );
}

#[test]
fn the_environment_form_renders_desired_actual_branches_verdict_and_next() {
    let snapshot = seven_states();
    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Environment("qa".to_string())).unwrap());

    assert!(rendered.starts_with("qa\n"), "{rendered}");
    assert!(has_line(&rendered, "Desired"), "{rendered}");
    assert!(
        has_line(&rendered, &format!("  qa = main + {BRANCH}")),
        "{rendered}"
    );
    assert!(has_line(&rendered, "Actual"), "{rendered}");
    assert!(has_line(&rendered, "  qa = main"), "{rendered}");
    assert!(has_line(&rendered, "Branches"), "{rendered}");
    assert!(
        has_line(&rendered, &format!("  {BRANCH}  ⛔ Held")),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            "                 it conflicts with feature/payments"
        ),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            "qa is partially realised: feature/alpha held out of the build."
        ),
        "{rendered}"
    );
    assert!(has_line(&rendered, "Next"), "{rendered}");
    assert!(
        has_line(&rendered, &format!("  hitch resolve qa --branch {BRANCH}")),
        "{rendered}"
    );
}

#[test]
fn the_environment_form_omits_the_actual_section_when_there_is_no_build() {
    let snapshot = snapshot(
        vec![legacy("legacy", "main", &[BRANCH])],
        vec![feature(
            BRANCH,
            &[("legacy", true, ActualMembership::Unknown)],
        )],
    );

    let rendered =
        render_why(&build_why(&snapshot, &WhySubject::Environment("legacy".to_string())).unwrap());
    assert!(has_line(&rendered, "Desired"), "{rendered}");
    assert!(
        !rendered.lines().any(|l| l == "Actual"),
        "an absent section, not an empty one:\n{rendered}"
    );
    assert!(
        has_line(&rendered, &format!("  {BRANCH}  ? Actual unknown")),
        "{rendered}"
    );
    assert!(
        has_line(
            &rendered,
            "                 hitch has no build record for this environment, so it cannot say what the last build contained"
        ),
        "{rendered}"
    );
    // And it says it will not guess, rather than suggesting a rebuild that would
    // overwrite a build it cannot describe.
    assert!(
        has_line(
            &rendered,
            "  hitch cannot describe the last build, so it will not guess at one"
        ),
        "{rendered}"
    );
}

/// Six health variants, six grammatical sentences, none with a hole in it.
///
/// `health_sentence` is private, so this goes through the real output path rather
/// than asserting on a function the command does not call.
#[test]
fn every_health_variant_has_a_sentence_that_fits_the_environment_name() {
    let cases: Vec<(EnvironmentHealth, &str)> = vec![
        (
            EnvironmentHealth::Realised,
            "dev is realised: the build matches its declaration.",
        ),
        (
            EnvironmentHealth::PartiallyRealised {
                held: vec!["feature/alpha".to_string(), "feature/beta".to_string()],
            },
            "dev is partially realised: feature/alpha, feature/beta held out of the build.",
        ),
        (
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![],
                added: vec![],
                removed: vec![],
            },
            "dev needs a rebuild: its inputs have moved since the last build.",
        ),
        (EnvironmentHealth::NeverBuilt, "dev has never been built."),
        (
            EnvironmentHealth::LegacyUnknown,
            "dev has a build hitch cannot describe.",
        ),
        (
            EnvironmentHealth::MissingBranch,
            "dev's branch does not exist locally.",
        ),
    ];

    for (health, sentence) in cases {
        let snapshot = snapshot(
            vec![built("dev", "main", &[BRANCH], health.clone(), &[], &[])],
            vec![],
        );
        let rendered =
            render_why(&build_why(&snapshot, &WhySubject::Environment("dev".to_string())).unwrap());
        assert!(
            has_line(&rendered, sentence),
            "expected {sentence:?} for {}, got:\n{rendered}",
            health.label()
        );
        // The two properties that make a sentence usable rather than merely
        // present: no format hole, and the environment named in it.
        assert!(!sentence.contains('{'), "no format hole: {sentence}");
        assert!(
            sentence.starts_with("dev"),
            "{sentence} names the environment"
        );
    }
}

/// §14 requires the text to survive colour being off. Nothing in `render_why`
/// emits an escape, so the real assertions are that there is none *and* that no
/// line begins with a bare glyph — a reader with colour disabled must still get
/// the word.
#[test]
fn a_why_renders_the_same_words_with_no_colour_in_it() {
    let snapshot = seven_states();
    for subject in [
        WhySubject::Feature(BRANCH.to_string()),
        WhySubject::FeatureIn(BRANCH.to_string(), "qa".to_string()),
        WhySubject::Environment("qa".to_string()),
    ] {
        let rendered = render_why(&build_why(&snapshot, &subject).unwrap());
        assert!(
            !rendered.contains('\u{1b}'),
            "no escape sequence for {subject:?}:\n{rendered}"
        );

        // A line that opens with a non-ASCII character opens with a *glyph* —
        // the membership markers, the em dash, the arrow in a
        // feature-in-environment headline. Colour is decoration, so a glyph
        // that carries no word is the only thing a reader with colour disabled
        // can read. Asserted structurally rather than by listing the glyphs, so
        // a new one is covered the day it is added.
        for line in rendered.lines() {
            let trimmed = line.trim_start();
            match trimmed.chars().next() {
                None => continue,
                Some(lead) if lead.is_ascii() => continue,
                Some(lead) => {
                    assert_eq!(
                        trimmed.chars().nth(1),
                        Some(' '),
                        "glyph {lead} with no word after it: {line:?}"
                    );
                    assert!(
                        trimmed.chars().nth(2).is_some_and(|c| !c.is_whitespace()),
                        "glyph {lead} followed by only whitespace: {line:?}"
                    );
                }
            }
        }
    }
}

/// A branch name is user data, not layout: it is never truncated, elided, or
/// wrapped, because `feature/pay…` in an answer is a branch name hitch made up.
#[test]
fn a_long_branch_name_is_shown_in_full_everywhere_it_appears() {
    let long = "feature/payment-gateway-integration-for-the-new-checkout-experience";
    let snapshot = snapshot(
        vec![built(
            "dev",
            "main",
            &[long],
            EnvironmentHealth::Realised,
            &[long],
            &[],
        )],
        vec![feature(long, &[("dev", true, ActualMembership::Included)])],
    );

    for subject in [
        WhySubject::Feature(long.to_string()),
        WhySubject::FeatureIn(long.to_string(), "dev".to_string()),
        WhySubject::Environment("dev".to_string()),
    ] {
        let rendered = render_why(&build_why(&snapshot, &subject).unwrap());
        assert!(
            rendered.contains(long),
            "the name in full for {subject:?}:\n{rendered}"
        );
        assert!(
            !rendered.contains('…'),
            "nothing elided for {subject:?}:\n{rendered}"
        );
    }
}
