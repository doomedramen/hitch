//! Best-effort GitHub PR comments reporting a promoted branch's held /
//! re-included status after a rebuild.
//!
//! See docs/merge-conflict-handling-plan.md (phase 3, PR comment
//! integration). This is visibility on top of a rebuild that has already
//! succeeded — every `gh`/network failure is logged at verbose level and
//! swallowed rather than propagated, so it can never be the reason a
//! rebuild command fails.

use crate::commands::global_context::GlobalContext;
use crate::utils::gh;
use crate::utils::prelude::CompatibilityConflict;

const MARKER_PREFIX: &str = "<!-- hitch:held:";

fn marker(env_name: &str, branch: &str) -> String {
    format!("{}{}:{} -->", MARKER_PREFIX, env_name, branch)
}

/// Whether `body` is hitch's own status comment for this exact env/branch
/// pair — not just any hitch comment, since a branch promoted to more than
/// one environment gets one comment per environment.
fn is_own_marker(body: &str, env_name: &str, branch: &str) -> bool {
    body.trim_start().starts_with(&marker(env_name, branch))
}

fn held_comment_body(
    env_name: &str,
    branch: &str,
    conflicts_with: &str,
    files: &[String],
) -> String {
    let mut out = marker(env_name, branch);
    out.push_str(&format!(
        "\n⛔ **Held from `{}`**\n\n`{}` conflicts with `{}` and was excluded from the last \
         rebuild of `{}`.\n\n**Conflicting files:**\n",
        env_name, branch, conflicts_with, env_name
    ));
    for f in files {
        out.push_str(&format!("- `{}`\n", f));
    }
    out.push_str(&format!(
        "\n**Fix it:**\n```\ngit checkout {} && git rebase {}\n```\n\n\
         This comment updates automatically once `{}` composes cleanly again.\n",
        branch, conflicts_with, branch
    ));
    out
}

fn healed_comment_body(env_name: &str, branch: &str) -> String {
    format!(
        "{}\n✅ **Re-included in `{}`**\n\n`{}` composed cleanly and is back in the last \
         rebuild of `{}`.\n",
        marker(env_name, branch),
        env_name,
        branch,
        env_name
    )
}

/// Find the open PR for `branch`, if any, and hitch's own prior status
/// comment on it, if any. Returns `None` when there's no open PR at all —
/// distinct from "PR exists but no prior comment" (`Some((pr, None))`).
fn find_existing_status_comment(
    owner: &str,
    repo: &str,
    env_name: &str,
    branch: &str,
) -> Option<(u64, Option<u64>)> {
    let pr_number = gh::find_open_pr_for_branch(owner, repo, branch)
        .ok()
        .flatten()?;
    let comments = gh::list_pr_comments(owner, repo, pr_number).ok()?;
    let existing_id = comments
        .iter()
        .find(|c| is_own_marker(&c.body, env_name, branch))
        .map(|c| c.id);
    Some((pr_number, existing_id))
}

/// For every promoted branch, upsert a single PR status comment: create one
/// for a newly-held branch, update it in place on every subsequent rebuild
/// that still holds it, and flip it to "re-included" the first rebuild
/// where it composes again. Never posts a fresh "re-included" comment to a
/// PR hitch has no prior comment on — only PRs it has already spoken up on
/// get touched.
///
/// No-ops silently (no error, nothing logged above verbose) if `gh` isn't
/// on PATH or the remote isn't a GitHub repo, so this is safe to call
/// unconditionally from opt-in call sites.
pub fn report_held_status(
    context: &GlobalContext,
    env_name: &str,
    promoted_branches: &[String],
    held: &[CompatibilityConflict],
) {
    if gh::find_gh().is_none() {
        return;
    }
    let Ok((owner, repo)) = gh::owner_repo_from_remote() else {
        return;
    };

    for branch in promoted_branches {
        let held_conflict = held.iter().find(|c| &c.branch == branch);

        let Some((pr_number, existing_id)) =
            find_existing_status_comment(&owner, &repo, env_name, branch)
        else {
            continue;
        };

        if held_conflict.is_none() && existing_id.is_none() {
            continue;
        }

        let body = match held_conflict {
            Some(c) => held_comment_body(env_name, branch, &c.conflicts_with, &c.conflicted_files),
            None => healed_comment_body(env_name, branch),
        };

        let result = match existing_id {
            Some(id) => gh::update_pr_comment(&owner, &repo, id, &body),
            None => gh::create_pr_comment(&owner, &repo, pr_number, &body).map(|_| ()),
        };

        if let Err(e) = result {
            context.log_verbose(&format!(
                "Could not update PR #{} status comment for '{}': {}",
                pr_number, branch, e
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_body_contains_marker_pair_and_files() {
        let body = held_comment_body(
            "dev",
            "payments-retry",
            "billing-v2",
            &["src/billing/invoice.rs".to_string()],
        );
        assert!(body.starts_with("<!-- hitch:held:dev:payments-retry -->"));
        assert!(body.contains("conflicts with `billing-v2`"));
        assert!(body.contains("src/billing/invoice.rs"));
        assert!(body.contains("git checkout payments-retry && git rebase billing-v2"));
    }

    #[test]
    fn healed_body_contains_marker() {
        let body = healed_comment_body("dev", "payments-retry");
        assert!(body.starts_with("<!-- hitch:held:dev:payments-retry -->"));
        assert!(body.contains("Re-included in `dev`"));
    }

    #[test]
    fn marker_matches_only_own_env_and_branch() {
        let body = held_comment_body("dev", "branch-b", "branch-a", &[]);
        assert!(is_own_marker(&body, "dev", "branch-b"));
        assert!(!is_own_marker(&body, "qa", "branch-b"));
        assert!(!is_own_marker(&body, "dev", "branch-a"));
    }

    #[test]
    fn marker_does_not_match_unrelated_comment() {
        assert!(!is_own_marker(
            "just a normal review comment",
            "dev",
            "branch-b"
        ));
    }

    #[test]
    fn marker_does_not_falsely_match_branch_name_prefix_collision() {
        // "branch-b" must not match a comment marked for "branch-bx" —
        // guards against substring-prefix false positives if branch names
        // share a prefix.
        let body = held_comment_body("dev", "branch-bx", "main", &[]);
        assert!(!is_own_marker(&body, "dev", "branch-b"));
    }
}
