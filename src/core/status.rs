//! The status view, projected from one snapshot.
//!
//! Everything here is a pure function of a [`RepositoryStateSnapshot`]: no
//! `GlobalContext`, no repository, no clock. That is not a stylistic
//! preference — it is the same rule `build_status_model` was built under, for
//! the same reason. A view that can re-read the repository can disagree with
//! the thing it is describing, and P3 already paid for that bug once, when
//! `hitch status` derived the same verdict four times from four timestamps.
//! Projecting a snapshot that was already taken makes a second opinion
//! impossible to write.
//!
//! The matrix (§12) and `hitch why` (§14) are both views over the same
//! snapshot, so they live here rather than each reaching into `core::state`.
//! The one thing they must agree on is what a cell *means*, and that is
//! [`MatrixCell::classify`] — a total function, called from both, with no
//! wildcard arm so a new `ActualMembership` variant is a compile error rather
//! than a silently-wrong grid.

use crate::core::state::{
    ActualMembership, EnvironmentHealth, EnvironmentState, FeatureMembership, FeatureState,
    RepositoryStateSnapshot,
};
use chrono::{DateTime, Utc};

#[derive(serde::Serialize, Debug, Clone)]
pub struct StatusSummary {
    pub total_envs: usize,
    pub locked_envs: usize,
    pub needs_rebuild_envs: usize,
    pub never_rebuilt_envs: usize,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct EnvironmentStatusModel {
    pub name: String,
    pub base: String,
    pub branches: Vec<String>,
    pub locked: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub rebuilt_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub requires_approval: bool,
    pub min_approvals: usize,
    pub approvers: Vec<String>,
    /// The environment's single verdict, carried whole so a renderer cannot
    /// re-derive one. This replaces a private `RebuildState` enum that
    /// answered the same question a fourth time, by timestamp.
    pub state: EnvironmentState,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct StatusModel {
    pub current_branch: Option<String>,
    pub summary: StatusSummary,
    pub environments: Vec<EnvironmentStatusModel>,
}

/// Project a snapshot into the status view.
///
/// Takes the snapshot rather than building one, and is infallible by
/// construction: a view that re-read the repository could disagree with the
/// snapshot it is supposed to be describing, and nothing in the caller would
/// notice.
pub fn build_status_model(snapshot: &RepositoryStateSnapshot) -> StatusModel {
    let environments: Vec<EnvironmentStatusModel> = snapshot
        .environments
        .iter()
        .map(|state| EnvironmentStatusModel {
            name: state.name.clone(),
            base: state.base.clone(),
            branches: state
                .desired
                .branches
                .iter()
                .map(|b| b.name.clone())
                .collect(),
            locked: state.locked,
            locked_by: state.locked_by.clone(),
            locked_at: state.locked_at,
            rebuilt_at: state.rebuilt_at,
            released_at: state.released_at,
            requires_approval: state.approval_policy.required,
            min_approvals: state.approval_policy.min_approvals,
            approvers: state.approval_policy.approvers.clone(),
            state: state.clone(),
        })
        .collect();

    let total_envs = environments.len();
    let locked_envs = environments.iter().filter(|e| e.locked).count();
    // `is_actionable` rather than a local `matches!`: the question "is there
    // pending work here" is answered by the health enum, and re-deciding it
    // here is exactly how a second verdict gets invented.
    let needs_rebuild_envs = environments
        .iter()
        .filter(|e| e.state.health.is_actionable())
        .count();
    // Never-built is a *subset* of actionable, not an alternative to it — an
    // environment that has never been built certainly has pending work — so it
    // is counted separately rather than subtracted out.
    let never_rebuilt_envs = environments
        .iter()
        .filter(|e| matches!(e.state.health, EnvironmentHealth::NeverBuilt))
        .count();

    StatusModel {
        current_branch: snapshot.current_branch.clone(),
        summary: StatusSummary {
            total_envs,
            locked_envs,
            needs_rebuild_envs,
            never_rebuilt_envs,
        },
        environments,
    }
}

// ── The feature × environment matrix (spec §12) ────────────────────────────

/// What one cell of the status matrix says about one feature in one
/// environment.
///
/// Seven states, because that is how many distinctions
/// [`ActualMembership`] plus "does a record exist" can actually support. The
/// spec's own list has seven too, but they are not the same seven: its
/// `✓ Released / ✓ In base` is unreachable from the declaration (see the
/// `AlreadyInBase` arm of [`MatrixCell::classify`]) and its `? Actual unknown`
/// and `↻ Needs rebuild` are one `Unknown` split in two by whether a build
/// record exists. Both of those are corrections, not omissions, and both are
/// argued in the doc comment on `classify`.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
// `snake_case` because P7 gives this enum a wire contract: `hitch status --json`
// emits one cell per feature × environment, and a consumer's only handle on a
// cell is the string. Deriving the default would freeze `ActualUnknown` — a Rust
// type name — into that contract, so renaming the variant would silently break
// every reader. The same rename is on `ActualComposition`, `ActualMembership`,
// and `EnvironmentHealth` in `core::state`, which reach the same envelope.
#[serde(rename_all = "snake_case")]
pub enum MatrixCell {
    /// The environment does not declare this feature. Absence, not a verdict.
    NotDesired,
    /// Declared and in the environment's last build.
    Included,
    /// Declared, and deliberately excluded from the last build because it
    /// conflicted. A fact about *that* build.
    Held,
    /// Declared, but already reachable from the environment's base, so
    /// building it changes nothing.
    InBase,
    /// Declared, a build exists, and that build does not mention this branch:
    /// a promotion that has not been built yet.
    NeedsRebuild,
    /// Declared, and hitch cannot say where it stands because no trustworthy
    /// record describes the build.
    ActualUnknown,
    /// Declared, but no ref resolves for it — locally or on the cached
    /// remote-tracking ref.
    Missing,
}

impl MatrixCell {
    /// The one cell verdict. Total, and with no wildcard arm: adding an
    /// `ActualMembership` variant is a compile error here rather than a cell
    /// that quietly reads as "included".
    ///
    /// # Order of authority
    ///
    /// 1. **Not declared** outranks everything. If the environment does not
    ///    declare the branch, no question about its standing in the build is
    ///    being asked.
    /// 2. **Missing** is next, and for the same reason
    ///    `membership_within` puts it second (`core::state`): a declared
    ///    branch with no ref anywhere cannot be held, cannot be in base, and
    ///    cannot be in a build.
    /// 3. **Held** and **InBase** are read straight from the record or the
    ///    live base check, in that order, and are facts about the current
    ///    state.
    /// 4. **Unknown** is the arm that needs splitting, and it is the reason
    ///    `has_record` is a parameter. An `Unknown` with a record means the
    ///    branch was promoted after the build ran — there is work to do, and
    ///    `hitch rebuild` is the next action. An `Unknown` without one means
    ///    hitch has nothing to go on; the same "needs rebuild" cell would
    ///    prescribe an action for a state it cannot describe, which is how
    ///    `LegacyUnknown` came to be treated as "probably fine". Two arms,
    ///    because the user's next step genuinely differs.
    ///
    /// # A cell and its row answer different questions
    ///
    /// A cell says *what is in the last build*. A row's health says *whether
    /// that build is current*. Those come apart, and they come apart constantly:
    /// promote `feature/a`, build, then commit to `feature/a` and do not rebuild.
    /// The cell correctly reads `included` — it *is* in the build — and the row
    /// correctly reads `needs rebuild` — that build predates the commit.
    ///
    /// This is not a cell that failed to notice the movement, and "fixing" it by
    /// checking staleness inside `classify` would break the record's authority
    /// over what a build consumed — the same ordering `membership_within` in
    /// `core::state` keeps on the other side of this. The reconciliation is the
    /// summary row beneath the grid, and for a *reason* rather than a bare
    /// staleness flag, `hitch why <branch> <environment>`.
    pub fn classify(desired: bool, actual: ActualMembership, has_record: bool) -> MatrixCell {
        if !desired {
            return MatrixCell::NotDesired;
        }
        match actual {
            ActualMembership::Missing => MatrixCell::Missing,
            ActualMembership::Held => MatrixCell::Held,
            // Not "Released". A release prunes the released branches out of
            // every environment based on the released target, so a genuinely
            // released feature stops being a declared feature and would not
            // appear in this grid at all. The honest cell is the one thing
            // that *is* knowable offline: the branch's tip is already
            // reachable from this environment's base.
            ActualMembership::AlreadyInBase => MatrixCell::InBase,
            ActualMembership::Included => MatrixCell::Included,
            ActualMembership::Unknown => {
                if has_record {
                    MatrixCell::NeedsRebuild
                } else {
                    MatrixCell::ActualUnknown
                }
            }
        }
    }

    /// The word, for a reader with colour disabled or a screen reader.
    ///
    /// The glyph and the word both carry the state. §12 requires the text to
    /// be understandable without colour, and a glyph alone is not text.
    pub fn label(&self) -> &'static str {
        match self {
            MatrixCell::NotDesired => "not desired",
            MatrixCell::Included => "included",
            MatrixCell::Held => "held",
            MatrixCell::InBase => "in base",
            MatrixCell::NeedsRebuild => "needs rebuild",
            MatrixCell::ActualUnknown => "actual unknown",
            MatrixCell::Missing => "missing",
        }
    }

    /// The glyph, as a prefix the renderer puts before [`MatrixCell::label`].
    pub fn glyph(&self) -> &'static str {
        match self {
            MatrixCell::NotDesired => "—",
            MatrixCell::Included => "●",
            MatrixCell::Held => "⛔",
            MatrixCell::InBase => "=",
            MatrixCell::NeedsRebuild => "↻",
            MatrixCell::ActualUnknown => "?",
            MatrixCell::Missing => "!",
        }
    }

    /// Whether this cell names pending work a user can act on.
    ///
    /// `ActualUnknown` is not actionable: there is nothing obviously wrong to
    /// fix, and a cell that said otherwise would put a rebuild suggestion on
    /// every environment last published by `hitch release`.
    pub fn is_actionable(&self) -> bool {
        matches!(
            self,
            MatrixCell::NeedsRebuild | MatrixCell::Missing | MatrixCell::Held
        )
    }
}

/// One feature's row: its name, and one cell per environment in the model.
#[derive(serde::Serialize, Debug, Clone)]
pub struct MatrixRow {
    pub feature: String,
    /// Same order as [`MatrixModel::columns`], one entry per column. A grid
    /// with holes in it reads as "not shown" rather than "not desired", so
    /// every cell is materialised.
    pub cells: Vec<MatrixCell>,
}

/// One environment's line beneath the grid.
#[derive(serde::Serialize, Debug, Clone)]
pub struct MatrixSummaryRow {
    pub environment: String,
    pub base: String,
    pub desired: usize,
    /// Declared and in the build, plus in-base: the two cells that mean "this
    /// environment really does contain it".
    pub realised: usize,
    pub held: usize,
    pub needs_rebuild: usize,
    pub missing: usize,
    pub actual_unknown: usize,
    /// A lock is a human-facing signal rather than a composition fact, so it has
    /// no cell — but it is exactly the kind of thing a rollup line used to
    /// count, and dropping the rollup without moving it here would lose it.
    /// Read from the snapshot like everything else in this row.
    pub locked: bool,
    /// The environment's own verdict, carried whole so a renderer cannot
    /// derive a second one.
    pub health: EnvironmentHealth,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct MatrixModel {
    /// Environment names, in the order the snapshot holds them (name order,
    /// inherited — this model does not re-sort).
    pub columns: Vec<String>,
    pub rows: Vec<MatrixRow>,
    pub summaries: Vec<MatrixSummaryRow>,
}

/// Project the snapshot into the status matrix.
///
/// The counts in each [`MatrixSummaryRow`] are counted from the cells in the
/// grid rather than from a second pass over the declaration, so the numbers
/// under the table cannot disagree with the table above it.
pub fn build_matrix_model(snapshot: &RepositoryStateSnapshot) -> MatrixModel {
    let columns: Vec<String> = snapshot
        .environments
        .iter()
        .map(|e| e.name.clone())
        .collect();

    let rows: Vec<MatrixRow> = snapshot
        .features
        .iter()
        .map(|feature: &FeatureState| MatrixRow {
            feature: feature.name.clone(),
            cells: columns
                .iter()
                .map(|env_name| {
                    let membership = feature
                        .memberships
                        .iter()
                        .find(|m| &m.environment == env_name);
                    classify_from_snapshot(snapshot, env_name, membership)
                })
                .collect(),
        })
        .collect();

    let summaries = columns
        .iter()
        .map(|env_name| {
            let cells: Vec<MatrixCell> = rows
                .iter()
                .map(|row| {
                    let index = columns
                        .iter()
                        .position(|c| c == env_name)
                        .expect("column comes from `columns`");
                    row.cells[index]
                })
                .collect();
            // One lookup for both the base and the health, rather than a second
            // `find` per field. Every field here comes from the environment
            // entry or from the cells; a third source would be a third answer.
            let environment = snapshot.environments.iter().find(|e| &e.name == env_name);
            MatrixSummaryRow {
                environment: env_name.clone(),
                base: environment.map(|e| e.base.clone()).unwrap_or_default(),
                locked: environment.is_some_and(|e| e.locked),
                desired: cells
                    .iter()
                    .filter(|c| **c != MatrixCell::NotDesired)
                    .count(),
                realised: cells
                    .iter()
                    .filter(|c| matches!(c, MatrixCell::Included | MatrixCell::InBase))
                    .count(),
                held: cells.iter().filter(|c| **c == MatrixCell::Held).count(),
                needs_rebuild: cells
                    .iter()
                    .filter(|c| **c == MatrixCell::NeedsRebuild)
                    .count(),
                missing: cells.iter().filter(|c| **c == MatrixCell::Missing).count(),
                actual_unknown: cells
                    .iter()
                    .filter(|c| **c == MatrixCell::ActualUnknown)
                    .count(),
                health: environment
                    .map(|e| e.health.clone())
                    .unwrap_or(EnvironmentHealth::LegacyUnknown),
            }
        })
        .collect();

    MatrixModel {
        columns,
        rows,
        summaries,
    }
}

/// Read one `(feature, environment)` pair out of the snapshot and classify it.
///
/// Whether hitch can say what this environment's last build contained.
///
/// Public, and used by `core::why` as well, because "does a build record exist"
/// has to be one function: a `why` that asked the question its own way could
/// report `NoBuildRecord` above a cell that says `needs rebuild`.
///
/// The predicate is `!LegacyUnknown`, and it is read from `health` rather than
/// from the `ActualComposition` variant on purpose — see the long comment on
/// `classify_from_snapshot`, which is the one place that explains why.
pub fn has_record_for(environment: &crate::core::state::EnvironmentState) -> bool {
    !matches!(environment.health, EnvironmentHealth::LegacyUnknown)
}

/// A feature the environment does not declare has no `FeatureMembership` at
/// all — P3 models "not desired" as absence — and this is the one place that
/// absence is turned back into an explicit answer, because a grid needs a cell.
///
/// `has_record` is read from the environment's **health**, not from its
/// `ActualComposition` variant, and that choice is load-bearing. The question
/// is not "is there a blob at `refs/hitch/state/<env>`" but "is there a build
/// whose contents hitch can speak for" — and `health` is the one place that
/// already decided that. `LegacyUnknown` is precisely the verdict "a build
/// exists but hitch cannot describe it", so it is the only value that means
/// `ActualUnknown`. `NeverBuilt` and `MissingBranch` both mean "there is no
/// current build", which is a form of not-in-the-build with the same remedy,
/// so they classify as `NeedsRebuild`. Deriving it from the composition variant
/// instead would let a cell disagree with the summary row printed beneath it,
/// which is the one thing a grid and its totals must not do.
pub fn classify_from_snapshot(
    snapshot: &RepositoryStateSnapshot,
    env_name: &str,
    membership: Option<&FeatureMembership>,
) -> MatrixCell {
    let has_record = snapshot
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .is_some_and(has_record_for);
    match membership {
        Some(m) => MatrixCell::classify(m.desired, m.actual, has_record),
        None => MatrixCell::classify(false, ActualMembership::Unknown, has_record),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{
        ActualComposition, ApprovalPolicy, ChangedInput, DeclaredBranch, DesiredComposition,
        RecordActual,
    };
    use crate::utils::build_record::EnvironmentBuildRecord;
    use chrono::TimeZone;

    fn declared(names: &[&str]) -> DesiredComposition {
        DesiredComposition {
            base: "main".to_string(),
            base_sha: Some("0".repeat(40)),
            branches: names
                .iter()
                .map(|n| DeclaredBranch {
                    name: n.to_string(),
                    sha: Some("1".repeat(40)),
                })
                .collect(),
        }
    }

    fn record(included: &[&str], held: &[&str]) -> RecordActual {
        RecordActual {
            tip: Some("2".repeat(40)),
            base_sha: "0".repeat(40),
            included: included
                .iter()
                .map(|b| crate::utils::build_record::PinnedBranch {
                    branch: b.to_string(),
                    sha: "1".repeat(40),
                })
                .collect(),
            held: held
                .iter()
                .map(|b| crate::utils::prelude::CompatibilityConflict {
                    branch: b.to_string(),
                    conflicts_with: "other".to_string(),
                    conflicted_files: vec!["src/x.rs".to_string()],
                })
                .collect(),
            replayed_resolutions: vec![],
            built_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            record: EnvironmentBuildRecord {
                schema_version: 1,
                environment: "dev".to_string(),
                metadata_sha: "3".repeat(40),
                base_name: "main".to_string(),
                base_sha: "0".repeat(40),
                desired_branches: vec![],
                included_branches: vec![],
                held: vec![],
                replayed_resolutions: vec![],
                result_sha: "2".repeat(40),
                built_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                hitch_version: "test".to_string(),
            },
        }
    }

    fn environment(
        name: &str,
        desired: &[&str],
        actual: ActualComposition,
        health: EnvironmentHealth,
    ) -> EnvironmentState {
        EnvironmentState {
            name: name.to_string(),
            base: "main".to_string(),
            desired: declared(desired),
            actual,
            health,
            locked: false,
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

    fn feature(name: &str, memberships: Vec<(&str, bool, ActualMembership)>) -> FeatureState {
        FeatureState {
            name: name.to_string(),
            memberships: memberships
                .into_iter()
                .map(|(environment, desired, actual)| FeatureMembership {
                    environment: environment.to_string(),
                    desired,
                    actual,
                })
                .collect(),
        }
    }

    fn snapshot(
        environments: Vec<EnvironmentState>,
        features: Vec<FeatureState>,
    ) -> RepositoryStateSnapshot {
        RepositoryStateSnapshot {
            metadata_sha: Some("3".repeat(40)),
            current_branch: Some("main".to_string()),
            environments,
            features,
            captured_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }

    /// Every `ActualMembership`, so the cross-product test below cannot go
    /// stale when a variant is added — this list and the `match` in
    /// `classify` are the same set, and a mismatch is a compile or lint
    /// failure rather than a silently-unclassified cell.
    const ALL_MEMBERSHIPS: [ActualMembership; 5] = [
        ActualMembership::Included,
        ActualMembership::Held,
        ActualMembership::AlreadyInBase,
        ActualMembership::Missing,
        ActualMembership::Unknown,
    ];

    /// §12's cell semantics, stated as a table so a disagreement is one
    /// changed line rather than a re-argument. The two rows that are not a
    /// one-to-one rename of an `ActualMembership` are the interesting ones:
    /// `Unknown` splits on `has_record`, and `NotDesired` needs no membership
    /// at all.
    #[test]
    fn classify_answers_the_spec_table_over_the_whole_cross_product() {
        for desired in [false, true] {
            for actual in ALL_MEMBERSHIPS {
                for has_record in [false, true] {
                    let expected = match (desired, actual, has_record) {
                        (false, _, _) => MatrixCell::NotDesired,
                        (true, ActualMembership::Missing, _) => MatrixCell::Missing,
                        (true, ActualMembership::Held, _) => MatrixCell::Held,
                        (true, ActualMembership::AlreadyInBase, _) => MatrixCell::InBase,
                        (true, ActualMembership::Included, _) => MatrixCell::Included,
                        (true, ActualMembership::Unknown, true) => MatrixCell::NeedsRebuild,
                        (true, ActualMembership::Unknown, false) => MatrixCell::ActualUnknown,
                    };
                    assert_eq!(
                        MatrixCell::classify(desired, actual, has_record),
                        expected,
                        "classify(desired={desired}, {actual:?}, has_record={has_record})"
                    );
                }
            }
        }
    }

    #[test]
    fn an_undeclared_feature_ignores_everything_else() {
        // A branch that is in the build of an environment that does not
        // declare it is still "not desired" in that environment: the cell
        // answers whether the environment wants it, not where its commits
        // happen to be reachable from.
        assert_eq!(
            MatrixCell::classify(false, ActualMembership::Included, true),
            MatrixCell::NotDesired
        );
    }

    #[test]
    fn a_missing_ref_outranks_a_record_that_mentions_nothing() {
        assert_eq!(
            MatrixCell::classify(true, ActualMembership::Missing, true),
            MatrixCell::Missing
        );
    }

    #[test]
    fn every_cell_has_a_distinct_glyph_and_a_non_empty_label() {
        let cells = [
            MatrixCell::NotDesired,
            MatrixCell::Included,
            MatrixCell::Held,
            MatrixCell::InBase,
            MatrixCell::NeedsRebuild,
            MatrixCell::ActualUnknown,
            MatrixCell::Missing,
        ];
        let mut glyphs: Vec<&str> = cells.iter().map(|c| c.glyph()).collect();
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), cells.len(), "two cells share a glyph");
        for cell in cells {
            assert!(!cell.label().is_empty());
        }
    }

    #[test]
    fn only_cells_naming_pending_work_are_actionable() {
        // `ActualUnknown` is the one that reads as actionable and is not: there
        // is nothing obviously wrong to fix, and a rebuild suggestion on every
        // environment last published by `hitch release` is noise.
        assert!(!MatrixCell::ActualUnknown.is_actionable());
        assert!(!MatrixCell::Included.is_actionable());
        assert!(!MatrixCell::InBase.is_actionable());
        assert!(!MatrixCell::NotDesired.is_actionable());
        for cell in [
            MatrixCell::NeedsRebuild,
            MatrixCell::Missing,
            MatrixCell::Held,
        ] {
            assert!(cell.is_actionable(), "{cell:?} should be actionable");
        }
    }

    /// A grid with a hole in it reads as "not shown" rather than "not
    /// desired", so every column gets an explicit cell.
    #[test]
    fn a_feature_the_environment_does_not_declare_gets_an_explicit_not_desired_cell() {
        let snap = snapshot(
            vec![
                environment(
                    "dev",
                    &["a"],
                    ActualComposition::FromRecord(Box::new(record(&["a"], &[]))),
                    EnvironmentHealth::Realised,
                ),
                environment(
                    "qa",
                    &["b"],
                    ActualComposition::FromRecord(Box::new(record(&["b"], &[]))),
                    EnvironmentHealth::Realised,
                ),
            ],
            vec![feature(
                "a",
                vec![("dev", true, ActualMembership::Included)],
            )],
        );
        let model = build_matrix_model(&snap);
        assert_eq!(model.columns, vec!["dev", "qa"]);
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].cells.len(), 2, "one cell per column");
        assert_eq!(model.rows[0].cells[1], MatrixCell::NotDesired);
    }

    /// §12's "legacy no-build-record repo" and "changed base" arms, which are
    /// the two that decide whether `has_record` is read from the right place.
    #[test]
    fn the_cell_agrees_with_the_environment_health_about_whether_a_build_is_describable() {
        let snap = snapshot(
            vec![environment(
                "dev",
                &["a", "b"],
                ActualComposition::LegacyUnknown,
                EnvironmentHealth::LegacyUnknown,
            )],
            vec![feature("a", vec![("dev", true, ActualMembership::Unknown)])],
        );
        let model = build_matrix_model(&snap);
        assert_eq!(
            model.rows[0].cells[0],
            MatrixCell::ActualUnknown,
            "no trustworthy build record means hitch cannot say where the branch stands"
        );
    }

    #[test]
    fn a_never_built_environment_classifies_a_declared_branch_as_needing_a_rebuild() {
        // "There is no current build" is a form of not-in-the-build with the
        // same remedy, so this is `NeedsRebuild` and not `ActualUnknown` — the
        // environment's own health says outright that it was never built.
        let snap = snapshot(
            vec![environment(
                "dev",
                &["a"],
                ActualComposition::LegacyUnknown,
                EnvironmentHealth::NeverBuilt,
            )],
            vec![feature("a", vec![("dev", true, ActualMembership::Unknown)])],
        );
        let model = build_matrix_model(&snap);
        assert_eq!(model.rows[0].cells[0], MatrixCell::NeedsRebuild);
    }

    #[test]
    fn an_unreadable_record_is_still_a_record() {
        // `Unreadable` reads as `LegacyUnknown` in the health, so the cell must
        // follow the health rather than the composition variant — otherwise
        // the cell would claim the branch is "needs rebuild" while the summary
        // row beneath it says the same environment's actual state is unknown.
        let snap = snapshot(
            vec![environment(
                "dev",
                &["a"],
                ActualComposition::Unreadable {
                    reason: "not json".to_string(),
                },
                EnvironmentHealth::LegacyUnknown,
            )],
            vec![feature("a", vec![("dev", true, ActualMembership::Unknown)])],
        );
        let model = build_matrix_model(&snap);
        assert_eq!(model.rows[0].cells[0], MatrixCell::ActualUnknown);
    }

    #[test]
    fn the_summary_counts_are_counted_from_the_cells_above_them() {
        let snap = snapshot(
            vec![environment(
                "dev",
                &["in", "held", "stale", "gone", "unknown"],
                ActualComposition::FromRecord(Box::new(record(&["in", "held"], &["held"]))),
                EnvironmentHealth::NeedsRebuild {
                    changed_inputs: vec![ChangedInput {
                        branch: "stale".to_string(),
                        previous_sha: Some("1".repeat(40)),
                        current_sha: Some("2".repeat(40)),
                    }],
                    added: vec!["unknown".to_string()],
                    removed: vec![],
                },
            )],
            vec![
                feature("in", vec![("dev", true, ActualMembership::Included)]),
                feature("held", vec![("dev", true, ActualMembership::Held)]),
                feature("stale", vec![("dev", true, ActualMembership::Included)]),
                feature("gone", vec![("dev", true, ActualMembership::Missing)]),
            ],
        );
        let model = build_matrix_model(&snap);
        let summary = &model.summaries[0];
        assert_eq!(summary.desired, 4);
        assert_eq!(summary.realised, 2, "included + in base");
        assert_eq!(summary.held, 1);
        assert_eq!(summary.needs_rebuild, 0);
        assert_eq!(summary.missing, 1);
        assert_eq!(summary.base, "main");

        // The invariant, recomputed from the grid rather than from the model:
        // a summary line that disagrees with the row above it is the drift
        // this design exists to prevent.
        let counted = |predicate: fn(&MatrixCell) -> bool| {
            model
                .rows
                .iter()
                .flat_map(|r| r.cells.iter())
                .filter(|c| predicate(c))
                .count()
        };
        assert_eq!(summary.desired, counted(|c| *c != MatrixCell::NotDesired));
        assert_eq!(summary.held, counted(|c| *c == MatrixCell::Held));
        assert_eq!(summary.missing, counted(|c| *c == MatrixCell::Missing));
        assert_eq!(
            summary.realised,
            counted(|c| matches!(c, MatrixCell::Included | MatrixCell::InBase))
        );
    }

    #[test]
    fn columns_and_rows_inherit_the_snapshots_own_order() {
        // `snapshot.features` is `BTreeMap`-ordered and `snapshot.environments`
        // is name-sorted, so the model re-sorts nothing. A second sort here
        // would be a place for the two orders to diverge.
        let snap = snapshot(
            vec![
                environment(
                    "qa",
                    &[],
                    ActualComposition::LegacyUnknown,
                    EnvironmentHealth::NeverBuilt,
                ),
                environment(
                    "dev",
                    &[],
                    ActualComposition::LegacyUnknown,
                    EnvironmentHealth::NeverBuilt,
                ),
            ],
            vec![feature("zebra", vec![]), feature("alpha", vec![])],
        );
        let model = build_matrix_model(&snap);
        assert_eq!(
            model.columns,
            vec!["qa", "dev"],
            "as the snapshot ordered them"
        );
        assert_eq!(
            model
                .rows
                .iter()
                .map(|r| r.feature.clone())
                .collect::<Vec<_>>(),
            vec!["zebra", "alpha"],
            "as the snapshot ordered them"
        );
    }
}
