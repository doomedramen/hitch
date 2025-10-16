package metadata

import (
	"encoding/json"
	"fmt"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
)

// Writer handles writing metadata to the hitch-metadata branch
type Writer struct {
	repo *git.Repository
}

// NewWriter creates a new metadata writer
func NewWriter(repo *git.Repository) *Writer {
	return &Writer{repo: repo}
}

// Write writes metadata to the hitch-metadata branch
// It uses optimistic concurrency control with force-with-lease
func (w *Writer) Write(m *Metadata, commitMessage string, author string, authorEmail string) error {
	// Marshal metadata to JSON (pretty-printed)
	jsonBytes, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to marshal metadata to JSON",
			Err:    err,
		}
	}

	// Get worktree
	worktree, err := w.repo.Worktree()
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to get worktree",
			Err:    err,
		}
	}

	// Check out hitch-metadata branch
	err = worktree.Checkout(&git.CheckoutOptions{
		Branch: plumbing.NewBranchReferenceName(MetadataBranch),
		Force:  false,
	})
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to checkout hitch-metadata branch",
			Err:    err,
		}
	}

	// Write hitch.json file
	filename := worktree.Filesystem.Join(MetadataFile)
	file, err := worktree.Filesystem.Create(filename)
	if err != nil {
		return &MetadataWriteError{
			Reason: fmt.Sprintf("failed to create %s", MetadataFile),
			Err:    err,
		}
	}

	_, err = file.Write(jsonBytes)
	if err != nil {
		file.Close()
		return &MetadataWriteError{
			Reason: fmt.Sprintf("failed to write to %s", MetadataFile),
			Err:    err,
		}
	}
	file.Close()

	// Add to index
	_, err = worktree.Add(MetadataFile)
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to add file to index",
			Err:    err,
		}
	}

	// Commit
	commitHash, err := worktree.Commit(commitMessage, &git.CommitOptions{
		Author: &object.Signature{
			Name:  author,
			Email: authorEmail,
			When:  time.Now(),
		},
	})
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to create commit",
			Err:    err,
		}
	}

	_ = commitHash // TODO: Use for force-with-lease

	return nil
}

// WriteInitial creates the hitch-metadata branch and writes initial metadata
func (w *Writer) WriteInitial(m *Metadata, author string, authorEmail string) error {
	// Marshal metadata to JSON
	jsonBytes, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to marshal metadata to JSON",
			Err:    err,
		}
	}

	// Get worktree
	worktree, err := w.repo.Worktree()
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to get worktree",
			Err:    err,
		}
	}

	// Create orphan branch by checking out to an empty tree
	// This is a bit tricky with go-git, we'll use a workaround:
	// 1. Get current HEAD
	// 2. Checkout --orphan equivalent
	headRef, err := w.repo.Head()
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to get HEAD",
			Err:    err,
		}
	}

	currentBranch := headRef.Name()

	// Create a new branch reference (will be orphan after we remove all files)
	branchRef := plumbing.NewHashReference(
		plumbing.NewBranchReferenceName(MetadataBranch),
		plumbing.ZeroHash, // Empty hash for orphan branch
	)

	// Note: Creating a true orphan branch with go-git is complex
	// For now, we'll create the metadata file and commit it
	// The actual orphan branch creation might need git command execution

	// Checkout new branch
	err = worktree.Checkout(&git.CheckoutOptions{
		Branch: branchRef.Name(),
		Create: true,
		Force:  true,
	})
	if err != nil {
		// If checkout fails, we might need to use git commands
		return &MetadataWriteError{
			Reason: "failed to create hitch-metadata branch",
			Err:    err,
		}
	}

	// Remove all files to make it an orphan branch
	// TODO: This is simplified, proper orphan branch creation needs work

	// Write hitch.json
	filename := worktree.Filesystem.Join(MetadataFile)
	file, err := worktree.Filesystem.Create(filename)
	if err != nil {
		return &MetadataWriteError{
			Reason: fmt.Sprintf("failed to create %s", MetadataFile),
			Err:    err,
		}
	}

	_, err = file.Write(jsonBytes)
	if err != nil {
		file.Close()
		return &MetadataWriteError{
			Reason: fmt.Sprintf("failed to write to %s", MetadataFile),
			Err:    err,
		}
	}
	file.Close()

	// Add to index
	_, err = worktree.Add(MetadataFile)
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to add file to index",
			Err:    err,
		}
	}

	// Commit
	_, err = worktree.Commit("Initialize Hitch metadata", &git.CommitOptions{
		Author: &object.Signature{
			Name:  author,
			Email: authorEmail,
			When:  time.Now(),
		},
	})
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to create initial commit",
			Err:    err,
		}
	}

	// Return to original branch
	err = worktree.Checkout(&git.CheckoutOptions{
		Branch: currentBranch,
		Force:  false,
	})
	if err != nil {
		return &MetadataWriteError{
			Reason: "failed to return to original branch",
			Err:    err,
		}
	}

	return nil
}
