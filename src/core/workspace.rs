#[derive(Debug, Clone)]
pub struct BranchRow {
    pub name: String,
    pub local: bool,
    pub remote: bool,
    pub is_environment: bool,
    pub promoted_to: Vec<String>,
    pub base_for: Vec<String>,
}

impl BranchRow {
    /// The branch name Hitch commands expect (without any `origin/` prefix).
    pub fn cli_ref(&self) -> &str {
        &self.name
    }

    /// A git reference usable in `git log` / `git diff` when the branch may be remote-only.
    pub fn git_ref(&self) -> String {
        if self.local {
            self.name.clone()
        } else if self.remote {
            format!("origin/{}", self.name)
        } else {
            self.name.clone()
        }
    }
}
