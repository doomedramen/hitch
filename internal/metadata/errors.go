package metadata

import (
	"fmt"
	"time"
)

// EnvironmentNotFoundError is returned when an environment doesn't exist
type EnvironmentNotFoundError struct {
	Environment string
}

func (e *EnvironmentNotFoundError) Error() string {
	return fmt.Sprintf("environment '%s' not found", e.Environment)
}

// EnvironmentLockedError is returned when an environment is locked by another user
type EnvironmentLockedError struct {
	Environment string
	LockedBy    string
	LockedAt    time.Time
}

func (e *EnvironmentLockedError) Error() string {
	return fmt.Sprintf("environment '%s' is locked by %s (since %s)",
		e.Environment, e.LockedBy, e.LockedAt.Format(time.RFC3339))
}

// BranchNotFoundError is returned when a branch doesn't exist
type BranchNotFoundError struct {
	Branch string
}

func (e *BranchNotFoundError) Error() string {
	return fmt.Sprintf("branch '%s' not found", e.Branch)
}

// MetadataReadError is returned when metadata cannot be read
type MetadataReadError struct {
	Reason string
	Err    error
}

func (e *MetadataReadError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("failed to read metadata: %s: %v", e.Reason, e.Err)
	}
	return fmt.Sprintf("failed to read metadata: %s", e.Reason)
}

func (e *MetadataReadError) Unwrap() error {
	return e.Err
}

// MetadataWriteError is returned when metadata cannot be written
type MetadataWriteError struct {
	Reason string
	Err    error
}

func (e *MetadataWriteError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("failed to write metadata: %s: %v", e.Reason, e.Err)
	}
	return fmt.Sprintf("failed to write metadata: %s", e.Reason)
}

func (e *MetadataWriteError) Unwrap() error {
	return e.Err
}

// InvalidMetadataError is returned when metadata format is invalid
type InvalidMetadataError struct {
	Reason string
	Err    error
}

func (e *InvalidMetadataError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("invalid metadata: %s: %v", e.Reason, e.Err)
	}
	return fmt.Sprintf("invalid metadata: %s", e.Reason)
}

func (e *InvalidMetadataError) Unwrap() error {
	return e.Err
}
