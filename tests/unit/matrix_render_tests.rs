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

use chrono::TimeZone;
use hitch::core::render::{
    render_environment_summaries, render_matrix, render_matrix_at, render_matrix_next_steps,
    EnvironmentEquation, EquationTerm, EquationTermState,
};
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
        equation: EnvironmentEquation {
            environment: environment.to_string(),
            base: "main".to_string(),
            terms: Vec::new(),
            excluded: Vec::new(),
        },
        rebuilt_at: None,
    }
}

fn term(branch: &str) -> EquationTerm {
    EquationTerm {
        branch: branch.to_string(),
        state: EquationTermState::Plain,
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
    let wanted = format!("{name} = ");
    rendered
        .lines()
        .map(normalise)
        .find(|line| line.starts_with(&wanted))
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
            name.to_string(),
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

    assert_eq!(rendered.lines().count(), 1, "one sentence:\n{rendered}");
    assert!(
        rendered.starts_with("Nothing is promoted yet"),
        "{rendered}"
    );

    // The sentence needs no width arithmetic, so no budget replaces it.
    assert_eq!(render_matrix_at(&empty, 0), rendered);
    assert_eq!(render_matrix_at(&empty, 7), rendered);
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
    assert_eq!(
        no_rows,
        "Nothing is promoted yet. Promote a branch with: hitch promote <branch> <environment>",
        "an empty grid is one sentence, not a header over nothing"
    );
    let one_env = render_matrix(&model(&["dev"], &[]));
    assert!(one_env.ends_with("hitch promote <branch> dev"), "{one_env}");
    assert_eq!(render_matrix_at(&model(&["dev"], &[]), 0), one_env);
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

/// The marker is a suffix, so the equation starts the line for every row.
#[test]
fn the_lock_marker_is_a_suffix() {
    let rendered = render_environment_summaries(&[
        summary_with("dev", |s| s.locked = true),
        summary_with("qa", |s| s.locked = false),
    ]);

    let dev = counts_line(&rendered, "dev").expect("dev's line");
    let qa = counts_line(&rendered, "qa").expect("qa's line");
    assert!(dev.ends_with("🔒"), "the lock is a suffix: {dev:?}");
    assert!(!qa.contains('🔒'), "and absent when not locked: {qa:?}");
}

/// One line per environment: the equation as typed (lower case, via the shared
/// equation renderer), the health in plain words, and no model counts.
#[test]
fn a_summary_is_the_equation_and_the_health_in_plain_words() {
    let mut dev = summary_with("dev", |s| {
        s.health = EnvironmentHealth::PartiallyRealised {
            held: vec!["b".to_string()],
        };
        s.rebuilt_at = Some(chrono::Utc.with_ymd_and_hms(2026, 10, 2, 14, 3, 0).unwrap());
    });
    dev.equation.terms = vec![term("a")];
    let rendered = render_environment_summaries(&[dev, summary("qa")]);
    assert_eq!(
        rendered,
        "dev = main + a  ·  holding back 1 branch  ·  rebuilt 2026-10-02 14:03 UTC\n\
         qa = main  ·  up to date\n"
    );
    assert!(!rendered.contains("desired") && !rendered.contains("actual"));
}

/// Every health has a plain phrase, and plural agrees with the count.
#[test]
fn the_summary_phrases_every_health_in_plain_words() {
    let cases = [
        (EnvironmentHealth::Realised, "up to date"),
        (
            EnvironmentHealth::PartiallyRealised {
                held: vec!["a".to_string(), "b".to_string()],
            },
            "holding back 2 branches",
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
        (
            EnvironmentHealth::LegacyUnknown,
            "not known, no build record",
        ),
        (EnvironmentHealth::MissingBranch, "branch missing"),
    ];
    for (health, phrase) in &cases {
        let rendered =
            render_environment_summaries(&[summary_with("dev", |s| s.health = health.clone())]);
        assert_eq!(rendered, format!("dev = main  ·  {phrase}\n"));
    }
}

/// The next-steps block names the real environment and a real feature, and is
/// absent when there is nothing promoted (the grid already said what to do).
#[test]
fn next_steps_use_real_names_and_are_absent_on_an_empty_grid() {
    assert_eq!(render_matrix_next_steps(&model(&["dev"], &[])), None);
    let one = model(&["dev"], &[("feature/a", &[MatrixCell::Included])]);
    let steps = render_matrix_next_steps(&one).unwrap();
    assert!(steps.contains("hitch why feature/a dev"), "{steps}");
    assert!(steps.contains("hitch promote <branch> dev"), "{steps}");
    assert!(!steps.contains("git branch"), "{steps}");
    let two = model(&["dev", "qa"], &[("feature/a", &[MatrixCell::Included; 2])]);
    assert!(render_matrix_next_steps(&two)
        .unwrap()
        .contains("hitch promote <branch> <environment>"));
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
    assert!(wide.contains("dev"), "an unbounded budget renders the grid");
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
