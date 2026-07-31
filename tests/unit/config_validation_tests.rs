//! Unit tests for untrusted hitch.json parsing.

use hitch::utils::config_validation::{
    parse_untrusted_config, MAX_BRANCHES_PER_ENV, MAX_CONFIG_BYTES, MAX_ENVIRONMENTS,
};

#[cfg(test)]
mod tests {
    use super::*;

    // `Environment::locked` has no `#[serde(default)]` (unlike its sibling
    // `locked_by`/`locked_at`, which are `Option` and so are implicitly
    // optional under serde), so a minimal fixture must supply it explicitly
    // or `serde_json::from_str` fails with "missing field `locked`" before
    // parse_untrusted_config's own checks ever run.
    fn config_with_branch(branch: &str) -> String {
        format!(
            r#"{{"version":"1.0","environments":{{"dev":{{"base":"main","branches":["{}"],"locked":false}}}}}}"#,
            branch
        )
    }

    #[test]
    fn test_parse_untrusted_config_accepts_ordinary_config() {
        let config = parse_untrusted_config(&config_with_branch("feature/login"))
            .expect("ordinary config must parse");
        assert_eq!(config.environments.len(), 1);
        assert_eq!(config.environments["dev"].branches, vec!["feature/login"]);
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_branch() {
        let err = parse_untrusted_config(&config_with_branch("--upload-pack=touch /tmp/pwn"))
            .expect_err("option-shaped branch must be rejected");
        assert!(
            err.to_string().contains("hitch.json"),
            "error must name hitch.json so the user knows where to look: {}",
            err
        );
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_base() {
        let raw = r#"{"version":"1.0","environments":{"dev":{"base":"-fdx","branches":[],"locked":false}}}"#;
        let err = parse_untrusted_config(raw).expect_err("option-shaped base must be rejected");
        assert!(
            err.to_string().contains("unusable base branch"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_parse_untrusted_config_rejects_option_shaped_env_name() {
        let raw = r#"{"version":"1.0","environments":{"--exec=x":{"base":"main","branches":[],"locked":false}}}"#;
        let err = parse_untrusted_config(raw).expect_err("option-shaped env name must be rejected");
        assert!(
            err.to_string().contains("unusable environment name"),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_parse_untrusted_config_rejects_oversized_input() {
        let raw = format!(
            r#"{{"version":"1.0","environments":{{}},"_pad":"{}"}}"#,
            "a".repeat(MAX_CONFIG_BYTES)
        );
        let err = parse_untrusted_config(&raw).expect_err("oversized config must be rejected");
        assert!(err.to_string().contains("too large"), "got: {}", err);
    }

    #[test]
    fn test_parse_untrusted_config_rejects_too_many_environments() {
        let envs_at_cap: Vec<String> = (0..MAX_ENVIRONMENTS)
            .map(|i| {
                format!(
                    r#""env{}":{{"base":"main","branches":[],"locked":false}}"#,
                    i
                )
            })
            .collect();
        let raw_at_cap = format!(
            r#"{{"version":"1.0","environments":{{{}}}}}"#,
            envs_at_cap.join(",")
        );
        parse_untrusted_config(&raw_at_cap).expect("exactly MAX_ENVIRONMENTS must be accepted");

        let envs: Vec<String> = (0..=MAX_ENVIRONMENTS)
            .map(|i| {
                format!(
                    r#""env{}":{{"base":"main","branches":[],"locked":false}}"#,
                    i
                )
            })
            .collect();
        let raw = format!(
            r#"{{"version":"1.0","environments":{{{}}}}}"#,
            envs.join(",")
        );
        let err = parse_untrusted_config(&raw).expect_err("over-cap environments must be rejected");
        assert!(
            err.to_string()
                .contains(&format!("declares {} environments", MAX_ENVIRONMENTS + 1)),
            "got: {}",
            err
        );
    }

    #[test]
    fn test_parse_untrusted_config_rejects_too_many_branches() {
        let branches_at_cap: Vec<String> = (0..MAX_BRANCHES_PER_ENV)
            .map(|i| format!(r#""feature/b{}""#, i))
            .collect();
        let raw_at_cap = format!(
            r#"{{"version":"1.0","environments":{{"dev":{{"base":"main","branches":[{}],"locked":false}}}}}}"#,
            branches_at_cap.join(",")
        );
        parse_untrusted_config(&raw_at_cap).expect("exactly MAX_BRANCHES_PER_ENV must be accepted");

        let branches: Vec<String> = (0..=MAX_BRANCHES_PER_ENV)
            .map(|i| format!(r#""feature/b{}""#, i))
            .collect();
        let raw = format!(
            r#"{{"version":"1.0","environments":{{"dev":{{"base":"main","branches":[{}],"locked":false}}}}}}"#,
            branches.join(",")
        );
        let err = parse_untrusted_config(&raw).expect_err("over-cap branches must be rejected");
        assert!(
            err.to_string()
                .contains(&format!("{} promoted branches", MAX_BRANCHES_PER_ENV + 1)),
            "got: {}",
            err
        );
    }
}
