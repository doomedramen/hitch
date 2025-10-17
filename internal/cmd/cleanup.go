package cmd

import (
	"bufio"
	"fmt"
	"os"
	"strings"
	"time"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var (
	cleanupDryRun bool
	cleanupForce  bool
)

var cleanupCmd = &cobra.Command{
	Use:   "cleanup",
	Short: "Clean up stale feature branches",
	Long: `Clean up feature branches that are safe to delete.

A branch is safe to delete if:
- It has been merged to main
- It has passed the retention period (configured days after merge)
- It is not currently in any environment

Example:
  hitch cleanup           # Interactive cleanup
  hitch cleanup --dry-run # Show what would be deleted
  hitch cleanup --force   # Delete without confirmation`,
	RunE: runCleanup,
}

func init() {
	cleanupCmd.Flags().BoolVar(&cleanupDryRun, "dry-run", false, "Show what would be deleted without actually deleting")
	cleanupCmd.Flags().BoolVarP(&cleanupForce, "force", "f", false, "Delete without confirmation")
	rootCmd.AddCommand(cleanupCmd)
}

func runCleanup(cmd *cobra.Command, args []string) error {
	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		return err
	}

	// 2. Get current branch to return to
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		errorMsg("Failed to get current branch")
		return err
	}
	defer func() {
		_ = repo.Checkout(currentBranch)
	}()

	// 3. Read metadata
	reader := metadata.NewReader(repo.Repository)
	if !reader.Exists() {
		errorMsg("Hitch is not initialized in this repository")
		fmt.Println("\nRun 'hitch init' to initialize Hitch.")
		return fmt.Errorf("hitch not initialized")
	}

	meta, err := reader.Read()
	if err != nil {
		errorMsg("Failed to read metadata")
		return err
	}

	// 4. Find stale branches
	safeToDelete := []string{}
	inactive := []string{}

	for branchName, info := range meta.Branches {
		// Safe to delete if merged + past retention period + not in any env
		if info.MergedToMainAt != nil {
			daysSinceMerge := int(time.Since(*info.MergedToMainAt).Hours() / 24)
			if daysSinceMerge > meta.Config.RetentionDaysAfterMerge {
				// Check not in any environment
				inEnv := false
				for _, env := range meta.Environments {
					for _, f := range env.Features {
						if f == branchName {
							inEnv = true
							break
						}
					}
					if inEnv {
						break
					}
				}
				if !inEnv {
					safeToDelete = append(safeToDelete, branchName)
				}
			}
		}

		// Warn about inactive branches (but don't delete)
		if info.MergedToMainAt == nil && !info.LastCommitAt.IsZero() {
			daysSinceCommit := int(time.Since(info.LastCommitAt).Hours() / 24)
			if daysSinceCommit > meta.Config.StaleDaysNoActivity {
				inactive = append(inactive, branchName)
			}
		}
	}

	// 5. Display results
	if len(safeToDelete) == 0 && len(inactive) == 0 {
		success("No stale branches found")
		return nil
	}

	if len(safeToDelete) > 0 {
		color.New(color.Bold).Println("Branches safe to delete (merged to main):")
		for _, branch := range safeToDelete {
			info := meta.Branches[branch]
			daysSinceMerge := int(time.Since(*info.MergedToMainAt).Hours() / 24)
			fmt.Printf("  ✓ %s (merged %d days ago)\n", branch, daysSinceMerge)
		}
		fmt.Println()
	}

	if len(inactive) > 0 {
		color.New(color.Bold).Println("Inactive branches (no recent commits):")
		for _, branch := range inactive {
			info := meta.Branches[branch]
			daysSinceCommit := int(time.Since(info.LastCommitAt).Hours() / 24)
			fmt.Printf("  ? %s (last commit %d days ago)\n", branch, daysSinceCommit)
		}
		fmt.Println()
		warning("Inactive branches are NOT automatically deleted. Review and delete manually if needed.")
		fmt.Println()
	}

	if len(safeToDelete) == 0 {
		return nil
	}

	// 6. Dry run mode
	if cleanupDryRun {
		info(fmt.Sprintf("Dry run: would delete %d branches", len(safeToDelete)))
		return nil
	}

	// 7. Confirm deletion
	if !cleanupForce {
		fmt.Printf("Delete %d branches? [y/N]: ", len(safeToDelete))
		reader := bufio.NewReader(os.Stdin)
		response, err := reader.ReadString('\n')
		if err != nil {
			return err
		}
		response = strings.TrimSpace(strings.ToLower(response))
		if response != "y" && response != "yes" {
			info("Cleanup cancelled")
			return nil
		}
	}

	// 8. Get user info for metadata update
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Failed to get user email from git config")
		return err
	}
	userName, err := repo.UserName()
	if err != nil {
		errorMsg("Failed to get user name from git config")
		return err
	}

	// 9. Delete branches
	deletedCount := 0
	for _, branch := range safeToDelete {
		// Delete local branch
		if err := repo.DeleteBranch(branch, true); err != nil {
			warning(fmt.Sprintf("Failed to delete local branch %s: %v", branch, err))
			continue
		}

		// Delete remote branch (if exists)
		if err := repo.DeleteRemoteBranch("origin", branch); err != nil {
			// This is OK if remote doesn't exist or branch wasn't pushed
			if verbose {
				warning(fmt.Sprintf("Could not delete remote branch %s (may not exist): %v", branch, err))
			}
		}

		// Remove from metadata
		delete(meta.Branches, branch)
		deletedCount++
		success(fmt.Sprintf("Deleted %s", branch))
	}

	// 10. Update metadata
	if deletedCount > 0 {
		meta.UpdateMeta(userEmail, "hitch cleanup")
		writer := metadata.NewWriter(repo.Repository)
		if err := writer.Write(meta, userName, userEmail, fmt.Sprintf("Clean up %d stale branches", deletedCount)); err != nil {
			errorMsg("Failed to update metadata")
			return err
		}
	}

	success(fmt.Sprintf("Deleted %d branches", deletedCount))

	return nil
}
