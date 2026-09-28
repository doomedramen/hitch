//! §12's display conditions for the status matrix.
//!
//! # Why the grid is tested over hand-built models
//!
//! `build_matrix_model` is a projection of the snapshot and its own tests cover
//! the projection. What is untested by those is the *rendering* — the column
//! arithmetic, the header, the rule, and the narrow-terminal decision. Those
//! properties are only checkable by reading the output, and reading output that
//! came from a real repository makes every assertion depend on whatever history
//! the fixture happened to have. So the models here are hand-built, which also
//! makes it cheap to ask for shapes a real repo would not have: zero
//! environments, twenty features, a branch name long enough to set the column.
//!
//! # Why the alignment assertions measure the column rather than matching a
//! literal
//!
//! Every width here comes from the data, so a hardcoded row is a test of the
//! fixture's data as much as of the renderer, and it fails the moment a name
//! changes length. `column_of` locates a cell by finding the glyph that starts
//! it, which is what a reader's eye does. If the renderer ever put two cells'
//! glyphs in different columns, that is the assertion that catches it, and it
//! catches it whether or not the fixture's names are the ones in the literal.
//!
//! The summary block's assertions collapse whitespace for the same reason
//! `tests/integration/status_tests.rs` does: its name column is sized to the
//! data, so `DEV  desired 3` and `PROD  desired 1` differ in padding for no
//! reason a reader can see, and hardcoding it would make every test that adds
//! an environment silently start asserting a layout decision.

use hitch::core::render::{render_environment_summaries, render_matrix, render_matrix_at};
use hitch::core::state::EnvironmentHealth;
use hitch::core::status::{MatrixCell, MatrixModel, MatrixRow, MatrixSummaryRow};

/// Every cell state, in the order §12 lists them.
const ALL_CELLS: [MatrixCell; 7] = [
    MatrixCell::NotDesired,
    MatrixCell::Included,
    MatrixCell::Held,
    MatrixCell::InBase,
    MatrixCell::NeedsRebuild,
    MatrixCell::ActualUnknown,
    MatrixCell::Missing,
];

/// A grid with one row per cell state, one column per environment.
fn one_row_per_cell() -> MatrixModel {
    let columns: Vec<String> = ALL_CELLS
        .iter()
        .enumerate()
        .map(|(i, _)| format!("env{i}"))
        .collect();
    MatrixModel {
        rows: vec![MatrixRow {
            feature: "feature/alpha".to_string(),
            cells: ALL_CELLS.to_vec(),
        }],
        columns,
        summaries: vec![],
    }
}

/// A summary row with everything zero, so a test can set the two or three fields
/// it cares about and not have to remember the rest.
fn summary(environment: &str) -> MatrixSummaryRow {
    MatrixSummaryRow {
        environment: environment.to_string(),
        base: "main".to_string(),
        desired: 0,
        realised: 0,
        held: 0,
        needs_rebuild: 0,
        missing: 0,
        actual_unknown: 0,
        locked: false,
        health: EnvironmentHealth::Realised,
    }
}

fn summary_with(name: &str, edit: impl FnOnce(&mut MatrixSummaryRow)) -> MatrixSummaryRow {
    let mut row = summary(name);
    edit(&mut row);
    row
}

fn model(columns: &[&str], rows: &[(&str, &[MatrixCell])]) -> MatrixModel {
    MatrixModel {
        columns: columns.iter().map(|c| c.to_string()).collect(),
        rows: rows
            .iter()
            .map(|(feature, cells)| MatrixRow {
                feature: feature.to_string(),
                cells: cells.to_vec(),
            })
            .collect(),
        summaries: vec![],
    }
}

/// The character index at which `glyph` starts on `line`, or `None`.
///
/// The lookup is by glyph rather than by column index because a column index
/// encodes the layout the test is trying to check — finding the glyph is the
/// check.
fn column_of(line: &str, glyph: char) -> Option<usize> {
    line.chars().position(|c| c == glyph)
}

/// Collapse runs of horizontal whitespace to one space and trim.
///
/// The summary block pads its name column to the widest name, so `DEV  desired
/// 3` and `PROD  desired 1` are both correct. An assertion that hardcoded the
/// padding would be asserting the width decision rather than the content, and
/// would break on every fixture that changes its set of environments.
fn normalise(line: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;
    for c in line.trim().chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out
}

/// One summary block line, with its whitespace collapsed, if it exists.
fn counts_line(rendered: &str, name: &str) -> Option<String> {
    let wanted = normalise(&name.to_uppercase());
    rendered.lines().map(normalise).find(|line| {
        line.contains("desired") && (*line == wanted || line.starts_with(&format!("{wanted} ")))
    })
}

/// The whole cell a glyph opens: from the glyph to the next glyph, trimmed.
///
/// Taking only the *first word* would pass for a cell rendered as a bare glyph
/// followed by a truncated label, and taking the rest of the *line* would pass
/// for a cell rendered as a bare glyph followed by the next cell's label. Only
/// the span between this glyph and the next is a cell, so only that span is the
/// thing being claimed.
fn cell_text(row: &str, at: usize) -> String {
    let end = glyphs(row)
        .into_iter()
        .map(|(_, at)| at)
        .find(|next| *next > at)
        .unwrap_or_else(|| row.chars().count());
    row.chars()
        .skip(at)
        .take(end - at)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Every glyph in `line`, in order, with the column each sits at.
///
/// Two identical glyphs in one line would make this ambiguous, which is why no
/// fixture has two columns showing the same state.
fn glyphs(line: &str) -> Vec<(char, usize)> {
    let glyphs = ['—', '●', '⛔', '=', '↻', '?', '!'];
    line.chars()
        .enumerate()
        .filter(|(_, c)| glyphs.contains(c))
        .map(|(i, c)| (c, i))
        .collect()
}

/// §12's load-bearing condition: a cell is a **glyph and a word**, never a glyph
/// alone. This is what makes the grid readable with colour off, in a pipe, and
/// by a screen reader — and it is a condition on the *output*, so it is asserted
/// on the output rather than on `MatrixCell::glyph` and `label` separately, which
/// a renderer could satisfy by emitting only one of them.
#[test]
fn every_cell_carries_a_glyph_and_a_word() {
    let rendered = render_matrix(&one_row_per_cell());

    let row = rendered
        .lines()
        .find(|l| l.contains("feature/alpha"))
        .expect("the fixture's row is in the output");
    for cell in ALL_CELLS {
        assert!(
            row.contains(&format!("{} {}", cell.glyph(), cell.label())),
            "{cell:?} renders as {:?} but the row is {row:?}",
            format!("{} {}", cell.glyph(), cell.label())
        );
    }

    // And, read cell by cell rather than by substring, that each of the seven
    // spells out its own glyph *and* its own label. The span is bounded by the
    // next glyph, so this cannot pass for a cell rendered as a bare glyph
    // followed by a truncated or borrowed label.
    let found: Vec<String> = glyphs(row)
        .into_iter()
        .map(|(glyph, at)| {
            let cell = cell_text(row, at);
            assert!(
                cell.starts_with(glyph),
                "the span at {at} does not open with {glyph}: {row:?}"
            );
            assert!(
                !cell.trim_start_matches(glyph).trim().is_empty(),
                "glyph {glyph} at {at} has no word after it: {row:?}"
            );
            cell
        })
        .collect();
    for cell in ALL_CELLS {
        let expected = format!("{} {}", cell.glyph(), cell.label());
        assert!(
            found.contains(&expected),
            "{cell:?} should render as {expected:?} but the row spells {found:?}"
        );
    }
    assert_eq!(found.len(), 7, "all seven cells, once each: {found:?}");
}

/// Colour is decoration, and the grid is where a repository's whole shape is
/// read. If a cell were colourised, the word would still be there — but the
/// escape codes would be in every CI log, and a test that pipes `hitch status`
/// into a file would produce a file full of them.
#[test]
fn the_grid_contains_no_escape_sequence_anywhere() {
    let rendered = render_matrix(&one_row_per_cell());
    assert!(
        !rendered.contains('\u{1b}'),
        "the grid must not colourise:\n{rendered}"
    );

    // Including the summary block, which is rendered by a different function and
    // had a stray `\x1b[33m\x1b[0m` for an empty lock marker before P7.
    let summaries = render_environment_summaries(&[
        summary_with("dev", |s| s.locked = true),
        summary_with("qa", |s| s.locked = false),
        summary("empty"),
    ]);
    assert!(
        !summaries.contains('\u{1b}'),
        "the summary block must not colourise either:\n{summaries}"
    );
}

/// The rule under the header is exactly as wide as the table, and the table is
/// exactly as wide as the rule. A rule computed separately — a constant, or the
/// header's length — is the classic way a table stops being a table.
#[test]
fn the_rule_is_exactly_as_wide_as_the_widest_row() {
    for columns in [
        vec!["dev"],
        vec!["dev", "qa", "prod"],
        vec!["dev", "qa", "prod", "staging", "edge"],
    ] {
        let cells = vec![MatrixCell::NeedsRebuild; columns.len()];
        let rendered = render_matrix(&model(&columns, &[("feature/alpha", &cells)]));

        let lines: Vec<&str> = rendered.lines().collect();
        let rule = lines[1];
        assert!(
            rule.chars().all(|c| c == '─'),
            "the second line is the rule: {rule:?}"
        );

        let widest = lines
            .iter()
            .map(|l| l.trim_end().chars().count())
            .max()
            .expect("lines");
        // The rule is the *untrimmed* table width, which is the last column's
        // padding included; every other line is trimmed back, so they can be
        // shorter but never longer.
        assert_eq!(
            rule.chars().count(),
            widest,
            "rule {} vs widest row {widest} for {columns:?}:\n{rendered}",
            rule.chars().count()
        );
    }
}

/// Every column's cells start in the same character position, and the header
/// above them does too.
///
/// Measured, not matched to a literal, so a long branch name moving the first
/// column does not turn this into a test of the fixture.
#[test]
fn every_column_agrees_on_its_own_start_across_all_rows() {
    // Deliberately unequal cell labels, because a column whose cells are all the
    // same length lines up no matter what the renderer does.
    let columns = ["dev", "qa", "prod", "staging"];
    let rendered = render_matrix(&model(
        &columns,
        &[
            (
                "short",
                &[
                    MatrixCell::Included,
                    MatrixCell::Held,
                    MatrixCell::InBase,
                    MatrixCell::Missing,
                ],
            ),
            (
                "a-much-longer-feature-name",
                &[
                    MatrixCell::NeedsRebuild,
                    MatrixCell::ActualUnknown,
                    MatrixCell::NotDesired,
                    MatrixCell::Included,
                ],
            ),
            (
                "mid-length",
                &[
                    MatrixCell::ActualUnknown,
                    MatrixCell::InBase,
                    MatrixCell::Held,
                    MatrixCell::NotDesired,
                ],
            ),
        ],
    ));

    let lines: Vec<&str> = rendered.lines().collect();
    let row_glyphs: Vec<Vec<usize>> = lines
        .iter()
        .skip(2)
        .map(|line| glyphs(line).into_iter().map(|(_, at)| at).collect())
        .collect();
    assert_eq!(row_glyphs.len(), 3, "three feature rows:\n{rendered}");

    // Each row's n-th glyph must be in the same column as every other row's
    // n-th glyph, and the header label for that column must begin in exactly the
    // same place. Checking the header *at the known column* rather than parsing
    // the header into fields is what keeps this from re-implementing the width
    // arithmetic it is testing.
    for (column, name) in columns.iter().enumerate() {
        let starts: Vec<usize> = row_glyphs.iter().map(|row| row[column]).collect();
        assert!(
            starts.windows(2).all(|w| w[0] == w[1]),
            "column {column} ({name}) starts at {starts:?}, not one column:\n{rendered}"
        );

        let at = starts[0];
        let header: Vec<char> = lines[0].chars().collect();
        let label: String = header[at..at + name.chars().count()].iter().collect();
        assert_eq!(
            label,
            name.to_uppercase(),
            "the header for {name} does not sit above its cells:\n{rendered}"
        );
        // The gutter is exactly two spaces, so the header cannot be narrower than
        // the cell column it labels.
        assert!(
            header[at - 1] == ' ' && header[at - 2] == ' ',
            "the gutter before {name} is not two spaces:\n{rendered}"
        );
    }
}

/// A long name beside a short one: the *table* stays aligned, because the long
/// name sets the first column's width for everyone. This is the case that a
/// per-row `name.len()` implementation gets wrong, and the case that makes
/// truncating tempting.
#[test]
fn a_long_name_beside_a_short_one_keeps_the_table_aligned_and_the_names_whole() {
    let long = "feature/payment-gateway-integration-for-the-new-checkout-experience";
    let rendered = render_matrix(&model(
        &["dev", "qa"],
        &[
            ("fix", &[MatrixCell::Included, MatrixCell::Held]),
            (long, &[MatrixCell::Missing, MatrixCell::InBase]),
        ],
    ));

    let lines: Vec<&str> = rendered.lines().collect();
    let short_start = column_of(lines[2], '●').expect("the short row's first cell");
    let long_start = column_of(lines[3], '!').expect("the long row's first cell");
    assert_eq!(
        short_start, long_start,
        "the short name's cell moved to accommodate the long one:\n{rendered}"
    );
    // Nothing is elided. A `feature/pay…` that no branch is called is a wrong
    // answer in the shape of a right one.
    assert!(
        rendered.contains(long),
        "the long name was shortened:\n{rendered}"
    );
    assert!(!rendered.contains('…'), "nothing is elided:\n{rendered}");
    // The short name is padded, not left ragged.
    assert!(
        lines[2].starts_with("fix ") && lines[2].contains("  ●"),
        "the short name is not padded into the column:\n{rendered}"
    );
}

/// The zero-environment repository is a real state — `hitch init` with no
/// environments yet. What it must not do is crash, print a lone rule, or imply
/// there is nothing here.
#[test]
fn a_repository_with_no_environments_says_so_rather_than_printing_an_empty_grid() {
    let empty = MatrixModel {
        columns: vec![],
        rows: vec![],
        summaries: vec![],
    };
    let rendered = render_matrix(&empty);

    assert_eq!(
        rendered.lines().count(),
        2,
        "a header and a rule, and nothing else:\n{rendered}"
    );
    assert_eq!(rendered.lines().next().unwrap().trim_end(), "Feature");
    assert!(rendered.lines().nth(1).unwrap().chars().all(|c| c == '─'));

    // A zero column budget cannot fit even the `Feature` header, so this is the
    // fallback — and "0 environments declared" is a better answer than a
    // two-line table with nothing in it, which is the point of the prose form.
    // A grid wide enough for the header gets the grid, empty.
    let at_zero = render_matrix_at(&empty, 0);
    assert!(
        at_zero.contains("0 environments declared, 0 features."),
        "{at_zero}"
    );
    assert!(at_zero.contains("this terminal has 0"), "{at_zero}");
    assert_eq!(
        render_matrix_at(&empty, 7),
        rendered,
        "a budget that fits the header renders the grid, empty or not"
    );
}

/// Rows with no columns, and columns with no rows: the model is rectangular by
/// construction, but a fixture can still be asked for a degenerate one and the
/// renderer must not index past the end of a row.
#[test]
fn a_row_with_no_cells_and_a_column_with_no_rows_both_render() {
    let no_cells = render_matrix(&model(&["dev", "qa"], &[("feature/alpha", &[])]));
    assert!(
        no_cells.contains("feature/alpha"),
        "the row's name still appears:\n{no_cells}"
    );
    assert!(
        !no_cells.contains('!'),
        "and no cell is invented for it:\n{no_cells}"
    );

    let no_rows = render_matrix(&model(&["dev", "qa"], &[]));
    assert!(no_rows.lines().next().unwrap().contains("DEV"));
    assert_eq!(
        no_rows.lines().count(),
        2,
        "header and rule only:\n{no_rows}"
    );
}

/// The first column header is a label for *branch names*, so it is padded to the
/// widest branch name and the name never gets truncated to fit a constant.
#[test]
fn the_first_column_is_narrow_only_when_the_names_are() {
    let narrow = render_matrix(&model(&["dev"], &[("a", &[MatrixCell::Included])]));
    let wide = render_matrix(&model(
        &["dev"],
        &[("feature/somewhat-longer", &[MatrixCell::Included])],
    ));

    // The `Feature` header is seven characters, so a one-character name still
    // gets a seven-wide column — the header is what the column has to fit.
    assert!(
        narrow.lines().nth(2).unwrap().starts_with("a       "),
        "the name column is at least as wide as its header:\n{narrow}"
    );
    assert!(
        wide.lines()
            .nth(2)
            .unwrap()
            .starts_with("feature/somewhat-longer "),
        "a longer name widens the column rather than overflowing it:\n{wide}"
    );
    let narrow_cells = column_of(narrow.lines().nth(2).unwrap(), '●').unwrap();
    let wide_cells = column_of(wide.lines().nth(2).unwrap(), '●').unwrap();
    assert_eq!(
        narrow_cells, 9,
        "'Feature' is seven plus a two-space gutter:\n{narrow}"
    );
    assert!(
        wide_cells > narrow_cells,
        "a longer name moves the cells right"
    );
}

/// The summary block's counts come from the cells, and the marker is a suffix.
///
/// A marker on the *front* pushes that row's name right of every other row's,
/// which is a table losing its alignment over an annotation — so the marker takes
/// the padding the name already had, and the names stay in a column.
#[test]
fn the_lock_marker_is_a_suffix_so_the_names_stay_in_a_column() {
    let rendered = render_environment_summaries(&[
        summary_with("dev", |s| s.locked = true),
        summary_with("qa", |s| s.locked = false),
    ]);

    let dev = counts_line(&rendered, "dev").expect("dev's line");
    let qa = counts_line(&rendered, "qa").expect("qa's line");
    assert!(dev.ends_with("🔒"), "the lock is a suffix: {dev:?}");
    assert!(!qa.contains('🔒'), "and absent when not locked: {qa:?}");

    // The name is the first field of both lines, in the same character column.
    // Measured on the *raw* lines, because that is the alignment being claimed.
    let raw: Vec<&str> = rendered.lines().collect();
    let dev_raw = raw
        .iter()
        .find(|l| normalise(l).starts_with("DEV "))
        .expect("dev's raw line");
    let qa_raw = raw
        .iter()
        .find(|l| normalise(l).starts_with("QA "))
        .expect("qa's raw line");
    assert_eq!(
        column_of(dev_raw, 'D'),
        column_of(qa_raw, 'Q'),
        "the locked row's name is not in the same column:\n{rendered}"
    );
}

/// A row that reads `dev  desired 3 · actual 3` is silent about three different
/// things that are not zero, and the one that is an error is a hold. So the
/// qualifier is its own clause, and the verb agrees with its count.
#[test]
fn the_summary_counts_qualify_themselves_and_agree_with_their_counts() {
    let rendered = render_environment_summaries(&[
        summary_with("dev", |s| {
            s.desired = 5;
            s.realised = 3;
            s.held = 1;
        }),
        summary_with("qa", |s| {
            s.desired = 2;
            s.needs_rebuild = 2;
        }),
        summary_with("prod", |s| {
            s.desired = 1;
            s.missing = 1;
        }),
        summary_with("edge", |s| {
            s.desired = 1;
            s.actual_unknown = 1;
        }),
    ]);

    // `desired` and `actual` are always present, so a rollup cannot read as if
    // the environment were empty.
    for (name, expected) in [
        ("dev", "DEV desired 5 · actual 3 · 1 held"),
        ("qa", "QA desired 2 · actual 0 · 2 need rebuild"),
        ("prod", "PROD desired 1 · actual 0 · 1 missing"),
        ("edge", "EDGE desired 1 · actual 0 · 1 actual unknown"),
    ] {
        assert_eq!(
            counts_line(&rendered, name).as_deref(),
            Some(expected),
            "the line for {name}:\n{rendered}"
        );
    }

    // The verb agrees with its count, and the two forms differ only in the verb —
    // which is what makes "1 held" vs "2 held" worth asserting at all.
    let two = render_environment_summaries(&[summary_with("dev", |s| s.held = 2)]);
    assert_eq!(
        counts_line(&two, "dev").as_deref(),
        Some("DEV desired 0 · actual 0 · 2 held"),
        "{two}"
    );
}

/// The health word is the *same* function `hitch status` already used, so the
/// block cannot grow a second vocabulary of verdicts. Asserted as the vocabulary
/// itself, because the property that matters is that these six words appear and
/// nothing else does.
#[test]
fn the_summary_uses_the_six_health_words_and_nothing_else() {
    let cases = [
        (EnvironmentHealth::Realised, "realised"),
        (
            EnvironmentHealth::PartiallyRealised {
                held: vec!["feature/a".to_string()],
            },
            "partially realised",
        ),
        (
            EnvironmentHealth::NeedsRebuild {
                changed_inputs: vec![],
                added: vec![],
                removed: vec![],
            },
            "needs rebuild",
        ),
        (EnvironmentHealth::NeverBuilt, "never built"),
        (EnvironmentHealth::LegacyUnknown, "actual unknown"),
        (EnvironmentHealth::MissingBranch, "branch missing"),
    ];

    for (health, word) in &cases {
        let rendered =
            render_environment_summaries(&[summary_with("dev", |s| s.health = health.clone())]);
        // On its own indented line under the counts, so it cannot be mistaken for
        // a qualifier on the counts line — `1 needs rebuild` (a count of branches)
        // and `needs rebuild` (the environment's verdict) are different claims
        // about different things.
        assert!(
            rendered.lines().any(|l| l.trim() == *word),
            "expected the health word {word:?} for {}:\n{rendered}",
            health.label()
        );
        assert!(
            rendered.lines().filter(|l| l.trim() == *word).count() == 1,
            "and exactly one of it, because one environment has one verdict:\n{rendered}"
        );
    }
}

/// The narrow-terminal decision, and it is a decision rather than a fallback:
/// below the width the grid needs, the grid is **replaced**, not squeezed.
#[test]
fn a_narrow_budget_replaces_the_grid_with_the_shape_and_where_to_read_it_in_full() {
    let columns = ["dev", "qa", "prod"];
    let cells = vec![MatrixCell::NeedsRebuild; 3];
    let full = model(&columns, &[("feature/alpha", &cells)]);
    let rendered = render_matrix(&full);

    // Measure the width the grid needs, from its own rule, so the budget is not
    // a magic number that a wider fixture would invalidate.
    let needed = rendered.lines().nth(1).unwrap().chars().count();
    assert!(needed > 0, "the rule is not empty:\n{rendered}");

    // One column of slack: the grid.
    let at_needed = render_matrix_at(&full, needed);
    assert_eq!(
        at_needed, rendered,
        "a budget that exactly fits renders the grid"
    );

    // One column short: the prose, and *not* the grid.
    let too_narrow = render_matrix_at(&full, needed - 1);
    assert_ne!(
        too_narrow, rendered,
        "a grid that does not fit is not the grid"
    );
    assert!(
        !too_narrow.contains('↻'),
        "no cells in the fallback — a cell that does not fit is a wrong cell:\n{too_narrow}"
    );
    assert!(
        !too_narrow.contains("Feature"),
        "and no table header over nothing:\n{too_narrow}"
    );
    // The two facts that make the fallback useful: the shape, and where to read
    // it in full. §12.1's expansion is the same shape, so the fallback points at
    // the flag rather than inventing a second rendering of it.
    assert!(
        too_narrow.contains("3 environments declared"),
        "{too_narrow}"
    );
    assert!(too_narrow.contains("1 feature."), "{too_narrow}");
    assert!(
        too_narrow.contains("'hitch status --environments'"),
        "{too_narrow}"
    );
    // And it says *why* it did that, with the numbers, so a reader can tell a
    // narrow terminal from a bug.
    assert!(
        too_narrow.contains(&format!("needs {needed} columns")),
        "{too_narrow}"
    );
    assert!(
        too_narrow.contains(&format!("this terminal has {}", needed - 1)),
        "{too_narrow}"
    );
}

/// Both singulars, because "1 environment declared, 1 feature." and the plural
/// forms are four different strings and a fixture with one row would pass
/// vacuously on three of them.
#[test]
fn the_fallback_agrees_with_the_grammar() {
    let one = model(&["dev"], &[("feature/alpha", &[MatrixCell::Included])]);
    let wide = render_matrix_at(&one, 10_000);
    let narrow = render_matrix_at(&one, 0);
    assert!(wide.contains("DEV"), "an unbounded budget renders the grid");
    assert!(
        narrow.contains("1 environment declared, 1 feature."),
        "{narrow}"
    );

    let many = model(
        &["dev", "qa"],
        &[
            ("feature/alpha", &[MatrixCell::Included, MatrixCell::Held]),
            ("feature/beta", &[MatrixCell::InBase, MatrixCell::Missing]),
        ],
    );
    let narrow = render_matrix_at(&many, 0);
    assert!(
        narrow.contains("2 environments declared, 2 features."),
        "{narrow}"
    );
}

/// An unbounded budget is the right default for a pipe, a CI log, or a file, and
/// inventing a width from a guess would be exactly the kind of untruthful input
/// this program keeps refusing. So a budget of zero — "no idea" — still renders
/// the grid when the grid needs nothing, rather than reporting a zero-width
/// terminal and falling back.
#[test]
fn an_unbounded_budget_is_representable_and_is_not_zero() {
    let tiny = model(&["d"], &[("f", &[MatrixCell::Included])]);
    // One column needs at least a name column, a gutter, and the cell.
    assert!(
        render_matrix_at(&tiny, 1_000).contains('●'),
        "a generous budget renders the grid"
    );
    // Zero *is* a real budget and zero is narrower than any grid, so it falls
    // back — but it reports itself as zero rather than as a made-up number.
    let at_zero = render_matrix_at(&tiny, 0);
    assert!(
        at_zero.contains("this terminal has 0"),
        "a real zero budget is honoured as itself:\n{at_zero}"
    );
}

/// Twenty features and twelve environments: the shape a large repository
/// produces, and the one where a width bug and an O(n²) bug would both show.
#[test]
fn a_wide_repository_renders_every_row_and_every_column_exactly_once() {
    let columns: Vec<String> = (0..12).map(|i| format!("env{i:02}")).collect();
    let rows: Vec<MatrixRow> = (0..20)
        .map(|i| MatrixRow {
            feature: format!("feature/{i:02}"),
            // Rotated, so no two rows are identical and no column is uniform.
            cells: vec![ALL_CELLS[i % ALL_CELLS.len()]; columns.len()],
        })
        .collect();
    let full = MatrixModel {
        columns,
        rows,
        summaries: vec![],
    };
    let rendered = render_matrix(&full);

    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines.len(), 22, "header, rule, twenty rows:\n{lines:?}");

    for row in &full.rows {
        assert!(
            lines.iter().any(|l| l.starts_with(&row.feature)),
            "row {} is missing:\n{rendered}",
            row.feature
        );
    }
    // Every cell's glyph is on the page the right number of times: 20 rows × 12
    // columns, per glyph, minus the rows that chose a different one.
    for cell in ALL_CELLS {
        let expected = full
            .rows
            .iter()
            .filter(|r| r.cells.iter().all(|c| *c == cell))
            .count()
            * full.columns.len();
        let found: usize = lines
            .iter()
            .skip(2)
            .map(|l| {
                glyphs(l)
                    .into_iter()
                    .filter(|(c, _)| glyph_of(*c) == cell)
                    .count()
            })
            .sum();
        assert_eq!(
            found, expected,
            "{cell:?} appears {found} times, wanted {expected}"
        );
    }
    // And the columns all still agree after all that.
    let starts: Vec<Vec<usize>> = lines
        .iter()
        .skip(2)
        .map(|l| glyphs(l).into_iter().map(|(_, at)| at).collect())
        .collect();
    for column in 0..full.columns.len() {
        assert!(
            starts.windows(2).all(|w| w[0][column] == w[1][column]),
            "column {column} is ragged across twenty rows"
        );
    }
}

fn glyph_of(glyph: char) -> MatrixCell {
    ALL_CELLS
        .into_iter()
        .find(|c| c.glyph().starts_with(glyph))
        .unwrap_or_else(|| panic!("{glyph} is not one of the seven glyphs"))
}
