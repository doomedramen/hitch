//go:build dockertest

package git_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/testutil"
)

func TestOpenRepo(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Test opening the repository
	repo, err := git.OpenRepo(testRepo.Path)
	if err != nil {
		t.Fatalf("Failed to open repository: %v", err)
	}

	if repo == nil {
		t.Fatal("Expected repo to be non-nil")
	}
}

func TestOpenRepoNonExistent(t *testing.T) {
	// Try to open a non-existent repository
	_, err := git.OpenRepo("/tmp/nonexistent-repo-hitch-test")
	if err == nil {
		t.Fatal("Expected error when opening non-existent repo")
	}
}

func TestCurrentBranch(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	branch, err := testRepo.Repo.CurrentBranch()
	if err != nil {
		t.Fatalf("Failed to get current branch: %v", err)
	}

	// testutil creates repos on main branch
	if branch != "main" {
		t.Errorf("Expected current branch to be 'main', got '%s'", branch)
	}
}

func TestIsDetachedHead(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Initially should not be detached
	if testRepo.Repo.IsDetachedHead() {
		t.Error("HEAD should not be detached initially")
	}

	// Get current commit SHA
	sha, err := testRepo.Repo.CurrentCommitSHA()
	if err != nil {
		t.Fatalf("Failed to get commit SHA: %v", err)
	}

	// Checkout the commit directly (detached HEAD)
	err = testRepo.Repo.Checkout(sha)
	if err != nil {
		t.Fatalf("Failed to checkout commit: %v", err)
	}

	// Now should be detached
	if !testRepo.Repo.IsDetachedHead() {
		t.Error("HEAD should be detached after checking out commit")
	}
}

func TestCurrentCommitSHA(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	sha, err := testRepo.Repo.CurrentCommitSHA()
	if err != nil {
		t.Fatalf("Failed to get commit SHA: %v", err)
	}

	// SHA should be 40 characters (hex)
	if len(sha) != 40 {
		t.Errorf("Expected SHA to be 40 characters, got %d", len(sha))
	}
}

func TestBranchExists(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// main branch should exist
	if !testRepo.Repo.BranchExists("main") {
		t.Error("Expected main branch to exist")
	}

	// Non-existent branch should not exist
	if testRepo.Repo.BranchExists("nonexistent-branch") {
		t.Error("Expected nonexistent-branch to not exist")
	}

	// Create a new branch
	err := testRepo.CreateBranch("test-branch", true)
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	// New branch should exist
	if !testRepo.Repo.BranchExists("test-branch") {
		t.Error("Expected test-branch to exist after creation")
	}
}

func TestUserName(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	name, err := testRepo.Repo.UserName()
	if err != nil {
		t.Fatalf("Failed to get user name: %v", err)
	}

	// testutil configures git with "Test User"
	if name != "Test User" {
		t.Errorf("Expected user name 'Test User', got '%s'", name)
	}
}

func TestUserEmail(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	email, err := testRepo.Repo.UserEmail()
	if err != nil {
		t.Fatalf("Failed to get user email: %v", err)
	}

	// testutil configures git with "test@example.com"
	if email != "test@example.com" {
		t.Errorf("Expected user email 'test@example.com', got '%s'", email)
	}
}

func TestCreateBranch(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a branch from main
	err := testRepo.Repo.CreateBranch("feature/new-feature", "main")
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	// Verify branch exists
	if !testRepo.Repo.BranchExists("feature/new-feature") {
		t.Error("Branch should exist after creation")
	}
}

func TestCreateBranchFromCurrent(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a branch from current HEAD (empty fromRef)
	err := testRepo.Repo.CreateBranch("feature/from-head", "")
	if err != nil {
		t.Fatalf("Failed to create branch from HEAD: %v", err)
	}

	// Verify branch exists
	if !testRepo.Repo.BranchExists("feature/from-head") {
		t.Error("Branch should exist after creation from HEAD")
	}
}

func TestCheckout(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a new branch
	err := testRepo.CreateBranch("feature/checkout-test", true)
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	// Checkout the branch
	err = testRepo.Repo.Checkout("feature/checkout-test")
	if err != nil {
		t.Fatalf("Failed to checkout branch: %v", err)
	}

	// Verify we're on the new branch
	currentBranch, err := testRepo.Repo.CurrentBranch()
	if err != nil {
		t.Fatalf("Failed to get current branch: %v", err)
	}

	if currentBranch != "feature/checkout-test" {
		t.Errorf("Expected current branch to be 'feature/checkout-test', got '%s'", currentBranch)
	}
}

func TestCheckoutNonExistent(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Try to checkout a non-existent branch
	err := testRepo.Repo.Checkout("nonexistent-branch")
	if err == nil {
		t.Fatal("Expected error when checking out non-existent branch")
	}
}

func TestDeleteBranch(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a branch
	err := testRepo.CreateBranch("feature/delete-me", true)
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	// Verify it exists
	if !testRepo.Repo.BranchExists("feature/delete-me") {
		t.Fatal("Branch should exist before deletion")
	}

	// Switch back to main (can't delete current branch)
	err = testRepo.Repo.Checkout("main")
	if err != nil {
		t.Fatalf("Failed to checkout main: %v", err)
	}

	// Delete the branch
	err = testRepo.Repo.DeleteBranch("feature/delete-me", false)
	if err != nil {
		t.Fatalf("Failed to delete branch: %v", err)
	}

	// Verify it's gone
	if testRepo.Repo.BranchExists("feature/delete-me") {
		t.Error("Branch should not exist after deletion")
	}
}

func TestDeleteBranchForce(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create two branches
	err := testRepo.CreateBranch("feature/force-delete-1", true)
	if err != nil {
		t.Fatalf("Failed to create first branch: %v", err)
	}

	err = testRepo.CreateBranch("feature/force-delete-2", true)
	if err != nil {
		t.Fatalf("Failed to create second branch: %v", err)
	}

	// Switch back to main
	err = testRepo.Repo.Checkout("main")
	if err != nil {
		t.Fatalf("Failed to checkout main: %v", err)
	}

	// Force delete both branches
	err = testRepo.Repo.DeleteBranch("feature/force-delete-1", true)
	if err != nil {
		t.Fatalf("Failed to force delete first branch: %v", err)
	}

	err = testRepo.Repo.DeleteBranch("feature/force-delete-2", true)
	if err != nil {
		t.Fatalf("Failed to force delete second branch: %v", err)
	}

	// Verify they're gone
	if testRepo.Repo.BranchExists("feature/force-delete-1") {
		t.Error("First branch should not exist after force deletion")
	}

	if testRepo.Repo.BranchExists("feature/force-delete-2") {
		t.Error("Second branch should not exist after force deletion")
	}
}

func TestMerge(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a feature branch
	err := testRepo.Repo.CreateBranch("feature/merge-test", "main")
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	// Checkout the feature branch and add a commit
	err = testRepo.Repo.Checkout("feature/merge-test")
	if err != nil {
		t.Fatalf("Failed to checkout feature branch: %v", err)
	}

	err = testRepo.CommitFile("merge-test.txt", "merge test content", "Add merge test file")
	if err != nil {
		t.Fatalf("Failed to commit on feature branch: %v", err)
	}

	// Checkout main
	err = testRepo.Repo.Checkout("main")
	if err != nil {
		t.Fatalf("Failed to checkout main: %v", err)
	}

	// Get commit SHA before merge
	beforeSHA, err := testRepo.Repo.CurrentCommitSHA()
	if err != nil {
		t.Fatalf("Failed to get commit SHA: %v", err)
	}

	// Merge the feature branch
	err = testRepo.Repo.Merge("feature/merge-test", "Merge feature/merge-test")
	if err != nil {
		t.Fatalf("Failed to merge branch: %v", err)
	}

	// Get commit SHA after merge
	afterSHA, err := testRepo.Repo.CurrentCommitSHA()
	if err != nil {
		t.Fatalf("Failed to get commit SHA: %v", err)
	}

	// SHA should have changed
	if beforeSHA == afterSHA {
		t.Error("Expected commit SHA to change after merge")
	}

	// File from feature branch should exist
	mergedFile := filepath.Join(testRepo.Path, "merge-test.txt")
	if _, err := os.Stat(mergedFile); os.IsNotExist(err) {
		t.Error("merge-test.txt should exist after merge")
	}
}

func TestMergeConflict(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a file on main
	conflictFile := filepath.Join(testRepo.Path, "conflict.txt")
	err := os.WriteFile(conflictFile, []byte("main content\n"), 0644)
	if err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	err = testRepo.CommitFile("conflict.txt", "main content\n", "Add conflict.txt on main")
	if err != nil {
		t.Fatalf("Failed to commit on main: %v", err)
	}

	// Create a feature branch from before the main commit
	// First, get the parent commit
	cmd := testRepo.Repo.Checkout("HEAD~1")
	if cmd != nil {
		// If we can't go back, skip this test
		t.Skip("Cannot create conflict scenario in this repo")
		return
	}

	err = testRepo.Repo.CreateBranch("feature/conflict", "")
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	err = testRepo.Repo.Checkout("feature/conflict")
	if err != nil {
		t.Fatalf("Failed to checkout feature branch: %v", err)
	}

	// Create conflicting content
	err = os.WriteFile(conflictFile, []byte("feature content\n"), 0644)
	if err != nil {
		t.Fatalf("Failed to write conflicting content: %v", err)
	}

	err = testRepo.CommitFile("conflict.txt", "feature content\n", "Add conflict.txt on feature")
	if err != nil {
		t.Fatalf("Failed to commit on feature: %v", err)
	}

	// Go back to main
	err = testRepo.Repo.Checkout("main")
	if err != nil {
		t.Fatalf("Failed to checkout main: %v", err)
	}

	// Try to merge - should conflict
	err = testRepo.Repo.Merge("feature/conflict", "Merge with conflict")

	// Should get a MergeConflictError
	if err == nil {
		t.Fatal("Expected merge conflict error")
	}

	_, isMergeConflict := err.(*git.MergeConflictError)
	if !isMergeConflict {
		t.Errorf("Expected MergeConflictError, got %T: %v", err, err)
	}

	// Abort the merge
	err = testRepo.Repo.MergeAbort()
	if err != nil {
		t.Fatalf("Failed to abort merge: %v", err)
	}
}

func TestMergeSquash(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// Create a feature branch
	err := testRepo.Repo.CreateBranch("feature/squash-test", "main")
	if err != nil {
		t.Fatalf("Failed to create branch: %v", err)
	}

	err = testRepo.Repo.Checkout("feature/squash-test")
	if err != nil {
		t.Fatalf("Failed to checkout feature branch: %v", err)
	}

	// Make multiple commits
	err = testRepo.CommitFile("file1.txt", "content 1", "Commit 1")
	if err != nil {
		t.Fatalf("Failed to make commit 1: %v", err)
	}

	err = testRepo.CommitFile("file2.txt", "content 2", "Commit 2")
	if err != nil {
		t.Fatalf("Failed to make commit 2: %v", err)
	}

	// Checkout main
	err = testRepo.Repo.Checkout("main")
	if err != nil {
		t.Fatalf("Failed to checkout main: %v", err)
	}

	// Squash merge
	err = testRepo.Repo.MergeSquash("feature/squash-test", "Squashed feature commits")
	if err != nil {
		t.Fatalf("Failed to squash merge: %v", err)
	}

	// Verify files exist
	file1 := filepath.Join(testRepo.Path, "file1.txt")
	file2 := filepath.Join(testRepo.Path, "file2.txt")

	if _, err := os.Stat(file1); os.IsNotExist(err) {
		t.Error("file1.txt should exist after squash merge")
	}

	if _, err := os.Stat(file2); os.IsNotExist(err) {
		t.Error("file2.txt should exist after squash merge")
	}
}

func TestHasUncommittedChanges(t *testing.T) {
	testRepo := testutil.NewTestRepo(t)

	// On main branch, should have no uncommitted changes
	hasChanges, err := testRepo.Repo.HasUncommittedChanges("main")
	if err != nil {
		t.Fatalf("Failed to check for uncommitted changes: %v", err)
	}

	if hasChanges {
		t.Error("Should not have uncommitted changes initially")
	}

	// Modify an existing tracked file (README.md exists from NewTestRepo)
	readmeFile := filepath.Join(testRepo.Path, "README.md")
	err = os.WriteFile(readmeFile, []byte("# Modified content\n"), 0644)
	if err != nil {
		t.Fatalf("Failed to modify file: %v", err)
	}

	// Now should detect uncommitted changes
	hasChanges, err = testRepo.Repo.HasUncommittedChanges("main")
	if err != nil {
		t.Fatalf("Failed to check for uncommitted changes: %v", err)
	}

	if !hasChanges {
		t.Error("Should detect modified tracked file in working directory")
	}
}

func TestMergeConflictErrorMessage(t *testing.T) {
	err := &git.MergeConflictError{
		Branch:  "feature/test",
		Message: "CONFLICT (content): Merge conflict in file.txt",
	}

	expectedMsg := "merge conflict when merging feature/test: CONFLICT (content): Merge conflict in file.txt"
	if err.Error() != expectedMsg {
		t.Errorf("Expected error message '%s', got '%s'", expectedMsg, err.Error())
	}
}
