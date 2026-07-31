//! Unit tests for the refname firewall in `utils::validation`.

use hitch::utils::validation::validate_name;

#[cfg(test)]
mod tests {
    use super::*;

    /// Names that must be accepted — ordinary branch names people really use.
    #[test]
    fn test_validate_name_accepts_ordinary_branches() {
        for name in [
            "main",
            "dev",
            "feature/login",
            "release-1.2.3",
            "user/martin/fix_thing",
            "v2",
        ] {
            assert!(
                validate_name(name, "Branch").is_ok(),
                "expected '{}' to be accepted",
                name
            );
        }
    }

    /// Names that must be rejected. Each is a real git-illegal or
    /// argv-hostile form; a rejection here is what stops hitch.json from
    /// steering a git command line.
    #[test]
    fn test_validate_name_rejects_hostile_names() {
        for name in [
            "",                             // empty
            "-fdx",                         // option-shaped
            "--upload-pack=touch /tmp/pwn", // option injection
            "feature/a..b",                 // double dot
            "feature/a b",                  // space
            "feature/a\tb",                 // tab
            "feature/a\nb",                 // newline
            "feature/a\u{7f}b",             // DEL
            "feature/a\u{1b}[31m",          // ANSI escape
            "feature/x.lock",               // .lock suffix
            "feature/.hidden",              // dot-leading component
            "@",                            // bare @
            "feature/a@{0}",                // @{ sequence
            "feature/a:b",                  // colon
            "/leading",                     // leading slash
            "trailing/",                    // trailing slash
            "double//slash",                // consecutive slashes
        ] {
            assert!(
                validate_name(name, "Branch").is_err(),
                "expected '{}' to be rejected",
                name.escape_debug()
            );
        }
    }
}
