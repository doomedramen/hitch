//! The metadata planner's diff, held against the model it diffs.
//!
//! `hitch set`'s plan is the *difference* between the declared environment and
//! the resolved one. That is what makes `NoChange` a truthful outcome and what
//! makes "3 settings" in the headline a count of things that will actually move
//! — and it is a single function, `field_changes`, comparing two `Environment`
//! values field by field.
//!
//! What is easy to get wrong is a field that is *named* in one place and
//! *compared* in another. `EnvironmentField` is a real enum, so a variant added
//! to it breaks the compiler in `changed_fields` — that match is exhaustive and
//! that is the compiler doing the work. But `field_changes` does not match on
//! the enum, it *names* variants, so nothing connects the two: a new field
//! compiles the moment its display name exists, and the comparison is an
//! optional extra nobody is prompted for. Missing it is the worst kind of bug
//! in this file, because it is silent in the worst way — `hitch set dev
//! --<that flag>` produces an empty change list, so the plan says "will do
//! nothing", the outcome is `NoChange`, and the receipt reports a no-op, while
//! `apply_metadata_plan` writes the edit anyway. Nothing crashes, the JSON is
//! well-formed, and the declaration ends up different from what every document
//! about it claimed.
//!
//! So these are source walks rather than behavioural tests: five hand-written
//! arms would pass forever and catch none of that. The walk reads the enum the
//! diff is supposed to be total over and requires each variant to appear in
//! both the comparison and the naming.

/// The body of `fn <name>` in `file`, brace-matched.
///
/// Brace counting rather than a line range, because a function that grows a
/// helper or a comment mentioning an `EnvironmentField` would move a line
/// range's meaning without moving its intent. The `fn` line is located by a
/// regex-ish search so an added `use` or a reordering above it is harmless.
fn fn_body(file: &str, name: &str) -> String {
    let text = std::fs::read_to_string(source(file)).expect("readable source");
    let marker = format!("fn {name}");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("no `fn {name}` in {file}"));
    // A `fn` signature can be multi-line, so the body's opening brace is the
    // first one at or after the name — not the first character after it.
    let open = text[start..]
        .find('{')
        .map(|at| start + at)
        .expect("a body");

    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (i, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return text[open..open + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in `{name}`");
}

fn source(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The variant names of an enum, read out of its declaration.
fn enum_variants(file: &str, name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(source(file)).expect("readable source");
    let marker = format!("pub enum {name} {{");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("no `pub enum {name}` in {file}"));
    let body_start = start + marker.len();
    let body_end = text[body_start..]
        .find("\n}")
        .map(|at| body_start + at)
        .expect("a closing brace");

    text[body_start..body_end]
        .lines()
        .map(str::trim)
        // Skip attributes, doc comments, and the blanks between variants: what
        // is left is a variant name, or a `Variant(Type)` alias.
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("//")
                && !line.starts_with("#[")
                && !line.starts_with("///")
        })
        .map(|line| {
            line.split([' ', '(', ',', '='])
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// Every `EnvironmentField` is compared by the diff that makes a `hitch set`
/// plan.
///
/// The compiler already forces a *name* for a new variant — `changed_fields`'s
/// `match` is exhaustive — but nothing forces a *comparison*. `field_changes`
/// names variants rather than matching on them, so its relationship to the enum
/// is a convention, and the failure mode is the silent one described in this
/// file's header. This is the test that makes the convention hold on both sides
/// at once rather than one.
#[test]
fn every_environment_field_is_diffed_by_the_set_planner() {
    let fields = enum_variants("src/operations/model.rs", "EnvironmentField");
    assert!(
        fields.len() >= 5,
        "the parse found {} EnvironmentField variants, which is too few to be \
         reading the enum: {fields:?}",
        fields.len(),
    );
    let body = fn_body("src/operations/metadata.rs", "field_changes");

    let missing: Vec<&String> = fields
        .iter()
        .filter(|f| !body.contains(&format!("EnvironmentField::{f}")))
        .collect();
    assert!(
        missing.is_empty(),
        "these EnvironmentField variants are never compared by `field_changes`, so \
         a `hitch set` that changes one produces an empty change list — a plan that \
         says it will do nothing, a `NoChange` outcome, and a receipt that reports a \
         no-op while `apply_metadata_plan` writes the edit anyway. Add a comparison \
         arm for each: {missing:?}\n  fields: {fields:?}",
    );
}

/// The diff names each field differently.
///
/// The names are joined into one line — `update base, approval threshold of
/// 'dev'` — so a field that reuses another's words is not a duplicate row, it is
/// one row that has swallowed another: the reader sees a setting named once and
/// cannot tell that two moved. The compiler's exhaustive `match` holds the arms
/// but says nothing about their contents, which is the part that can be wrong.
#[test]
fn every_environment_field_names_itself_distinctly() {
    let fields = enum_variants("src/operations/model.rs", "EnvironmentField");
    let body = fn_body("src/operations/metadata.rs", "changed_fields");

    let mut seen: std::collections::BTreeSet<String> = Default::default();
    for field in &fields {
        let arm = format!("EnvironmentField::{field} =>");
        let value = body
            .split_once(&arm)
            .unwrap_or_else(|| panic!("`changed_fields` has no arm for {field}"))
            .1
            .split_once('"')
            .unwrap_or_else(|| panic!("the {field} arm carries no quoted name"))
            .1
            .split('"')
            .next()
            .expect("a closing quote")
            .to_string();
        assert!(
            seen.insert(value.clone()),
            "{field} is called `{value}`, which another field already used — so a \
             plan that changes both lists one setting where there are two"
        );
    }
    assert_eq!(
        seen.len(),
        fields.len(),
        "each field should have contributed its own name"
    );
}

/// The plan's identity is fully qualified, so two operations that agree on their
/// environment cannot share an id.
///
/// `with_id` builds `<kind>:<environment>:<argument>:<digest>`. The digest
/// covers what the plan *read*, so two plans of the same kind over the same
/// environment with the same inputs share an id — which is correct, they are the
/// same decision. But the id is also what appears in a log line and in
/// `--verbose` output, and there the reader is distinguishing *runs*, not
/// decisions. An id that omitted the argument would give `hitch remove dev` and
/// `hitch remove qa` the same leading text with only the digest to tell them
/// apart, and the digest changes on every unrelated edit.
#[test]
fn a_plan_id_names_its_kind_environment_and_argument() {
    let text =
        std::fs::read_to_string(source("src/operations/metadata.rs")).expect("readable source");
    let body = fn_body("src/operations/metadata.rs", "with_id");
    for component in ["kind", "environment", "argument", "digest"] {
        assert!(
            body.contains(component),
            "`with_id` does not read its `{component}`, so two plans differing only \
             in it would share an id:\n{body}"
        );
    }
    assert!(
        text.contains("fn with_id"),
        "the id builder moved; this test walks it by name and needs to follow"
    );
}
