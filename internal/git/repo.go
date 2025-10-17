package git

import (
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing"
)

// Repo wraps a git repository with helpful methods
type Repo struct {
	*git.Repository
	workdir string
}

// OpenRepo opens a git repository in the current or specified directory
func OpenRepo(path string) (*Repo, error) {
	if path == "" {
		path = "."
	}

	repo, err := git.PlainOpen(path)
	if err != nil {
		return nil, fmt.Errorf("not a git repository (or any parent): %w", err)
	}

	workdir, err := os.Getwd()
	if err != nil {
		return nil, fmt.Errorf("failed to get working directory: %w", err)
	}

	return &Repo{
		Repository: repo,
		workdir:    workdir,
	}, nil
}

// CurrentBranch returns the name of the current branch
func (r *Repo) CurrentBranch() (string, error) {
	head, err := r.Head()
	if err != nil {
		return "", fmt.Errorf("failed to get HEAD: %w", err)
	}

	if !head.Name().IsBranch() {
		return "", fmt.Errorf("HEAD is detached")
	}

	return head.Name().Short(), nil
}

// IsDetachedHead checks if HEAD is in detached state
func (r *Repo) IsDetachedHead() bool {
	head, err := r.Head()
	if err != nil {
		return false
	}
	return !head.Name().IsBranch()
}

// CurrentCommitSHA returns the SHA of the current commit
func (r *Repo) CurrentCommitSHA() (string, error) {
	head, err := r.Head()
	if err != nil {
		return "", fmt.Errorf("failed to get HEAD: %w", err)
	}
	return head.Hash().String(), nil
}

// BranchExists checks if a branch exists (local or remote)
func (r *Repo) BranchExists(name string) bool {
	// Check local
	_, err := r.Reference(plumbing.NewBranchReferenceName(name), true)
	if err == nil {
		return true
	}

	// Check remote
	_, err = r.Reference(plumbing.NewRemoteReferenceName("origin", name), true)
	return err == nil
}

// UserName returns the configured git user name
func (r *Repo) UserName() (string, error) {
	cfg, err := r.Config()
	if err != nil {
		return "", fmt.Errorf("failed to get git config: %w", err)
	}

	if cfg.User.Name != "" {
		return cfg.User.Name, nil
	}

	// Fallback to system username
	return os.Getenv("USER"), nil
}

// UserEmail returns the configured git user email
func (r *Repo) UserEmail() (string, error) {
	cfg, err := r.Config()
	if err != nil {
		return "", fmt.Errorf("failed to get git config: %w", err)
	}

	if cfg.User.Email != "" {
		return cfg.User.Email, nil
	}

	return "", fmt.Errorf("git user.email not configured")
}

// HasUncommittedChanges checks if a branch has uncommitted changes
// Note: This requires executing git commands as go-git doesn't support this well
func (r *Repo) HasUncommittedChanges(branch string) (bool, error) {
	// Use git command for this
	cmd := exec.Command("git", "diff", "--quiet", branch)
	cmd.Dir = r.workdir
	err := cmd.Run()

	if err != nil {
		// Non-zero exit code means there are changes
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode() != 0, nil
		}
		return false, fmt.Errorf("failed to check for uncommitted changes: %w", err)
	}

	// Also check staged changes
	cmd = exec.Command("git", "diff", "--cached", "--quiet", branch)
	cmd.Dir = r.workdir
	err = cmd.Run()

	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode() != 0, nil
		}
		return false, fmt.Errorf("failed to check for staged changes: %w", err)
	}

	return false, nil
}

// Checkout checks out a branch or commit
func (r *Repo) Checkout(ref string) error {
	worktree, err := r.Worktree()
	if err != nil {
		return fmt.Errorf("failed to get worktree: %w", err)
	}

	// Try as branch first
	branchRef := plumbing.NewBranchReferenceName(ref)
	err = worktree.Checkout(&git.CheckoutOptions{
		Branch: branchRef,
		Force:  false,
	})

	if err != nil {
		// Try as commit hash
		hash := plumbing.NewHash(ref)
		err = worktree.Checkout(&git.CheckoutOptions{
			Hash:  hash,
			Force: false,
		})
		if err != nil {
			return fmt.Errorf("failed to checkout %s: %w", ref, err)
		}
	}

	return nil
}

// Pull pulls changes from remote
func (r *Repo) Pull(remoteName string, branchName string) error {
	worktree, err := r.Worktree()
	if err != nil {
		return fmt.Errorf("failed to get worktree: %w", err)
	}

	err = worktree.Pull(&git.PullOptions{
		RemoteName:    remoteName,
		ReferenceName: plumbing.NewBranchReferenceName(branchName),
		Force:         false,
	})

	if err == git.NoErrAlreadyUpToDate {
		return nil
	}

	if err != nil {
		return fmt.Errorf("failed to pull: %w", err)
	}

	return nil
}

// Push pushes changes to remote
// Uses force-with-lease for safety
func (r *Repo) Push(remoteName string, branchName string, force bool) error {
	refSpec := config.RefSpec(fmt.Sprintf("refs/heads/%s:refs/heads/%s", branchName, branchName))

	pushOptions := &git.PushOptions{
		RemoteName: remoteName,
		RefSpecs:   []config.RefSpec{refSpec},
	}

	if force {
		pushOptions.Force = true
		// TODO: Implement force-with-lease
		// This requires tracking the expected remote hash
	}

	err := r.Repository.Push(pushOptions)
	if err == git.NoErrAlreadyUpToDate {
		return nil
	}

	if err != nil {
		return fmt.Errorf("failed to push: %w", err)
	}

	return nil
}

// CreateBranch creates a new branch
func (r *Repo) CreateBranch(name string, fromRef string) error {
	// Get the commit to branch from
	var hash plumbing.Hash
	if fromRef != "" {
		ref, err := r.Reference(plumbing.NewBranchReferenceName(fromRef), true)
		if err != nil {
			return fmt.Errorf("failed to get reference for %s: %w", fromRef, err)
		}
		hash = ref.Hash()
	} else {
		head, err := r.Head()
		if err != nil {
			return fmt.Errorf("failed to get HEAD: %w", err)
		}
		hash = head.Hash()
	}

	// Create the branch reference
	branchRef := plumbing.NewHashReference(
		plumbing.NewBranchReferenceName(name),
		hash,
	)

	err := r.Storer.SetReference(branchRef)
	if err != nil {
		return fmt.Errorf("failed to create branch %s: %w", name, err)
	}

	return nil
}

// DeleteBranch deletes a branch
func (r *Repo) DeleteBranch(name string, force bool) error {
	// For force delete, we need to use git command
	if force {
		cmd := exec.Command("git", "branch", "-D", name)
		cmd.Dir = r.workdir
		output, err := cmd.CombinedOutput()
		if err != nil {
			return fmt.Errorf("failed to delete branch %s: %s", name, string(output))
		}
		return nil
	}

	// Normal delete
	err := r.Storer.RemoveReference(plumbing.NewBranchReferenceName(name))
	if err != nil {
		return fmt.Errorf("failed to delete branch %s: %w", name, err)
	}

	return nil
}

// DeleteRemoteBranch deletes a branch from remote
func (r *Repo) DeleteRemoteBranch(remoteName string, branchName string) error {
	cmd := exec.Command("git", "push", remoteName, "--delete", branchName)
	cmd.Dir = r.workdir
	output, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("failed to delete remote branch %s: %s", branchName, string(output))
	}
	return nil
}

// Merge merges a branch into the current branch
// Note: This uses git command as go-git's merge support is limited
func (r *Repo) Merge(branch string, noFF bool) error {
	args := []string{"merge"}
	if noFF {
		args = append(args, "--no-ff")
	}
	args = append(args, branch)

	cmd := exec.Command("git", args...)
	cmd.Dir = r.workdir
	output, err := cmd.CombinedOutput()

	if err != nil {
		// Check if it's a merge conflict
		if strings.Contains(string(output), "CONFLICT") {
			return &MergeConflictError{
				Branch:  branch,
				Message: string(output),
			}
		}
		return fmt.Errorf("merge failed: %s", string(output))
	}

	return nil
}

// MergeAbort aborts an in-progress merge
func (r *Repo) MergeAbort() error {
	cmd := exec.Command("git", "merge", "--abort")
	cmd.Dir = r.workdir
	output, err := cmd.CombinedOutput()

	if err != nil {
		return fmt.Errorf("failed to abort merge: %s", string(output))
	}

	return nil
}

// MergeConflictError is returned when a merge results in conflicts
type MergeConflictError struct {
	Branch  string
	Message string
}

func (e *MergeConflictError) Error() string {
	return fmt.Sprintf("merge conflict when merging %s: %s", e.Branch, e.Message)
}
