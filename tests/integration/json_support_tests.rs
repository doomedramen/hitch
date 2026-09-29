//! The `--json` doc comment is a claim about *every* command, so it needs a
//! test that can fail when the CLI drifts away from it.
//!
//! The comment in `src/cli.rs` names the commands that honour the flag. That
//! sentence is prose, and prose about a growing surface goes stale silently:
//! adding a command that plans nothing breaks no build, it just makes the
//! sentence wrong, and a user who trusts it parses a stream that has prose in
//! it. `the_documented_json_command_list_is_the_real_one` compares the sentence
//! against a source walk in both directions, and
//! `every_documented_json_command_exists_in_the_command_tree` closes the
//! remaining gap — a source walk and a doc comment can agree on a name that is
//! not a command at all.
//!
//! The walk is over `src/commands/**` rather than over `OperationKind`, because
//! a *command* reaches a document by calling `emit_receipt`, and a kind is a
//! model variant: a `--dry-run` emits a plan and never a receipt, so the two
//! legitimately differ. Subcommand files (`approvals/approve.rs`) are named
//! `parent/stem` to match how the comment and clap both spell them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::CommandFactory;

/// The crate root, so the walk does not depend on the test's working directory.
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every command invocation clap knows, as `parent/child/…`.
///
/// Recursive because a documented entry may name a subcommand of a subcommand,
/// and a test that only walked one level would report a false negative for it.
fn invocation_names() -> BTreeSet<String> {
    fn walk(cmd: &clap::Command, prefix: &str, out: &mut BTreeSet<String>) {
        for sub in cmd.get_subcommands() {
            let name = if prefix.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{prefix}/{}", sub.get_name())
            };
            out.insert(name.clone());
            walk(sub, &name, out);
        }
    }
    let mut out = BTreeSet::new();
    walk(&hitch::cli::Cli::command(), "", &mut out);
    out
}

/// The command names the `--json` doc comment claims, parsed out of the comment.
///
/// Parsed rather than retyped so the test cannot drift from the sentence a user
/// actually reads. A second hand-maintained list in this file would be a second
/// thing to forget, which is the failure this whole file exists to prevent.
fn documented_json_commands() -> BTreeSet<String> {
    let cli = hitch::cli::Cli::command();
    let json_arg = cli
        .get_arguments()
        .find(|a| a.get_id() == "json")
        .expect("--json is a registered global argument");
    let help = json_arg
        .get_long_help()
        .or_else(|| json_arg.get_help())
        .expect("--json carries a doc comment")
        .to_string();

    let sentence = help
        .lines()
        .find(|line| line.contains("Honours this today"))
        .expect("the `--json` doc comment names the commands it honours");
    let after_marker = sentence
        .split_once("Honours this today:")
        .expect("the marker is present")
        .1;
    // The sentence ends with an em-dash clause of its own, so only the names
    // before it are the list.
    let names = after_marker.split('—').next().unwrap_or(after_marker);

    names
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|name| name.trim().replace(' ', "/"))
        .filter(|name| !name.is_empty())
        .collect()
}

/// The invocations whose own source reaches a JSON emitter.
fn commands_reaching_an_emitter(root: &Path) -> BTreeSet<String> {
    fn reaches_emitter(text: &str) -> bool {
        ["emit_json(", "emit_plan(", "emit_receipt("]
            .iter()
            .any(|call| text.contains(call))
    }

    let commands = root.join("src/commands");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&commands)
        .expect("src/commands is readable")
        .map(|e| e.expect("a readable dir entry").path())
        .collect();
    entries.sort();

    let mut out = BTreeSet::new();
    for path in entries {
        if path.is_file() {
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable source");
            if reaches_emitter(&text) {
                out.insert(
                    path.file_stem()
                        .expect("a file stem")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        } else if path.is_dir() {
            let parent = path
                .file_name()
                .expect("a dir name")
                .to_string_lossy()
                .into_owned();
            let mut subs: Vec<PathBuf> = std::fs::read_dir(&path)
                .expect("a readable subcommand dir")
                .map(|e| e.expect("a readable dir entry").path())
                .collect();
            subs.sort();
            for sub in subs {
                if sub.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&sub).expect("readable source");
                if reaches_emitter(&text) {
                    out.insert(format!(
                        "{parent}/{}",
                        sub.file_stem().expect("a file stem").to_string_lossy()
                    ));
                }
            }
        }
    }
    out
}

/// The doc comment's list and the code's list are the same list.
#[test]
fn the_documented_json_command_list_is_the_real_one() {
    let documented = documented_json_commands();
    let actual = commands_reaching_an_emitter(&crate_root());

    let undocumented: Vec<&String> = actual.difference(&documented).collect();
    assert!(
        undocumented.is_empty(),
        "these commands emit a `--json` document but the `--json` doc comment does \
         not name them: {undocumented:?}\n  \
         Add them to the comment in `src/cli.rs`, or find out why they reach an \
         emitter.\n  documented: {documented:?}\n  actual:    {actual:?}"
    );

    let overclaimed: Vec<&String> = documented.difference(&actual).collect();
    assert!(
        overclaimed.is_empty(),
        "the `--json` doc comment names commands that emit no document, so a \
         caller that trusts it will parse nothing: {overclaimed:?}\n  \
         documented: {documented:?}\n  actual:    {actual:?}"
    );
}

/// Every name the comment lists is a real invocation.
///
/// Neither half above can see this on its own: the source walk finds
/// `src/commands/approvals/approve.rs` and the comment says `approvals approve`,
/// and the two would agree on `hitch promto` just as happily. clap's own tree
/// is the only thing that knows what a user can type.
#[test]
fn every_documented_json_command_exists_in_the_command_tree() {
    let invocations = invocation_names();
    let documented = documented_json_commands();

    assert!(
        !documented.is_empty(),
        "the parse found no command names — the doc comment's wording changed and \
         this test needs to follow it"
    );
    for name in &documented {
        assert!(
            invocations.contains(name),
            "the `--json` doc comment names `{name}`, which is not a command. \
             It is documented as honouring a flag it cannot be given."
        );
    }
}

/// The list is not a subset of the whole CLI — it is the mutating commands plus
/// the three read-only ones, and the test says so in a form that a new read-only
/// command would have to acknowledge.
///
/// This is the half a membership comparison cannot express. `status`, `why` and `log`
/// are the read-only commands with a document, and they are the reason the
/// comment describes two envelope shapes rather than one; if a third read-only
/// view were added it would have to be listed here, which is the point.
#[test]
fn the_read_only_json_commands_are_status_why_and_log() {
    let documented = documented_json_commands();
    for name in ["status", "why", "log"] {
        assert!(
            documented.contains(name),
            "`{name}` emits a `--json` document and must be in the comment: \
             {documented:?}"
        );
    }
    for name in ["add", "remove", "set", "lock", "unlock", "cleanup"] {
        assert!(
            documented.contains(name),
            "`{name}` is a P8 mutation and must be in the comment: {documented:?}"
        );
    }
    assert!(
        documented.contains("approvals/approve"),
        "`hitch approvals approve` plans and applies, so it belongs beside the \
         other mutations: {documented:?}"
    );
}
