//! The hardened parser for bytes read off the `hitch-metadata` branch,
//! used by every *mutating* code path (rebuild, promote, demote, release,
//! approve, ...) that turns `hitch.json` into a `HitchConfig`.
//!
//! This is NOT the only place `hitch.json` gets parsed — `src/core/timeline.rs`
//! (`read_config_at`, reading historical configs from arbitrary past commits
//! for `hitch timeline`) and `src/commands/status.rs` (`display_diff`, reading
//! both the committed config and a cwd-local `hitch.json` for a diff preview)
//! both do a bare `serde_json::from_str` instead. That's a real gap — the size
//! cap and ref-shape checks below don't apply there — left deliberately
//! unrouted for now because `timeline.rs` reads configs written by
//! arbitrarily old versions of hitch, which may have looser name validation
//! than `validate_name` currently enforces; routing it through here risked
//! breaking `hitch timeline` on real repositories' real history without
//! evidence either way. Revisit if either site becomes a demonstrated attack
//! surface (`status.rs`'s cwd-local read is the more plausible candidate,
//! since nothing has validated that file yet).
//!
//! `hitch-metadata` is a normal branch: anyone who can push to the repository
//! can rewrite `hitch.json`, and every environment name, base branch and
//! promoted branch in it ends up in a `git` argv on every machine that runs a
//! rebuild. So parsing what mutating commands consume is a trust boundary,
//! not a convenience parser — bound the input before parsing it, and prove
//! every name is ref-shaped before it can steer a command line.
//!
//! Deliberately NOT `#[serde(deny_unknown_fields)]`: `HitchConfig`'s forward
//! compatibility story (`check_write_compatibility`) depends on a newer config
//! still being *readable* by an older binary — denying unknown fields would
//! turn "cannot safely rewrite" into "cannot read at all".

use crate::types::HitchConfig;
use crate::utils::validation::validate_name;
use anyhow::{Context, Result};

/// Refuse to parse a config larger than this. A rebuild reads it on every
/// invocation; there is no legitimate megabyte-scale hitch.json.
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// Upper bounds on the shape of a config. These are not business limits — they
/// are the point past which a crafted config is a denial-of-service rather than
/// a configuration.
pub const MAX_ENVIRONMENTS: usize = 256;
pub const MAX_BRANCHES_PER_ENV: usize = 1024;

/// Parse `raw` as a `hitch.json` that may have been written by anyone.
///
/// Bounds size first, then parses, then proves every name is ref-shaped, then
/// runs the ordinary semantic validation. Every error names `hitch.json` and
/// ends with the command to run next.
pub fn parse_untrusted_config(raw: &str) -> Result<HitchConfig> {
    if raw.len() > MAX_CONFIG_BYTES {
        return Err(anyhow::anyhow!(
            "hitch.json is too large ({} bytes, limit {}). This is not a configuration \
             hitch wrote. Inspect it with:\n  git show hitch-metadata:hitch.json | head",
            raw.len(),
            MAX_CONFIG_BYTES
        ));
    }

    let config: HitchConfig = serde_json::from_str(raw).context(
        "Failed to parse hitch.json from the hitch-metadata branch. Inspect it with:\n  \
         git show hitch-metadata:hitch.json",
    )?;

    if config.environments.len() > MAX_ENVIRONMENTS {
        return Err(anyhow::anyhow!(
            "hitch.json declares {} environments (limit {}). Inspect it with:\n  \
             git show hitch-metadata:hitch.json",
            config.environments.len(),
            MAX_ENVIRONMENTS
        ));
    }

    for (env_name, env) in &config.environments {
        validate_name(env_name, "Environment").context(
            "hitch.json contains an unusable environment name. Fix it on the \
             hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
        )?;

        validate_name(&env.base, "Base branch").with_context(|| {
            format!(
                "hitch.json gives environment '{}' an unusable base branch. Fix it on the \
                 hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
                env_name
            )
        })?;

        if env.branches.len() > MAX_BRANCHES_PER_ENV {
            return Err(anyhow::anyhow!(
                "hitch.json gives environment '{}' {} promoted branches (limit {}). \
                 Inspect it with:\n  git show hitch-metadata:hitch.json",
                env_name,
                env.branches.len(),
                MAX_BRANCHES_PER_ENV
            ));
        }

        for branch in &env.branches {
            validate_name(branch, "Branch").with_context(|| {
                format!(
                    "hitch.json gives environment '{}' an unusable promoted branch. Fix it on \
                     the hitch-metadata branch:\n  git show hitch-metadata:hitch.json",
                    env_name
                )
            })?;
        }
    }

    if let Err(validation_error) = config.validate() {
        return Err(anyhow::anyhow!(
            "hitch.json failed validation: {}. Inspect it with:\n  \
             git show hitch-metadata:hitch.json",
            validation_error
        ));
    }

    Ok(config)
}
