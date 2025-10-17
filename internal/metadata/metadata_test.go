//go:build dockertest

package metadata_test

import (
	"testing"
	"time"

	"github.com/DoomedRamen/hitch/internal/metadata"
)

func TestMetadataInitialization(t *testing.T) {
	// Create new metadata with environments
	environments := []string{"dev", "qa"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)

	// Verify environments were created
	if len(meta.Environments) != 2 {
		t.Errorf("Expected 2 environments, got %d", len(meta.Environments))
	}

	if _, exists := meta.Environments["dev"]; !exists {
		t.Error("Environment 'dev' should exist")
	}

	if _, exists := meta.Environments["qa"]; !exists {
		t.Error("Environment 'qa' should exist")
	}

	// Verify config defaults
	if meta.Config.BaseBranch != "main" {
		t.Errorf("Expected base branch 'main', got '%s'", meta.Config.BaseBranch)
	}

	if meta.Config.RetentionDaysAfterMerge != 7 {
		t.Errorf("Expected retention days 7, got %d", meta.Config.RetentionDaysAfterMerge)
	}

	if meta.Config.StaleDaysNoActivity != 30 {
		t.Errorf("Expected stale days 30, got %d", meta.Config.StaleDaysNoActivity)
	}

	if meta.Config.LockTimeoutMinutes != 15 {
		t.Errorf("Expected lock timeout 15, got %d", meta.Config.LockTimeoutMinutes)
	}

	// Verify meta info
	if meta.Meta.InitializedBy != user {
		t.Errorf("Expected initialized by '%s', got '%s'", user, meta.Meta.InitializedBy)
	}

	if meta.Meta.LastModifiedBy != user {
		t.Errorf("Expected last modified by '%s', got '%s'", user, meta.Meta.LastModifiedBy)
	}

	// Verify branches map is initialized
	if meta.Branches == nil {
		t.Error("Branches map should be initialized")
	}
}

func TestAddBranchToEnvironment(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)

	// Add branch to environment
	err := meta.AddBranchToEnvironment("dev", "feature/test", user)
	if err != nil {
		t.Fatalf("Failed to add branch to environment: %v", err)
	}

	// Verify branch was added
	env := meta.Environments["dev"]
	if len(env.Features) != 1 {
		t.Fatalf("Expected 1 feature, got %d", len(env.Features))
	}

	if env.Features[0] != "feature/test" {
		t.Errorf("Expected feature 'feature/test', got '%s'", env.Features[0])
	}

	// Verify branch metadata
	branchInfo, exists := meta.Branches["feature/test"]
	if !exists {
		t.Fatal("Branch metadata should exist")
	}

	if len(branchInfo.PromotedTo) != 1 {
		t.Fatalf("Expected 1 promoted environment, got %d", len(branchInfo.PromotedTo))
	}

	if branchInfo.PromotedTo[0] != "dev" {
		t.Errorf("Expected promoted to 'dev', got '%s'", branchInfo.PromotedTo[0])
	}

	// Adding duplicate should be idempotent
	err = meta.AddBranchToEnvironment("dev", "feature/test", user)
	if err != nil {
		t.Errorf("Adding duplicate branch should not error: %v", err)
	}

	// Should still have only 1 feature
	env = meta.Environments["dev"]
	if len(env.Features) != 1 {
		t.Errorf("Expected 1 feature after duplicate add, got %d", len(env.Features))
	}
}

func TestRemoveBranchFromEnvironment(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)

	// Add then remove branch
	meta.AddBranchToEnvironment("dev", "feature/test", user)

	err := meta.RemoveBranchFromEnvironment("dev", "feature/test", user)
	if err != nil {
		t.Fatalf("Failed to remove branch: %v", err)
	}

	// Verify removal
	env := meta.Environments["dev"]
	if len(env.Features) != 0 {
		t.Errorf("Expected 0 features after removal, got %d", len(env.Features))
	}

	// Branch metadata should still exist but with no active promotions
	branchInfo := meta.Branches["feature/test"]
	if len(branchInfo.PromotedTo) != 0 {
		t.Errorf("Expected 0 active promotions after removal, got %d", len(branchInfo.PromotedTo))
	}

	// History should show demotion
	if len(branchInfo.PromotedHistory) == 0 {
		t.Error("Expected promotion history to exist")
	} else {
		lastEvent := branchInfo.PromotedHistory[len(branchInfo.PromotedHistory)-1]
		if lastEvent.DemotedAt == nil {
			t.Error("Expected demotion timestamp in history")
		}
	}
}

func TestLockUnlockEnvironment(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)

	// Lock environment
	err := meta.LockEnvironment("dev", user, "Testing lock")
	if err != nil {
		t.Fatalf("Failed to lock environment: %v", err)
	}

	env := meta.Environments["dev"]
	if !env.Locked {
		t.Error("Environment should be locked")
	}

	if env.LockedBy != user {
		t.Errorf("Expected locked by '%s', got '%s'", user, env.LockedBy)
	}

	if env.LockedReason != "Testing lock" {
		t.Errorf("Expected reason 'Testing lock', got '%s'", env.LockedReason)
	}

	// Unlock environment
	err = meta.UnlockEnvironment("dev")
	if err != nil {
		t.Fatalf("Failed to unlock environment: %v", err)
	}

	env = meta.Environments["dev"]
	if env.Locked {
		t.Error("Environment should be unlocked")
	}

	if env.LockedBy != "" {
		t.Error("LockedBy should be empty after unlock")
	}
}

func TestIsLockStale(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)
	meta.Config.LockTimeoutMinutes = 30

	// Fresh lock
	meta.LockEnvironment("dev", user, "Test")
	if meta.IsLockStale("dev") {
		t.Error("Fresh lock should not be stale")
	}

	// Make lock old
	env := meta.Environments["dev"]
	oldTime := time.Now().Add(-2 * time.Hour)
	env.LockedAt = oldTime
	meta.Environments["dev"] = env

	if !meta.IsLockStale("dev") {
		t.Error("Old lock should be stale")
	}
}

func TestBranchCleanupEligibility(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)
	meta.Config.RetentionDaysAfterMerge = 7

	// Add branch
	meta.AddBranchToEnvironment("dev", "feature/test", user)

	branchInfo := meta.Branches["feature/test"]

	// Not merged yet
	if branchInfo.IsEligibleForCleanup() {
		t.Error("Unmerged branch should not be eligible for cleanup")
	}

	// Just merged
	now := time.Now()
	branchInfo.MergedToMainAt = &now
	tomorrow := now.Add(24 * time.Hour)
	branchInfo.EligibleForCleanupAt = &tomorrow
	meta.Branches["feature/test"] = branchInfo

	if branchInfo.IsEligibleForCleanup() {
		t.Error("Recently merged branch should not be eligible yet")
	}

	// Past cleanup date
	yesterday := now.Add(-24 * time.Hour)
	branchInfo.EligibleForCleanupAt = &yesterday
	meta.Branches["feature/test"] = branchInfo

	if !branchInfo.IsEligibleForCleanup() {
		t.Error("Branch past cleanup date should be eligible")
	}
}

func TestEnvironmentNotFound(t *testing.T) {
	environments := []string{"dev"}
	baseBranch := "main"
	user := "test@example.com"

	meta := metadata.NewMetadata(environments, baseBranch, user)

	// Try to add branch to non-existent environment
	err := meta.AddBranchToEnvironment("production", "feature/test", user)
	if err == nil {
		t.Error("Expected error when adding to non-existent environment")
	}

	// Try to lock non-existent environment
	err = meta.LockEnvironment("production", user, "test")
	if err == nil {
		t.Error("Expected error when locking non-existent environment")
	}
}
