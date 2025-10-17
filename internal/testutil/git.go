//go:build dockertest

package testutil

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
)

// TestRepo represents an isolated Git repository for testing
type TestRepo struct {
	Path string
	Repo *hitchgit.Repo
	T    *testing.T
}

// NewTestRepo creates a new isolated Git repository in a temporary directory
func NewTestRepo(t *testing.T) *TestRepo {
	t.Helper()

	// Create temporary directory
	tmpDir, err := os.MkdirTemp("", "hitch-test-*")
	if err != nil {
		t.Fatalf("Failed to create temp dir: %v", err)
	}

	// Initialize Git repository with main as default branch
	cmd := exec.Command("git", "init", "--initial-branch=main")
	cmd.Dir = tmpDir
	if err := cmd.Run(); err != nil {
		os.RemoveAll(tmpDir)
		t.Fatalf("Failed to init git repo: %v", err)
	}

	// Configure git user
	configCmds := [][]string{
		{"git", "config", "user.name", "Test User"},
		{"git", "config", "user.email", "test@example.com"},
		{"git", "config", "commit.gpgsign", "false"},
	}

	for _, cmdArgs := range configCmds {
		cmd := exec.Command(cmdArgs[0], cmdArgs[1:]...)
		cmd.Dir = tmpDir
		if err := cmd.Run(); err != nil {
			os.RemoveAll(tmpDir)
			t.Fatalf("Failed to configure git: %v", err)
		}
	}

	// Create initial commit on main branch
	if err := createInitialCommit(tmpDir); err != nil {
		os.RemoveAll(tmpDir)
		t.Fatalf("Failed to create initial commit: %v", err)
	}

	// Open as Hitch repo
	repo, err := hitchgit.OpenRepo(tmpDir)
	if err != nil {
		os.RemoveAll(tmpDir)
		t.Fatalf("Failed to open repo: %v", err)
	}

	tr := &TestRepo{
		Path: tmpDir,
		Repo: repo,
		T:    t,
	}

	// Register cleanup
	t.Cleanup(func() {
		tr.Cleanup()
	})

	return tr
}

// Cleanup removes the test repository
func (tr *TestRepo) Cleanup() {
	if tr.Path != "" {
		os.RemoveAll(tr.Path)
	}
}

// CreateBranch creates a new branch with an optional commit
func (tr *TestRepo) CreateBranch(name string, createCommit bool) error {
	tr.T.Helper()

	if err := tr.Repo.CreateBranch(name, "main"); err != nil {
		return err
	}

	if createCommit {
		if err := tr.Repo.Checkout(name); err != nil {
			return err
		}

		// Create a file and commit it
		// Replace slashes in branch name to avoid directory issues
		fileName := strings.ReplaceAll(name, "/", "-") + ".txt"
		filePath := filepath.Join(tr.Path, fileName)
		if err := os.WriteFile(filePath, []byte("test content"), 0644); err != nil {
			return err
		}

		cmd := exec.Command("git", "add", ".")
		cmd.Dir = tr.Path
		if err := cmd.Run(); err != nil {
			return err
		}

		cmd = exec.Command("git", "commit", "-m", "Add "+name)
		cmd.Dir = tr.Path
		if err := cmd.Run(); err != nil {
			return err
		}

		// Return to main
		if err := tr.Repo.Checkout("main"); err != nil {
			return err
		}
	}

	return nil
}

// CommitFile creates a file and commits it to the current branch
func (tr *TestRepo) CommitFile(filename, content, message string) error {
	tr.T.Helper()

	filePath := filepath.Join(tr.Path, filename)
	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		return err
	}

	cmd := exec.Command("git", "add", filename)
	cmd.Dir = tr.Path
	if err := cmd.Run(); err != nil {
		return err
	}

	cmd = exec.Command("git", "commit", "-m", message)
	cmd.Dir = tr.Path
	return cmd.Run()
}

// GetCurrentBranch returns the current branch name
func (tr *TestRepo) GetCurrentBranch() (string, error) {
	return tr.Repo.CurrentBranch()
}

// BranchExists checks if a branch exists
func (tr *TestRepo) BranchExists(name string) bool {
	return tr.Repo.BranchExists(name)
}

// createInitialCommit creates an initial commit in the repository
func createInitialCommit(repoPath string) error {
	// Create README
	readmePath := filepath.Join(repoPath, "README.md")
	if err := os.WriteFile(readmePath, []byte("# Test Repository\n"), 0644); err != nil {
		return err
	}

	// Add and commit
	cmd := exec.Command("git", "add", ".")
	cmd.Dir = repoPath
	if err := cmd.Run(); err != nil {
		return err
	}

	cmd = exec.Command("git", "commit", "-m", "Initial commit")
	cmd.Dir = repoPath
	return cmd.Run()
}
