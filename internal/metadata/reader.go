package metadata

import (
	"encoding/json"
	"fmt"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
)

const (
	MetadataBranch = "hitch-metadata"
	MetadataFile   = "hitch.json"
)

// Reader handles reading metadata from the hitch-metadata branch
type Reader struct {
	repo *git.Repository
}

// NewReader creates a new metadata reader
func NewReader(repo *git.Repository) *Reader {
	return &Reader{repo: repo}
}

// Read reads the metadata from the hitch-metadata branch
func (r *Reader) Read() (*Metadata, error) {
	// Get reference to hitch-metadata branch
	ref, err := r.repo.Reference(plumbing.NewBranchReferenceName(MetadataBranch), true)
	if err != nil {
		return nil, &MetadataReadError{
			Reason: fmt.Sprintf("hitch-metadata branch not found (has 'hitch init' been run?)"),
			Err:    err,
		}
	}

	// Get commit
	commit, err := r.repo.CommitObject(ref.Hash())
	if err != nil {
		return nil, &MetadataReadError{
			Reason: "failed to get commit from hitch-metadata branch",
			Err:    err,
		}
	}

	// Get tree
	tree, err := commit.Tree()
	if err != nil {
		return nil, &MetadataReadError{
			Reason: "failed to get tree from commit",
			Err:    err,
		}
	}

	// Get hitch.json file
	file, err := tree.File(MetadataFile)
	if err != nil {
		return nil, &MetadataReadError{
			Reason: fmt.Sprintf("%s not found in hitch-metadata branch", MetadataFile),
			Err:    err,
		}
	}

	// Read file contents
	contents, err := file.Contents()
	if err != nil {
		return nil, &MetadataReadError{
			Reason: fmt.Sprintf("failed to read %s contents", MetadataFile),
			Err:    err,
		}
	}

	// Parse JSON
	var metadata Metadata
	if err := json.Unmarshal([]byte(contents), &metadata); err != nil {
		return nil, &InvalidMetadataError{
			Reason: "failed to parse JSON",
			Err:    err,
		}
	}

	// Validate
	if err := r.validate(&metadata); err != nil {
		return nil, err
	}

	return &metadata, nil
}

// Exists checks if the hitch-metadata branch exists
func (r *Reader) Exists() bool {
	_, err := r.repo.Reference(plumbing.NewBranchReferenceName(MetadataBranch), true)
	return err == nil
}

// validate performs basic validation on metadata
func (r *Reader) validate(m *Metadata) error {
	if m.Version == "" {
		return &InvalidMetadataError{Reason: "version is required"}
	}

	if m.Environments == nil {
		return &InvalidMetadataError{Reason: "environments is required"}
	}

	if m.Config.BaseBranch == "" {
		return &InvalidMetadataError{Reason: "config.base_branch is required"}
	}

	return nil
}
