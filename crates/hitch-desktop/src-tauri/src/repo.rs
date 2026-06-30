use crate::types::RepoProbeResultDto;
use std::path::Path;

fn normalize_origin_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);

    // git@github.com:owner/repo
    if let Some(rest) = trimmed.strip_prefix("git@") {
        let mut parts = rest.splitn(2, ':');
        let host = parts.next()?.trim();
        let path = parts.next()?.trim().trim_start_matches('/');
        if host.is_empty() || path.is_empty() {
            return None;
        }
        return Some(format!("{}/{}", host.to_lowercase(), path));
    }

    // ssh://git@github.com/owner/repo
    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        // rest: user@host/path or host/path
        let rest = rest
            .strip_prefix("git@")
            .unwrap_or(rest)
            .trim_start_matches('/');
        let mut parts = rest.splitn(2, '/');
        let host = parts.next()?.trim();
        let path = parts.next()?.trim();
        if host.is_empty() || path.is_empty() {
            return None;
        }
        return Some(format!("{}/{}", host.to_lowercase(), path));
    }

    // https://github.com/owner/repo
    if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
    {
        let rest = rest.trim_start_matches('/');
        let mut parts = rest.splitn(2, '/');
        let host = parts.next()?.trim();
        let path = parts.next()?.trim();
        if host.is_empty() || path.is_empty() {
            return None;
        }
        return Some(format!("{}/{}", host.to_lowercase(), path));
    }

    None
}

pub fn probe_repo(path: &str) -> RepoProbeResultDto {
    let p = Path::new(path);
    let path = p.to_string_lossy().to_string();

    let repo = match git2::Repository::open(p) {
        Ok(r) => r,
        Err(e) => {
            return RepoProbeResultDto::err(path, format!("Not a git repository: {}", e));
        }
    };

    let workdir = repo.workdir().unwrap_or(p);
    let display_name = workdir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    let origin_url_raw = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|u| u.to_string()));
    let origin_url_normalized = origin_url_raw.as_deref().and_then(normalize_origin_url);

    let root_commit_oid = match repo.revwalk() {
        Ok(mut walk) => {
            let _ = walk.push_head();
            let mut last: Option<git2::Oid> = None;
            for oid in walk.flatten() {
                last = Some(oid);
            }
            last.map(|o| o.to_string())
        }
        Err(_) => None,
    };

    RepoProbeResultDto::ok(path, display_name, origin_url_normalized, root_commit_oid)
}
