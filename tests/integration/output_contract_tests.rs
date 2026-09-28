//! Two contracts about how the CLI is allowed to *print*, checked against the
//! source rather than against a rendered string.
//!
//! Both exist because the failure they catch is invisible in review and
//! invisible in a screenshot: it needs a *specific* message at a *specific* level
//! to show up, and every individual line looks fine. A test that renders one
//! command's output would pass while a different command's output was doubled.
//!
//! Reading source is normally the wrong way to test behaviour. It is the right
//! way for a *house style* rule, because the rule is about the shape of the call
//! rather than about the value it produces, and because the alternative — a
//! lint, a reviewer, or nothing — is what let sixteen duplicated glyphs and one
//! bare `println!` accumulate.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// The glyph each `log_*` sink prints, and therefore the glyph a message
    /// handed to that sink must *not* also carry.
    ///
    /// Asserted against `src/utils/output.rs` in
    /// [`the_glyph_table_still_matches_the_sinks_that_own_them`] so this list
    /// cannot drift away from the thing it is describing.
    const GLYPHING_SINKS: [(&str, char); 4] = [
        ("log_info", '\u{2139}'),
        ("log_success", '\u{2705}'),
        ("log_warning", '\u{26a0}'),
        ("log_error", '\u{274c}'),
    ];

    /// **Any** status glyph, not just the one the sink happens to print.
    ///
    /// The narrower version of this rule — "the message must not repeat the
    /// sink's own glyph" — was tried first and caught nothing, because the two
    /// are different characters: the sink prints `✅` (U+2705) and the sixteen
    /// offending call sites all carried `✓` (U+2713). A rule that only catches
    /// the exact character is a rule that passes on the real defect.
    ///
    /// The wider rule is also the *correct* one, and for a reason beyond
    /// duplication. A message carrying `⛔` handed to `log_info` prints
    /// `ℹ️ ⛔ …`: two glyphs on one line, disagreeing about how serious it is,
    /// with nothing to tell a reader which is the real marker. That is the same
    /// shape as the fact/prediction glyph collision this program exists to
    /// separate, and it is not a duplication bug — it is two vocabularies
    /// colliding on one surface.
    const STATUS_GLYPHS: [char; 12] = [
        '\u{2139}',  // ℹ️ info
        '\u{2705}',  // ✅ heavy check mark — the sink's own
        '\u{2713}',  // ✓ check mark — what every offending call site actually used
        '\u{274c}',  // ❌ cross mark
        '\u{26a0}',  // ⚠ warning sign
        '\u{2717}',  // ✗ ballot x
        '\u{2718}',  // ✘ heavy ballot x
        '\u{26d4}',  // ⛔ no entry
        '\u{23f3}',  // ⏳ hourglass not done
        '\u{29d7}',  // ⧗ hourglass
        '\u{267b}',  // ♻️ recycling
        '\u{1f512}', // 🔒 locked
    ];

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Every `.rs` file under `src/`, sorted so a failure names the same file
    /// twice in a row.
    fn source_files() -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
            for entry in entries {
                let path = entry.expect("readable dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&crate_root().join("src"), &mut out);
        out.sort();
        out
    }

    /// The `log_*` call starting at `lines[start]`, as one string, or `None` if
    /// that line does not open a call to any glyphing sink.
    ///
    /// The statement is read up to its `;` rather than just its first line,
    /// because the argument is routinely wrapped:
    ///
    /// ```ignore
    /// context.log_success(&format!(
    ///     "✓ Request {} rejected successfully!",
    ///     request_id
    /// ));
    /// ```
    ///
    /// and a single-line scan would miss exactly the shape most likely to be
    /// wrong.
    fn glyphing_statement<'a>(lines: &'a [&'a str], start: usize) -> Option<(&'a str, String)> {
        let first = lines[start];
        let (method, _) = GLYPHING_SINKS
            .iter()
            .find(|(m, _)| first.contains(&format!("{m}(")))?;
        // A single-line call is *finished* on that line. Reading past its `;`
        // would sweep in the next statement's message, which is how a `log_info("")`
        // gets blamed for the `⏳` on the line below it.
        let mut window = String::from(first);
        if !first.contains(';') {
            for line in lines.iter().skip(start + 1).take(8) {
                window.push('\n');
                window.push_str(line);
                if line.contains(';') {
                    break;
                }
            }
        }
        Some((method, window))
    }

    /// The `log_*` sinks own the glyph; a message carries the words.
    ///
    /// This is a *style* rule with a real cost behind it. `log_success("✓ …")`
    /// prints `✅ ✓ …`, which reads as a rendering bug and trains the eye to skip
    /// the success lines; `log_info("⚠️  Are you sure…")` prints `ℹ️ ⚠️  Are you
    /// sure…`, which is worse, because two glyphs on one line disagree about how
    /// serious it is and there is no principled way to read that.
    #[test]
    fn no_message_handed_to_a_glyphing_sink_carries_a_status_glyph() {
        let mut offenders = Vec::new();
        for file in source_files() {
            let source = std::fs::read_to_string(&file).expect("readable source file");
            let lines: Vec<&str> = source.lines().collect();
            for (index, _) in lines.iter().enumerate() {
                let Some((method, statement)) = glyphing_statement(&lines, index) else {
                    continue;
                };
                for glyph in STATUS_GLYPHS {
                    if statement.contains(glyph) {
                        let line = source
                            .lines()
                            .nth(index)
                            .expect("index within the same file")
                            .trim();
                        offenders.push(format!(
                            "{}:{}: {method} message carries '{glyph}': {line}",
                            file.strip_prefix(crate_root()).unwrap_or(&file).display(),
                            index + 1,
                        ));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "the log_* sinks already print a glyph (see src/utils/output.rs); \
             a message must not carry its own:\n{}",
            offenders.join("\n")
        );
    }

    /// The table above is a claim about `src/utils/output.rs`, so it is checked
    /// against it.
    ///
    /// Without this, the table is just four literals in a test that would keep
    /// passing if someone changed a sink to stop printing its glyph — at which
    /// point the rule above would be banning something harmless, which is how a
    /// house-style test loses its credibility and gets deleted.
    #[test]
    fn the_glyph_table_still_matches_the_sinks_that_own_them() {
        let source = std::fs::read_to_string(crate_root().join("src/utils/output.rs"))
            .expect("src/utils/output.rs is readable");
        for (method, glyph) in GLYPHING_SINKS {
            let arm = source
                .lines()
                .find(|line| line.contains(&format!("OutputLevel::{} =>", level_of(method))))
                .unwrap_or_else(|| panic!("output.rs has no arm for {method}"));
            assert!(
                arm.contains(glyph),
                "{method} is listed as printing '{glyph}', but output.rs's arm is: {arm}"
            );
        }
        // And the one sink that is *not* in the table prints no glyph at all,
        // which is why a `log_verbose` message may carry a `✓` without breaking
        // the rule above.
        let verbose = source
            .lines()
            .find(|line| line.contains("OutputLevel::Verbose =>"))
            .expect("output.rs has a Verbose arm");
        assert!(
            !verbose.contains('\u{2139}') && !verbose.contains('\u{2705}'),
            "log_verbose was expected to be the glyph-free sink, but its arm is: {verbose}"
        );
    }

    /// `OutputLevel` variant name for a `log_*` method, e.g. `log_warning` →
    /// `Warning`.
    fn level_of(method: &str) -> &'static str {
        match method {
            "log_info" => "Info",
            "log_success" => "Success",
            "log_warning" => "Warning",
            "log_error" => "Error",
            other => panic!("{other} has no OutputLevel"),
        }
    }
}
